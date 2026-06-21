/*
 * PROBE (not for upstream). Compares external double-array builders against
 * the current yada/Sudachi byte-level double-array format.
 */

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use darts_clone_rs::darts::DoubleArrayTrie;
use sudachi::dic::lexicon::trie::Trie;
use yada::DoubleArray;

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

fn load_index_forms(paths: &[PathBuf]) -> Vec<(Vec<u8>, u32)> {
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
            keys.push((index_form.as_bytes().to_vec(), keys.len() as u32));
        }
    }
    keys.sort_by(|a, b| a.0.cmp(&b.0));
    keys
}

fn collect_lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()))
        .lines()
        .map(str::to_owned)
        .collect()
}

fn unit_at(bytes: &[u8], idx: usize) -> Option<u32> {
    let start = idx.checked_mul(4)?;
    let raw = bytes.get(start..start + 4)?;
    Some(u32::from_le_bytes(raw.try_into().ok()?))
}

#[inline]
fn label(unit: u32) -> usize {
    unit as usize & ((1 << 31) | 0xFF)
}

#[inline]
fn offset(unit: u32) -> usize {
    ((unit as usize) >> 10) << (((unit as usize) & (1 << 9)) >> 6)
}

fn line_footprint(da: &[u8], lines: &[&str], line_size: usize) -> usize {
    let root = offset(unit_at(da, 0).expect("empty trie"));
    let mut touched = HashSet::new();
    for line in lines {
        let input = line.as_bytes();
        for (start, _) in line.char_indices() {
            let mut node_pos = root;
            for &byte in &input[start..] {
                node_pos ^= byte as usize;
                touched.insert((node_pos * 4) / line_size);
                let Some(unit) = unit_at(da, node_pos) else {
                    break;
                };
                if label(unit) != byte as usize {
                    break;
                }
                node_pos ^= offset(unit);
            }
        }
    }
    touched.len()
}

fn validate_exact(keys: &[(Vec<u8>, u32)], da_bytes: &[u8]) {
    let da = DoubleArray::new(da_bytes);
    for (key, value) in keys {
        assert_eq!(
            da.exact_match_search(key),
            Some(*value),
            "bad key {:?}",
            String::from_utf8_lossy(key)
        );
    }
}

fn validate_exact_sudachi(name: &str, keys: &[(Vec<u8>, u32)], da_bytes: &[u8]) {
    let trie = Trie::from_bytes(da_bytes);
    for (key, value) in keys {
        let got = trie
            .common_prefix_iterator(key, 0)
            .find(|entry| entry.end == key.len())
            .map(|entry| entry.value);
        assert_eq!(
            got,
            Some(*value),
            "{name}: bad key {:?}",
            String::from_utf8_lossy(key)
        );
    }
}

fn common_prefix_count(da_bytes: &[u8], lines: &[&str]) -> usize {
    let trie = Trie::from_bytes(da_bytes);
    let mut count = 0usize;
    for line in lines {
        let input = line.as_bytes();
        for (start, _) in line.char_indices() {
            count += trie.common_prefix_iterator(input, start).count();
        }
    }
    count
}

fn timed_count(name: &str, da: &[u8], lines: &[&str]) -> (usize, f64) {
    let start = Instant::now();
    let count = common_prefix_count(da, lines);
    let ms = start.elapsed().as_secs_f64() * 1e3;
    println!("{name:<12} matches {count:>10} time_ms {ms:>8.2}");
    (count, ms)
}

fn build_darts(keys: &[(Vec<u8>, u32)]) -> Vec<u8> {
    let strings = keys
        .iter()
        .map(|(k, _)| String::from_utf8(k.clone()).expect("non-utf8 key"))
        .collect::<Vec<_>>();
    let values = keys
        .iter()
        .map(|(_, v)| i32::try_from(*v).expect("value too large for darts"))
        .collect::<Vec<_>>();

    let darts = DoubleArrayTrie::new();
    darts
        .build(strings.len(), &strings, None, Some(&values), None)
        .expect("darts build failed");

    let path = std::env::temp_dir().join("sudachi-darts-builder-probe.bin");
    darts
        .save(path.to_str().expect("non-utf8 temp path"), "wb", 0)
        .expect("darts save failed");
    std::fs::read(path).expect("darts read failed")
}

fn report_builder(
    name: &str,
    bytes: &[u8],
    keys: &[(Vec<u8>, u32)],
    lines: &[&str],
) -> (usize, f64) {
    validate_exact(keys, bytes);
    validate_exact_sudachi(name, keys, bytes);
    let (count, ms) = timed_count(name, bytes, lines);
    for line_size in [64usize, 128] {
        let lines_touched = line_footprint(bytes, lines, line_size);
        println!(
            "# {name} footprint {line_size}B: {lines_touched} lines ({:.2} MiB)",
            lines_touched as f64 * line_size as f64 / (1 << 20) as f64
        );
    }
    (count, ms)
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
    let corpus = collect_lines(&corpus_path);
    let lines = corpus.iter().map(String::as_str).collect::<Vec<_>>();

    let t0 = Instant::now();
    let keys = load_index_forms(&lexicons);
    println!(
        "# keys: {} loaded_ms: {:.1}",
        keys.len(),
        t0.elapsed().as_secs_f64() * 1e3
    );

    let t1 = Instant::now();
    let yada = yada::builder::DoubleArrayBuilder::build(&keys).expect("yada build failed");
    println!(
        "# yada bytes: {} build_ms: {:.1}",
        yada.len(),
        t1.elapsed().as_secs_f64() * 1e3
    );

    let t2 = Instant::now();
    let tried = tried::DoubleArrayBuilder::build(&keys).expect("tried build failed");
    println!(
        "# tried bytes: {} build_ms: {:.1}",
        tried.len(),
        t2.elapsed().as_secs_f64() * 1e3
    );

    let t3 = Instant::now();
    let darts = build_darts(&keys);
    println!(
        "# darts bytes: {} build_ms: {:.1}",
        darts.len(),
        t3.elapsed().as_secs_f64() * 1e3
    );

    let t4 = Instant::now();
    println!("# bytes_eq tried/yada: {}", tried == yada);
    println!("# bytes_eq darts/yada: {}", darts == yada);
    let (yada_count, yada_ms) = report_builder("yada", &yada, &keys, &lines);
    let (tried_count, tried_ms) = report_builder("tried", &tried, &keys, &lines);
    let (darts_count, darts_ms) = report_builder("darts", &darts, &keys, &lines);
    println!(
        "# validation/report_ms: {:.1}",
        t4.elapsed().as_secs_f64() * 1e3
    );

    assert_eq!(yada_count, tried_count, "tried common-prefix count changed");
    assert_eq!(yada_count, darts_count, "darts common-prefix count changed");
    println!(
        "# tried time delta vs yada: {:+.1}%",
        (tried_ms / yada_ms - 1.0) * 100.0
    );
    println!(
        "# darts time delta vs yada: {:+.1}%",
        (darts_ms / yada_ms - 1.0) * 100.0
    );
}
