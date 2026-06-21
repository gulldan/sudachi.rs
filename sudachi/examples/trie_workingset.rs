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

//! Issue #117 DIAGNOSIS: where does the double-array trie walk live in the cache?
//! Replays the real corpus' per-boundary common-prefix walk over the yada
//! double-array (the same structure Sudachi uses) and counts, per array load,
//! which 64-byte cache line is touched and how often. Reports:
//!   - total loads, loads/char (the dependent-load chain length);
//!   - distinct cache lines and the working-set size (fits L1 / L2 / L3?);
//!   - the Zipf head: how many (hottest) lines cover 50/90/99% of all loads.
//! If a tiny hot set covers most loads it is already L1-resident and no layout
//! helps; if loads are spread over an L2/L3-sized set, a layout could help.
//! Pure counting, no profiler attribution (the method eiennohito accepts).
//!
//! SUDACHI_BENCH_CONFIG  SUDACHI_BENCH_DICT  SUDACHI_BENCH_INPUTS

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;

use sudachi::config::Config;
use sudachi::dic::dictionary::JapaneseDictionary;
use sudachi::dic::subset::InfoSubset;
use yada::builder::DoubleArrayBuilder;

const LINE: usize = 64; // bytes per cache line
const L1: usize = 128 * 1024; // M4 L1d
const L2: usize = 16 * 1024 * 1024; // M4 L2

fn env_path(key: &str, default: &str) -> PathBuf {
    std::env::var_os(key).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(default))
}

#[inline]
fn unit_at(da: &[u32], idx: usize) -> Option<u32> {
    da.get(idx).copied()
}
#[inline]
fn label(unit: u32) -> usize {
    unit as usize & ((1 << 31) | 0xFF)
}
#[inline]
fn offset(unit: u32) -> usize {
    ((unit as usize) >> 10) << (((unit as usize) & (1 << 9)) >> 6)
}
#[inline]
fn has_leaf(unit: u32) -> bool {
    ((unit >> 8) & 1) == 1
}

fn main() {
    let config_path = env_path("SUDACHI_BENCH_CONFIG", "resources/sudachi.json");
    let resource_dir = config_path.parent().map(|p| p.to_path_buf()).filter(|p| !p.as_os_str().is_empty());
    let dict_override = std::env::var_os("SUDACHI_BENCH_DICT").map(PathBuf::from);
    let config = Config::new(Some(config_path), resource_dir, dict_override).expect("config");
    let dict = JapaneseDictionary::from_cfg(&config).expect("dict");

    // Same keyset/structure Sudachi uses: a yada double-array over headwords.
    let mut words = HashSet::<String>::new();
    let mut list = Vec::new();
    for wid in dict.lexicon().word_ids() {
        let wid = wid.expect("wid");
        let info = dict.lexicon().get_word_info_subset(wid, InfoSubset::HEADWORD).expect("wi");
        let hw = info.headword(dict.lexicon()).to_owned();
        if !hw.is_empty() && words.insert(hw.clone()) {
            list.push(hw);
        }
    }
    list.sort();
    let entries: Vec<(&[u8], u32)> = list.iter().enumerate().map(|(i, w)| (w.as_bytes(), i as u32)).collect();
    let da_bytes = DoubleArrayBuilder::build(&entries).expect("build");
    let da: Vec<u32> = da_bytes.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
    let trie_bytes = da.len() * 4;

    let inputs = env_path("SUDACHI_BENCH_INPUTS", "target/issue-117-corpora/kyoto-leads.txt");
    let text = std::fs::read_to_string(&inputs).expect("inputs");
    let lines: Vec<&str> = text.lines().collect();
    let total_chars: usize = lines.iter().map(|l| l.chars().count()).sum();

    // Per-boundary common-prefix walk; record every array index loaded.
    let root = offset(da[0]);
    let mut line_hits: HashMap<usize, u64> = HashMap::new();
    let mut loads: u64 = 0;
    for line in &lines {
        let b = line.as_bytes();
        for (start, _) in line.char_indices() {
            let mut node_pos = root;
            for &byte in &b[start..] {
                node_pos ^= byte as usize;
                let Some(unit) = unit_at(&da, node_pos) else { break };
                loads += 1;
                *line_hits.entry((node_pos * 4) / LINE).or_default() += 1;
                if label(unit) != byte as usize {
                    break;
                }
                let _ = has_leaf(unit);
                node_pos ^= offset(unit);
            }
        }
    }

    let distinct = line_hits.len();
    let ws = distinct * LINE;
    let fits = if ws <= L1 { "fits L1" } else if ws <= L2 { "fits L2" } else { "spills to L3/DRAM" };
    println!("# trie {:.1} MiB ({} u32 nodes) | corpus {} sents, {} chars", trie_bytes as f64 / (1<<20) as f64, da.len(), lines.len(), total_chars);
    println!("# loads {loads} ({:.2}/char) | distinct 64B lines {distinct} | working set {:.2} MiB -> {fits}", loads as f64 / total_chars.max(1) as f64, ws as f64 / (1<<20) as f64);

    // Zipf head: hottest lines covering X% of all loads.
    let mut counts: Vec<u64> = line_hits.values().copied().collect();
    counts.sort_unstable_by(|a, b| b.cmp(a));
    let mut acc = 0u64;
    let (mut n50, mut n90, mut n99) = (0usize, 0usize, 0usize);
    for (i, &c) in counts.iter().enumerate() {
        acc += c;
        let frac = acc as f64 / loads as f64;
        if n50 == 0 && frac >= 0.50 { n50 = i + 1; }
        if n90 == 0 && frac >= 0.90 { n90 = i + 1; }
        if n99 == 0 && frac >= 0.99 { n99 = i + 1; }
    }
    let kib = |n: usize| (n * LINE) as f64 / 1024.0;
    println!("# Zipf head of the loads:");
    println!("#   50% of loads <- {n50} lines ({:.1} KiB) {}", kib(n50), if n50 * LINE <= L1 { "[L1-resident]" } else { "" });
    println!("#   90% of loads <- {n90} lines ({:.1} KiB) {}", kib(n90), if n90 * LINE <= L1 { "[L1-resident]" } else { "" });
    println!("#   99% of loads <- {n99} lines ({:.1} KiB) {}", kib(n99), if n99 * LINE <= L1 { "[L1-resident]" } else if n99 * LINE <= L2 { "[L2-resident]" } else { "" });
}
