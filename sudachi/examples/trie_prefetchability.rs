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

//! Issue #117: how PREFETCHABLE is each trie layout? eiennohito's goal is a layout
//! "more exploitable by hw prefetches". A HW prefetcher follows the consecutive
//! load-address stream; it helps when the next line is close to / a constant
//! stride from the current one. So for the within-walk consecutive loads we
//! measure the cache-line delta distribution:
//!   - frac |delta| <= 1  (next-line prefetcher can reach it),
//!   - frac |delta| <= 4  (a small prefetch window),
//!   - entropy of the delta distribution (lower = more regular = more stride-able).
//! Built over the same headwords for baseline / freq / freq+phase layouts; the
//! one with the highest small-delta fraction is the most prefetch-friendly — a
//! concrete, M4-independent objective to take to eiennohito (M4 speed is ~0
//! regardless because the working set is L2-resident).
//!
//! SUDACHI_BENCH_CONFIG  SUDACHI_BENCH_DICT  SUDACHI_BENCH_INPUTS

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;

use sudachi::config::Config;
use sudachi::dic::build::weighted_trie::{build_weighted_cfg, build_weighted_obj};
use sudachi::dic::dictionary::JapaneseDictionary;
use sudachi::dic::subset::InfoSubset;
use yada::builder::DoubleArrayBuilder;

const LINE: usize = 64;

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
fn to_u32(b: &[u8]) -> Vec<u32> {
    b.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect()
}

/// Walk the corpus; for consecutive loads within each walk, tally |line delta|.
/// Returns (delta-bucket counts keyed by signed line delta, total transitions,
/// distinct lines).
fn measure(da: &[u32], lines: &[&str]) -> (HashMap<i64, u64>, u64, usize) {
    let root = offset(da[0]);
    let mut deltas: HashMap<i64, u64> = HashMap::new();
    let mut transitions = 0u64;
    let mut distinct: HashSet<usize> = HashSet::new();
    for line in lines {
        let b = line.as_bytes();
        for (start, _) in line.char_indices() {
            let mut node_pos = root;
            let mut prev_line: Option<i64> = None;
            for &byte in &b[start..] {
                node_pos ^= byte as usize;
                let Some(&unit) = da.get(node_pos) else { break };
                let cur_line = ((node_pos * 4) / LINE) as i64;
                distinct.insert((node_pos * 4) / LINE);
                if let Some(pl) = prev_line {
                    *deltas.entry(cur_line - pl).or_default() += 1;
                    transitions += 1;
                }
                prev_line = Some(cur_line);
                if label(unit) != byte as usize {
                    break;
                }
                node_pos ^= offset(unit);
            }
        }
    }
    (deltas, transitions, distinct.len())
}

fn report(name: &str, da: &[u32], lines: &[&str]) {
    let (deltas, total, distinct) = measure(da, lines);
    let frac = |pred: &dyn Fn(i64) -> bool| -> f64 {
        let s: u64 = deltas.iter().filter(|(d, _)| pred(**d)).map(|(_, c)| *c).sum();
        100.0 * s as f64 / total.max(1) as f64
    };
    // Shannon entropy of the line-delta distribution (bits).
    let mut entropy = 0.0f64;
    for &c in deltas.values() {
        let p = c as f64 / total as f64;
        if p > 0.0 {
            entropy -= p * p.log2();
        }
    }
    println!(
        "{name:<14} distinct {distinct:>7} lines | |d|=0 {:>5.1}%  |d|<=1 {:>5.1}%  |d|<=4 {:>5.1}%  | entropy {:>5.2} bits | distinct deltas {}",
        frac(&|d| d == 0),
        frac(&|d| d.abs() <= 1),
        frac(&|d| d.abs() <= 4),
        entropy,
        deltas.len()
    );
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
    let ew: Vec<(&str, u32)> = words.iter().enumerate().map(|(i, w)| (w.as_str(), i as u32)).collect();
    let baseline = to_u32(&DoubleArrayBuilder::build(&eb).expect("baseline"));
    let freq = to_u32(&build_weighted_cfg(&ew, &text, false).expect("freq"));
    let phase = to_u32(&build_weighted_obj(&ew, &text, true, 0).expect("phase"));
    let parent = to_u32(&build_weighted_obj(&ew, &text, true, 1).expect("parent"));
    let stride = to_u32(&build_weighted_obj(&ew, &text, true, 2).expect("stride"));

    println!("# prefetchability of the trie walk's within-walk consecutive loads (higher small-|delta| = more HW-prefetchable)");
    report("baseline", &baseline, &lines);
    report("freq", &freq, &lines);
    report("phase(H2)", &phase, &lines);
    report("parent-child", &parent, &lines);
    report("parent+stride", &stride, &lines);
}
