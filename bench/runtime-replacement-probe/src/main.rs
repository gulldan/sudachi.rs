/*
 * PROBE (not for upstream). Tests runtime dictionary-lookup replacements:
 * byte-wise trie, char-wise trie, and single-pass Aho-Corasick automata.
 */

use std::cmp::Ordering;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crawdad::{MpTrie as CrawdadMpTrie, Trie as CrawdadTrie};
use daachorse::{CharwiseDoubleArrayAhoCorasick, DoubleArrayAhoCorasick};
use lexime_trie::{DoubleArray as LeximeDoubleArray, TrieSearch};
use yada::DoubleArray as YadaDoubleArray;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct Hit {
    line: u32,
    start: u32,
    end: u32,
    value: u32,
}

struct CharLine<'a> {
    text: &'a str,
    chars: Vec<char>,
    char_to_byte: Vec<usize>,
}

fn env_paths(var: &str, defaults: &[&str]) -> Vec<PathBuf> {
    std::env::var(var)
        .ok()
        .map(|raw| {
            raw.split(':')
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
                .collect()
        })
        .unwrap_or_else(|| defaults.iter().map(PathBuf::from).collect())
}

fn env_path(var: &str, default: &str) -> PathBuf {
    std::env::var(var)
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(default))
}

fn env_usize(var: &str, default: usize) -> usize {
    std::env::var(var)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

fn load_index_forms(paths: &[PathBuf]) -> Vec<(String, u32)> {
    let mut seen = HashSet::new();
    let mut keys = Vec::new();
    for path in paths {
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .from_path(path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
        for record in reader.records() {
            let record =
                record.unwrap_or_else(|err| panic!("failed to parse {}: {err}", path.display()));
            let Some(index_form) = record.get(0) else {
                continue;
            };
            if index_form.is_empty() || !seen.insert(index_form.to_owned()) {
                continue;
            }
            keys.push(index_form.to_owned());
        }
    }
    keys.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    keys.into_iter()
        .enumerate()
        .map(|(i, key)| (key, i as u32))
        .collect()
}

fn collect_lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()))
        .lines()
        .map(str::to_owned)
        .collect()
}

fn make_char_lines(lines: &[String]) -> Vec<CharLine<'_>> {
    lines
        .iter()
        .map(|line| {
            let mut chars = Vec::new();
            let mut char_to_byte = Vec::new();
            for (byte, ch) in line.char_indices() {
                char_to_byte.push(byte);
                chars.push(ch);
            }
            char_to_byte.push(line.len());
            CharLine {
                text: line,
                chars,
                char_to_byte,
            }
        })
        .collect()
}

fn build_yada(entries: &[(String, u32)]) -> Vec<u8> {
    let keyset = entries
        .iter()
        .map(|(key, value)| (key.as_bytes(), *value))
        .collect::<Vec<_>>();
    yada::builder::DoubleArrayBuilder::build(&keyset).expect("yada build failed")
}

fn build_lexime(entries: &[(String, u32)]) -> LeximeDoubleArray<u8> {
    let keys = entries
        .iter()
        .map(|(key, _)| key.as_bytes())
        .collect::<Vec<_>>();
    LeximeDoubleArray::<u8>::build(&keys)
}

fn build_crawdad(entries: &[(String, u32)]) -> CrawdadTrie {
    CrawdadTrie::from_records(entries.iter().map(|(key, value)| (key.as_str(), *value)))
        .expect("crawdad trie build failed")
}

fn build_crawdad_mp(entries: &[(String, u32)]) -> CrawdadMpTrie {
    CrawdadMpTrie::from_records(entries.iter().map(|(key, value)| (key.as_str(), *value)))
        .expect("crawdad mptrie build failed")
}

fn build_byte_daac(entries: &[(String, u32)]) -> DoubleArrayAhoCorasick<u32> {
    DoubleArrayAhoCorasick::with_values(entries.iter().map(|(key, value)| (key.as_str(), *value)))
        .expect("byte daachorse build failed")
}

fn build_char_daac(entries: &[(String, u32)]) -> CharwiseDoubleArrayAhoCorasick<u32> {
    CharwiseDoubleArrayAhoCorasick::with_values(
        entries.iter().map(|(key, value)| (key.as_str(), *value)),
    )
    .expect("char daachorse build failed")
}

fn collect_yada(bytes: &[u8], lines: &[String]) -> Vec<Hit> {
    let da = YadaDoubleArray::new(bytes);
    let mut hits = Vec::new();
    for (line_idx, line) in lines.iter().enumerate() {
        for (start, _) in line.char_indices() {
            for (value, len) in da.common_prefix_search(&line.as_bytes()[start..]) {
                hits.push(Hit {
                    line: line_idx as u32,
                    start: start as u32,
                    end: (start + len) as u32,
                    value,
                });
            }
        }
    }
    hits.sort_unstable();
    hits
}

fn collect_lexime(trie: &LeximeDoubleArray<u8>, lines: &[String]) -> Vec<Hit> {
    let mut hits = Vec::new();
    for (line_idx, line) in lines.iter().enumerate() {
        for (start, _) in line.char_indices() {
            for m in trie.common_prefix_search(&line.as_bytes()[start..]) {
                hits.push(Hit {
                    line: line_idx as u32,
                    start: start as u32,
                    end: (start + m.len) as u32,
                    value: m.value_id,
                });
            }
        }
    }
    hits.sort_unstable();
    hits
}

fn collect_crawdad(trie: &CrawdadTrie, char_lines: &[CharLine<'_>]) -> Vec<Hit> {
    let mut hits = Vec::new();
    for (line_idx, line) in char_lines.iter().enumerate() {
        for start_char in 0..line.chars.len() {
            let start = line.char_to_byte[start_char];
            for (value, len_chars) in
                trie.common_prefix_search(line.chars[start_char..].iter().copied())
            {
                hits.push(Hit {
                    line: line_idx as u32,
                    start: start as u32,
                    end: line.char_to_byte[start_char + len_chars] as u32,
                    value,
                });
            }
        }
    }
    hits.sort_unstable();
    hits
}

fn collect_crawdad_mp(trie: &CrawdadMpTrie, char_lines: &[CharLine<'_>]) -> Vec<Hit> {
    let mut hits = Vec::new();
    for (line_idx, line) in char_lines.iter().enumerate() {
        for start_char in 0..line.chars.len() {
            let start = line.char_to_byte[start_char];
            for (value, len_chars) in
                trie.common_prefix_search(line.chars[start_char..].iter().copied())
            {
                hits.push(Hit {
                    line: line_idx as u32,
                    start: start as u32,
                    end: line.char_to_byte[start_char + len_chars] as u32,
                    value,
                });
            }
        }
    }
    hits.sort_unstable();
    hits
}

fn collect_byte_daac(ac: &DoubleArrayAhoCorasick<u32>, lines: &[String]) -> Vec<Hit> {
    let mut hits = Vec::new();
    for (line_idx, line) in lines.iter().enumerate() {
        for m in ac.find_overlapping_iter(line.as_bytes()) {
            hits.push(Hit {
                line: line_idx as u32,
                start: m.start() as u32,
                end: m.end() as u32,
                value: m.value(),
            });
        }
    }
    hits.sort_unstable();
    hits
}

fn collect_char_daac(ac: &CharwiseDoubleArrayAhoCorasick<u32>, lines: &[String]) -> Vec<Hit> {
    let mut hits = Vec::new();
    for (line_idx, line) in lines.iter().enumerate() {
        for m in ac.find_overlapping_iter(line) {
            hits.push(Hit {
                line: line_idx as u32,
                start: m.start() as u32,
                end: m.end() as u32,
                value: m.value(),
            });
        }
    }
    hits.sort_unstable();
    hits
}

fn compare(name: &str, baseline: &[Hit], got: &[Hit]) {
    match baseline.cmp(got) {
        Ordering::Equal => println!("# parity {name}: OK ({})", got.len()),
        _ => {
            let first = baseline
                .iter()
                .zip(got.iter())
                .position(|(a, b)| a != b)
                .unwrap_or_else(|| baseline.len().min(got.len()));
            panic!(
                "{name} parity mismatch: baseline={} got={} first_diff={first} base={:?} got={:?}",
                baseline.len(),
                got.len(),
                baseline.get(first),
                got.get(first)
            );
        }
    }
}

fn median_ms<F>(trials: usize, mut f: F) -> (usize, f64)
where
    F: FnMut() -> usize,
{
    let mut times = Vec::with_capacity(trials);
    let mut count = 0;
    for _ in 0..trials {
        let start = Instant::now();
        count = f();
        times.push(start.elapsed().as_secs_f64() * 1e3);
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    (count, times[times.len() / 2])
}

fn count_yada(bytes: &[u8], lines: &[String]) -> usize {
    let da = YadaDoubleArray::new(bytes);
    let mut count = 0;
    for line in lines {
        for (start, _) in line.char_indices() {
            count += da.common_prefix_search(&line.as_bytes()[start..]).count();
        }
    }
    count
}

fn count_lexime(trie: &LeximeDoubleArray<u8>, lines: &[String]) -> usize {
    let mut count = 0;
    for line in lines {
        for (start, _) in line.char_indices() {
            count += trie.common_prefix_search(&line.as_bytes()[start..]).count();
        }
    }
    count
}

fn count_crawdad(trie: &CrawdadTrie, char_lines: &[CharLine<'_>]) -> usize {
    let mut count = 0;
    for line in char_lines {
        for start_char in 0..line.chars.len() {
            count += trie
                .common_prefix_search(line.chars[start_char..].iter().copied())
                .count();
        }
    }
    count
}

fn count_crawdad_mp(trie: &CrawdadMpTrie, char_lines: &[CharLine<'_>]) -> usize {
    let mut count = 0;
    for line in char_lines {
        for start_char in 0..line.chars.len() {
            count += trie
                .common_prefix_search(line.chars[start_char..].iter().copied())
                .count();
        }
    }
    count
}

fn count_byte_daac(ac: &DoubleArrayAhoCorasick<u32>, lines: &[String]) -> usize {
    lines
        .iter()
        .map(|line| ac.find_overlapping_iter(line.as_bytes()).count())
        .sum()
}

fn count_char_daac(ac: &CharwiseDoubleArrayAhoCorasick<u32>, lines: &[String]) -> usize {
    lines
        .iter()
        .map(|line| ac.find_overlapping_iter(line).count())
        .sum()
}

fn report_time<F>(name: &str, chars: usize, trials: usize, f: F)
where
    F: FnMut() -> usize,
{
    let (count, ms) = median_ms(trials, f);
    println!(
        "{name:<16} matches {count:>8} median_ms {ms:>8.2} ns/char {:>7.2}",
        ms * 1e6 / chars.max(1) as f64
    );
}

fn main() {
    let lexicons = env_paths(
        "SUDACHI_LAYOUT_LEXICONS",
        &[
            "target/bench-lookup/raw/unzipped/small/small_lex.csv",
            "target/bench-lookup/raw/unzipped/core/core_lex.csv",
            "target/bench-lookup/raw/unzipped/notcore/notcore_lex.csv",
        ],
    );
    let corpus_path = env_path(
        "SUDACHI_BENCH_INPUTS",
        "target/issue-117-corpora/kyoto-leads.txt",
    );
    let trials = env_usize("SUDACHI_RUNTIME_TRIALS", 11);

    let t0 = Instant::now();
    let entries = load_index_forms(&lexicons);
    println!(
        "# keys: {} loaded_ms: {:.1}",
        entries.len(),
        t0.elapsed().as_secs_f64() * 1e3
    );

    let lines = collect_lines(&corpus_path);
    let chars = lines.iter().map(|l| l.chars().count()).sum::<usize>();
    let bytes = lines.iter().map(|l| l.len()).sum::<usize>();
    println!(
        "# corpus: lines={} chars={} bytes={} trials={}",
        lines.len(),
        chars,
        bytes,
        trials
    );

    let t_chars = Instant::now();
    let char_lines = make_char_lines(&lines);
    println!(
        "# char_line_maps_ms: {:.1}",
        t_chars.elapsed().as_secs_f64() * 1e3
    );
    if let Some(first) = char_lines.first() {
        println!(
            "# first_line_chars={} bytes={}",
            first.chars.len(),
            first.text.len()
        );
    }

    let t = Instant::now();
    let yada = build_yada(&entries);
    println!(
        "# build_yada_ms: {:.1} bytes={}",
        t.elapsed().as_secs_f64() * 1e3,
        yada.len()
    );

    let t = Instant::now();
    let lexime = build_lexime(&entries);
    let lexime_bytes = lexime.as_bytes();
    println!(
        "# build_lexime_ms: {:.1} bytes={} node_slots={}",
        t.elapsed().as_secs_f64() * 1e3,
        lexime_bytes.len(),
        lexime.node_slot_count()
    );

    let t = Instant::now();
    let crawdad = build_crawdad(&entries);
    println!(
        "# build_crawdad_ms: {:.1} heap={} io={}",
        t.elapsed().as_secs_f64() * 1e3,
        crawdad.heap_bytes(),
        crawdad.io_bytes()
    );

    let t = Instant::now();
    let crawdad_mp = build_crawdad_mp(&entries);
    println!(
        "# build_crawdad_mp_ms: {:.1} heap={} io={}",
        t.elapsed().as_secs_f64() * 1e3,
        crawdad_mp.heap_bytes(),
        crawdad_mp.io_bytes()
    );

    let t = Instant::now();
    let byte_daac = build_byte_daac(&entries);
    let byte_daac_serialized = byte_daac.serialize();
    println!(
        "# build_byte_daac_ms: {:.1} heap={} bytes={}",
        t.elapsed().as_secs_f64() * 1e3,
        byte_daac.heap_bytes(),
        byte_daac_serialized.len()
    );

    let t = Instant::now();
    let char_daac = build_char_daac(&entries);
    let char_daac_serialized = char_daac.serialize();
    println!(
        "# build_char_daac_ms: {:.1} heap={} bytes={}",
        t.elapsed().as_secs_f64() * 1e3,
        char_daac.heap_bytes(),
        char_daac_serialized.len()
    );

    let t = Instant::now();
    let baseline = collect_yada(&yada, &lines);
    println!("# collect_yada_ms: {:.1}", t.elapsed().as_secs_f64() * 1e3);

    compare("lexime", &baseline, &collect_lexime(&lexime, &lines));
    compare(
        "crawdad",
        &baseline,
        &collect_crawdad(&crawdad, &char_lines),
    );
    compare(
        "crawdad_mp",
        &baseline,
        &collect_crawdad_mp(&crawdad_mp, &char_lines),
    );
    compare(
        "byte_daac",
        &baseline,
        &collect_byte_daac(&byte_daac, &lines),
    );
    compare(
        "char_daac",
        &baseline,
        &collect_char_daac(&char_daac, &lines),
    );

    report_time("yada", chars, trials, || count_yada(&yada, &lines));
    report_time("lexime", chars, trials, || count_lexime(&lexime, &lines));
    report_time("crawdad", chars, trials, || {
        count_crawdad(&crawdad, &char_lines)
    });
    report_time("crawdad_mp", chars, trials, || {
        count_crawdad_mp(&crawdad_mp, &char_lines)
    });
    report_time("byte_daac", chars, trials, || {
        count_byte_daac(&byte_daac, &lines)
    });
    report_time("char_daac", chars, trials, || {
        count_char_daac(&char_daac, &lines)
    });
}
