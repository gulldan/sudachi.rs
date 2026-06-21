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

//! Issue #117 follow-up: WHY does the corpus-frequency trie layout give its
//! (small) win? Walks the corpus over the baseline yada double-array and the
//! frequency-weighted one, both over the SAME headwords, and compares the
//! cache-line "Zipf head": how many distinct 64-byte lines carry 50/90/99% of
//! all loads. If the freq layout packs the hot nodes into FEWER lines, that is
//! the mechanism of its win (more of the hot path stays L1-resident). Pure
//! counting; the two walks visit the same logical nodes (assert equal loads).
//!
//! SUDACHI_BENCH_CONFIG  SUDACHI_BENCH_DICT  SUDACHI_BENCH_INPUTS

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;

use sudachi::config::Config;
use sudachi::dic::build::weighted_trie::build_weighted;
use sudachi::dic::dictionary::JapaneseDictionary;
use sudachi::dic::subset::InfoSubset;
use yada::builder::DoubleArrayBuilder;

const LINE: usize = 64;
const L1: usize = 128 * 1024;

fn env_path(key: &str, default: &str) -> PathBuf {
    std::env::var_os(key).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(default))
}
#[inline]
fn label(unit: u32) -> usize {
    unit as usize & ((1 << 31) | 0xFF)
}
#[inline]
fn offset(unit: u32) -> usize {
    ((unit as usize) >> 10) << (((unit as usize) & (1 << 9)) >> 6)
}

fn to_u32(bytes: &[u8]) -> Vec<u32> {
    bytes.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect()
}

/// Walk the corpus over `da`, returning per-64B-line load counts and total loads.
fn walkset(da: &[u32], lines: &[&str]) -> (HashMap<usize, u64>, u64) {
    let root = offset(da[0]);
    let mut hits: HashMap<usize, u64> = HashMap::new();
    let mut loads = 0u64;
    for line in lines {
        let b = line.as_bytes();
        for (start, _) in line.char_indices() {
            let mut node_pos = root;
            for &byte in &b[start..] {
                node_pos ^= byte as usize;
                let Some(&unit) = da.get(node_pos) else { break };
                loads += 1;
                *hits.entry((node_pos * 4) / LINE).or_default() += 1;
                if label(unit) != byte as usize {
                    break;
                }
                node_pos ^= offset(unit);
            }
        }
    }
    (hits, loads)
}

/// Hottest-line counts covering 50/90/99% of loads.
fn zipf_head(hits: &HashMap<usize, u64>, loads: u64) -> (usize, usize, usize) {
    let mut counts: Vec<u64> = hits.values().copied().collect();
    counts.sort_unstable_by(|a, b| b.cmp(a));
    let (mut n50, mut n90, mut n99) = (0usize, 0usize, 0usize);
    let mut acc = 0u64;
    for (i, &c) in counts.iter().enumerate() {
        acc += c;
        let f = acc as f64 / loads as f64;
        if n50 == 0 && f >= 0.50 { n50 = i + 1; }
        if n90 == 0 && f >= 0.90 { n90 = i + 1; }
        if n99 == 0 && f >= 0.99 { n99 = i + 1; }
    }
    (n50, n90, n99)
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
    let eb: Vec<(&[u8], u32)> = words.iter().enumerate().map(|(i, w)| (w.as_bytes(), i as u32)).collect();
    let baseline = to_u32(&DoubleArrayBuilder::build(&eb).expect("baseline"));
    let ew: Vec<(&str, u32)> = words.iter().enumerate().map(|(i, w)| (w.as_str(), i as u32)).collect();
    let weighted = to_u32(&build_weighted(&ew, &text).expect("weighted"));

    let (hb, lb) = walkset(&baseline, &lines);
    let (hw, lw) = walkset(&weighted, &lines);
    assert_eq!(lb, lw, "walks visited a different number of nodes");

    let kib = |n: usize| (n * LINE) as f64 / 1024.0;
    let l1 = |n: usize| if n * LINE <= L1 { " [L1]" } else { "" };
    let (b50, b90, b99) = zipf_head(&hb, lb);
    let (w50, w90, w99) = zipf_head(&hw, lw);
    println!("# keys {} | loads {} | baseline lines {} | freq lines {}", words.len(), lb, hb.len(), hw.len());
    println!("# Zipf head (distinct 64B lines carrying X% of loads):");
    println!("#          baseline                 freq-weighted");
    println!("#  50%   {b50:>6} lines ({:>7.1} KiB){}   {w50:>6} lines ({:>7.1} KiB){}", kib(b50), l1(b50), kib(w50), l1(w50));
    println!("#  90%   {b90:>6} lines ({:>7.1} KiB){}   {w90:>6} lines ({:>7.1} KiB){}", kib(b90), l1(b90), kib(w90), l1(w90));
    println!("#  99%   {b99:>6} lines ({:>7.1} KiB){}   {w99:>6} lines ({:>7.1} KiB){}", kib(b99), l1(b99), kib(w99), l1(w99));
    let tighten = |b: usize, w: usize| 100.0 * (b as f64 - w as f64) / b as f64;
    println!("# freq packs the hot set tighter by: 50%: {:+.1}%  90%: {:+.1}%  99%: {:+.1}%", tighten(b50, w50), tighten(b90, w90), tighten(b99, w99));
}
