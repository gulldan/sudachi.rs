/*
 * Copyright (c) 2021-2026 Works Applications Co., Ltd.
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

//! Isolated common-prefix lookup A/B: the BYTE-wise double-array trie Sudachi
//! actually uses (yada, 3 steps per kanji) vs a CHAR-wise automaton (daachorse,
//! 1 step per kanji). Both are built over the SAME dictionary headwords with the
//! same value = sorted index. Tests the literature claim (char-wise ~up to 2× the
//! trie) on Sudachi's real keyset + corpus, and bounds the end-to-end effect
//! (the trie is ~9% of do_tokenize).
//!
//! - correctness: the full (start, end, value) match set over the corpus must be
//!   IDENTICAL between the two structures (asserted) — otherwise the speed number
//!   is meaningless.
//! - speed: total lookup time over the corpus (yada = per-boundary common-prefix;
//!   daachorse = one overlapping pass) and ns/char.
//! - memory: serialized/heap bytes.
//!
//! SUDACHI_BENCH_CONFIG  SUDACHI_BENCH_DICT  SUDACHI_BENCH_INPUTS  SUDACHI_BENCH_TRIALS

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;

use daachorse::CharwiseDoubleArrayAhoCorasick;
use sudachi::config::Config;
use sudachi::dic::dictionary::JapaneseDictionary;
use sudachi::dic::subset::InfoSubset;
use yada::builder::DoubleArrayBuilder;
use yada::DoubleArray;

fn env_path(key: &str, default: &str) -> PathBuf {
    std::env::var_os(key).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(default))
}

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn load_dict() -> JapaneseDictionary {
    let config_path = env_path("SUDACHI_BENCH_CONFIG", "resources/sudachi.json");
    let resource_dir = config_path
        .parent()
        .map(|p| p.to_path_buf())
        .filter(|p| !p.as_os_str().is_empty());
    let dict_override = std::env::var_os("SUDACHI_BENCH_DICT").map(PathBuf::from);
    let config = Config::new(Some(config_path), resource_dir, dict_override)
        .expect("failed to load config");
    JapaneseDictionary::from_cfg(&config).expect("failed to load dictionary")
}

fn collect_headwords(dict: &JapaneseDictionary) -> Vec<String> {
    let mut seen = HashSet::<String>::new();
    let mut words = Vec::new();
    for wid in dict.lexicon().word_ids() {
        let wid = wid.expect("invalid word id");
        let info = dict
            .lexicon()
            .get_word_info_subset(wid, InfoSubset::HEADWORD)
            .expect("word info");
        let hw = info.headword(dict.lexicon()).to_owned();
        if !hw.is_empty() && seen.insert(hw.clone()) {
            words.push(hw);
        }
    }
    words
}

fn main() {
    let dict = load_dict();
    let inputs = env_path("SUDACHI_BENCH_INPUTS", "target/issue-117-corpora/kyoto-leads.txt");
    let trials = env_usize("SUDACHI_BENCH_TRIALS", 10);
    let text = std::fs::read_to_string(&inputs).expect("read inputs");
    let lines: Vec<&str> = text.lines().collect();
    let total_chars: usize = lines.iter().map(|l| l.chars().count()).sum();

    // Keyset: unique headwords, sorted; value = sorted index (same for both).
    let mut words = collect_headwords(&dict);
    words.sort();
    words.dedup();
    let entries: Vec<(&[u8], u32)> = words.iter().enumerate().map(|(i, w)| (w.as_bytes(), i as u32)).collect();

    let t = Instant::now();
    let yada_bytes = DoubleArrayBuilder::build(&entries).expect("yada build");
    let yada_build = t.elapsed().as_secs_f64();
    let da = DoubleArray::new(yada_bytes.as_slice());

    let t = Instant::now();
    let char_daac: CharwiseDoubleArrayAhoCorasick<u32> =
        CharwiseDoubleArrayAhoCorasick::new(&words).expect("daac build");
    let daac_build = t.elapsed().as_secs_f64();

    println!(
        "# keys {} | yada {:.1} MiB (build {:.1}s) | char-daac {:.1} MiB (build {:.1}s)",
        words.len(),
        yada_bytes.len() as f64 / (1 << 20) as f64,
        yada_build,
        char_daac.heap_bytes() as f64 / (1 << 20) as f64,
        daac_build,
    );

    // Correctness: identical (start, end, value) match sets over the corpus.
    let mut yset: HashSet<(u32, u32, u32)> = HashSet::new();
    for line in &lines {
        let b = line.as_bytes();
        for (i, _) in line.char_indices() {
            for (value, len) in da.common_prefix_search(&b[i..]) {
                yset.insert((i as u32, (i + len) as u32, value));
            }
        }
    }
    let mut dset: HashSet<(u32, u32, u32)> = HashSet::new();
    for line in &lines {
        for m in char_daac.find_overlapping_iter(*line) {
            dset.insert((m.start() as u32, m.end() as u32, m.value()));
        }
    }
    let identical = yset == dset;
    println!(
        "# match sets identical: {identical}  (yada {} | char-daac {} matches)",
        yset.len(),
        dset.len()
    );
    assert!(identical, "yada and char-daac disagree on the match set!");

    // Speed.
    let run = |f: &dyn Fn() -> usize| -> (f64, usize) {
        let mut times = Vec::with_capacity(trials);
        let mut n = 0;
        for _ in 0..trials {
            let s = Instant::now();
            n = f();
            times.push(s.elapsed().as_secs_f64() * 1e3);
        }
        times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        (times[times.len() / 2], n)
    };

    let (yada_ms, yn) = run(&|| {
        let mut n = 0usize;
        for line in &lines {
            let b = line.as_bytes();
            for (i, _) in line.char_indices() {
                n += da.common_prefix_search(&b[i..]).count();
            }
        }
        n
    });
    let (daac_ms, dn) = run(&|| {
        let mut n = 0usize;
        for line in &lines {
            n += char_daac.find_overlapping_iter(*line).count();
        }
        n
    });

    println!("# corpus {} sents, {} chars, {} matches, {} trials", lines.len(), total_chars, yn.max(dn), trials);
    println!(
        "yada  (byte-wise)  median {yada_ms:>7.2} ms  {:>6.2} ns/char",
        yada_ms * 1e6 / total_chars.max(1) as f64
    );
    println!(
        "char-daac          median {daac_ms:>7.2} ms  {:>6.2} ns/char",
        daac_ms * 1e6 / total_chars.max(1) as f64
    );
    println!("# char-wise speedup (yada/char-daac)  {:.4}x", yada_ms / daac_ms);
}
