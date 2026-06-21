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

//! Issue #117 CAPSTONE proof: does a corpus-frequency trie layout help MORE as
//! cache pressure rises? Builds the baseline yada double-array and the
//! frequency-weighted one over the SAME headwords, then replays the corpus' trie
//! walk with a controllable amount of cache eviction between sentences (read S
//! bytes of scratch memory). At each S, reports the freq-vs-baseline walk speedup.
//!
//! If the speedup grows with S, eiennohito's prefetchable-layout idea is proven
//! to pay in proportion to cache pressure → it would help on smaller-cache /
//! DRAM-bound hardware ("even Java free wins"), and is ~0 on M4 only because the
//! working set is L2-resident here. Pure causal sweep, no profiler attribution.
//!
//! Walks produce IDENTICAL match counts (asserted) so the timings are comparable.
//! SUDACHI_BENCH_CONFIG  SUDACHI_BENCH_DICT  SUDACHI_BENCH_INPUTS  SUDACHI_BENCH_TRIALS

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;

use sudachi::config::Config;
use sudachi::dic::build::weighted_trie::build_weighted;
use sudachi::dic::dictionary::JapaneseDictionary;
use sudachi::dic::subset::InfoSubset;
use yada::builder::DoubleArrayBuilder;
use yada::DoubleArray;

fn env_path(key: &str, default: &str) -> PathBuf {
    std::env::var_os(key).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(default))
}

fn walk_count(da: &DoubleArray<&[u8]>, lines: &[&str], scratch: &[u64], evict_words: usize) -> (f64, usize) {
    let start = Instant::now();
    let mut n = 0usize;
    let mut sink = 0u64;
    for line in lines {
        let b = line.as_bytes();
        for (i, _) in line.char_indices() {
            n += da.common_prefix_search(&b[i..]).count();
        }
        // Evict ~evict_words*8 bytes of cache between sentences.
        let mut k = 0;
        while k < evict_words {
            sink = sink.wrapping_add(scratch[k]);
            k += 8; // one touch per 64-byte line
        }
    }
    std::hint::black_box(sink);
    (start.elapsed().as_secs_f64() * 1e3, n)
}

fn main() {
    let config_path = env_path("SUDACHI_BENCH_CONFIG", "resources/sudachi.json");
    let resource_dir = config_path.parent().map(|p| p.to_path_buf()).filter(|p| !p.as_os_str().is_empty());
    let dict_override = std::env::var_os("SUDACHI_BENCH_DICT").map(PathBuf::from);
    let config = Config::new(Some(config_path), resource_dir, dict_override).expect("config");
    let dict = JapaneseDictionary::from_cfg(&config).expect("dict");

    let inputs = env_path("SUDACHI_BENCH_INPUTS", "target/issue-117-corpora/kyoto-leads.txt");
    let text = std::fs::read_to_string(&inputs).expect("inputs");
    let lines: Vec<&str> = text.lines().collect();
    let trials: usize = std::env::var("SUDACHI_BENCH_TRIALS").ok().and_then(|v| v.parse().ok()).unwrap_or(7);

    // Same headwords for both layouts; value = sorted index.
    let mut seen = HashSet::<String>::new();
    let mut words = Vec::new();
    for wid in dict.lexicon().word_ids() {
        let wid = wid.expect("wid");
        let info = dict.lexicon().get_word_info_subset(wid, InfoSubset::HEADWORD).expect("wi");
        let hw = info.headword(dict.lexicon()).to_owned();
        if !hw.is_empty() && seen.insert(hw.clone()) {
            words.push(hw);
        }
    }
    words.sort();
    let entries_b: Vec<(&[u8], u32)> = words.iter().enumerate().map(|(i, w)| (w.as_bytes(), i as u32)).collect();
    let baseline = DoubleArrayBuilder::build(&entries_b).expect("baseline build");
    let entries_w: Vec<(&str, u32)> = words.iter().enumerate().map(|(i, w)| (w.as_str(), i as u32)).collect();
    let weighted = build_weighted(&entries_w, &text).expect("weighted build");

    let da_b = DoubleArray::new(baseline.as_slice());
    let da_w = DoubleArray::new(weighted.as_slice());

    // 96 MiB scratch (> L2) so we can evict any chosen footprint.
    let scratch: Vec<u64> = (0..(96 * 1024 * 1024 / 8)).map(|i| i as u64).collect();

    println!("# keys {} | baseline {:.1} MiB | weighted {:.1} MiB | corpus {} sents", words.len(), baseline.len() as f64 / (1<<20) as f64, weighted.len() as f64 / (1<<20) as f64, lines.len());
    println!("# evict/sent   baseline ms   freq ms   freq speedup");
    for &kib in &[0usize, 256, 1024, 4096, 16384, 65536] {
        let ew = (kib * 1024) / 8;
        let med = |da: &DoubleArray<&[u8]>| -> (f64, usize) {
            let mut ts = Vec::with_capacity(trials);
            let mut n = 0;
            for _ in 0..trials {
                let (ms, c) = walk_count(da, &lines, &scratch, ew);
                ts.push(ms);
                n = c;
            }
            ts.sort_by(|a, b| a.partial_cmp(b).unwrap());
            (ts[ts.len() / 2], n)
        };
        let (bm, bn) = med(&da_b);
        let (wm, wn) = med(&da_w);
        assert_eq!(bn, wn, "match counts differ");
        let label = if kib >= 1024 { format!("{} MiB", kib / 1024) } else { format!("{kib} KiB") };
        println!("  {label:>9}   {bm:>10.2}   {wm:>7.2}   {:.4}x", bm / wm);
    }
}
