/*
 * PROBE (not for upstream). Attempts a corpus-aware double-array trie layout
 * in the exact yada-compatible serialized format. This is a feasibility test
 * for issue #117 / eiennohito's suggestion, not production code.
 */

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

use sudachi::dic::lexicon::trie::Trie;
use yada::unit::{Unit, UnitID};
use yada::DoubleArray;

const BLOCK_SIZE: usize = 256;
const NUM_TARGET_BLOCKS: i32 = 16;
const INVALID_NEXT: u8 = 0;
const INVALID_PREV: u8 = 255;

const DEFAULT_UNITS: [Unit; BLOCK_SIZE] = [Unit::new(); BLOCK_SIZE];
const DEFAULT_IS_USED: [bool; BLOCK_SIZE] = [false; BLOCK_SIZE];
const DEFAULT_NEXT_UNUSED: [u8; BLOCK_SIZE] = {
    let mut next_unused = [INVALID_NEXT; BLOCK_SIZE];
    let mut i = 0;
    while i < next_unused.len() - 1 {
        next_unused[i] = (i + 1) as u8;
        i += 1;
    }
    next_unused
};
const DEFAULT_PREV_UNUSED: [u8; BLOCK_SIZE] = {
    let mut prev_unused = [INVALID_PREV; BLOCK_SIZE];
    let mut i = 1;
    while i < prev_unused.len() {
        prev_unused[i] = (i - 1) as u8;
        i += 1;
    }
    prev_unused
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct Hit {
    line: u32,
    start: u32,
    end: u32,
    value: u32,
}

#[derive(Debug)]
struct WeightedDoubleArrayBuilder<'a> {
    blocks: Vec<DoubleArrayBlock>,
    used_offsets: HashSet<u32>,
    prefix_hits: &'a HashMap<Vec<u8>, u32>,
}

impl<'a> WeightedDoubleArrayBuilder<'a> {
    fn new(prefix_hits: &'a HashMap<Vec<u8>, u32>) -> Self {
        Self {
            blocks: vec![DoubleArrayBlock::new(0)],
            used_offsets: HashSet::new(),
            prefix_hits,
        }
    }

    fn build(mut self, keyset: &[(Vec<u8>, u32)]) -> Option<Vec<u8>> {
        self.reserve(0);
        self.build_recursive(keyset, Vec::new(), 0, keyset.len(), 0)?;

        let mut out = Vec::with_capacity(self.blocks.len() * BLOCK_SIZE * 4);
        for block in &self.blocks {
            for unit in block.units.iter() {
                out.extend_from_slice(&unit.as_u32().to_le_bytes());
            }
        }
        Some(out)
    }

    fn get_block(&self, unit_id: UnitID) -> Option<&DoubleArrayBlock> {
        self.blocks.get(unit_id / BLOCK_SIZE)
    }

    fn get_block_mut(&mut self, unit_id: UnitID) -> Option<&mut DoubleArrayBlock> {
        self.blocks.get_mut(unit_id / BLOCK_SIZE)
    }

    fn extend_block(&mut self) {
        let block_id = self.blocks.len();
        self.blocks.push(DoubleArrayBlock::new(block_id));
    }

    fn get_unit_mut(&mut self, unit_id: UnitID) -> &mut Unit {
        while self.get_block(unit_id).is_none() {
            self.extend_block();
        }
        &mut self.get_block_mut(unit_id).unwrap().units[unit_id % BLOCK_SIZE]
    }

    fn reserve(&mut self, unit_id: UnitID) {
        while self.get_block(unit_id).is_none() {
            self.extend_block();
        }
        self.get_block_mut(unit_id)
            .unwrap()
            .reserve((unit_id % BLOCK_SIZE) as u8);
    }

    fn build_recursive(
        &mut self,
        keyset: &[(Vec<u8>, u32)],
        prefix: Vec<u8>,
        begin: usize,
        end: usize,
        unit_id: UnitID,
    ) -> Option<()> {
        let depth = prefix.len();
        let mut labels: Vec<(u8, usize, usize)> = Vec::with_capacity(256);
        let mut value = None;

        for i in begin..end {
            let (key, val) = keyset.get(i).unwrap();
            let label = if depth == key.len() {
                0
            } else {
                *key.get(depth)?
            };
            if label == 0 {
                value = Some(*val);
            }
            match labels.last_mut() {
                Some(last) if last.0 == label => {}
                Some(last) => {
                    last.2 = i;
                    labels.push((label, i, 0));
                }
                None => labels.push((label, i, 0)),
            }
        }
        labels.last_mut().unwrap().2 = end;

        let label_bytes = labels
            .iter()
            .map(|(label, _, _)| *label)
            .collect::<Vec<_>>();
        let offset = loop {
            if let Some(offset) = self.find_offset(unit_id, &label_bytes) {
                break offset;
            }
            self.extend_block();
        };

        self.used_offsets.insert(offset);
        let has_leaf = label_bytes.first().copied() == Some(0);

        let parent = self.get_unit_mut(unit_id);
        parent.set_offset(offset ^ unit_id as u32);
        parent.set_has_leaf(has_leaf);

        for label in &label_bytes {
            let child_id = (offset ^ *label as u32) as UnitID;
            self.reserve(child_id);
            let unit = self.get_unit_mut(child_id);
            if *label == 0 {
                unit.set_value(value?);
            } else {
                unit.set_label(*label);
            }
        }

        let mut weighted_labels = labels
            .into_iter()
            .map(|(label, begin, end)| {
                let freq = self.child_freq(&prefix, label);
                (label, begin, end, freq)
            })
            .collect::<Vec<_>>();
        weighted_labels.sort_by(|a, b| b.3.cmp(&a.3).then_with(|| a.0.cmp(&b.0)));

        for (label, begin, end, _) in weighted_labels {
            if label == 0 {
                continue;
            }
            let mut child_prefix = prefix.clone();
            child_prefix.push(label);
            let _ = self.build_recursive(
                keyset,
                child_prefix,
                begin,
                end,
                (label as u32 ^ offset) as UnitID,
            );
        }

        Some(())
    }

    fn child_freq(&self, prefix: &[u8], label: u8) -> u32 {
        if label == 0 {
            return u32::MAX;
        }
        let mut child = Vec::with_capacity(prefix.len() + 1);
        child.extend_from_slice(prefix);
        child.push(label);
        self.prefix_hits.get(&child).copied().unwrap_or(0)
    }

    fn find_offset(&self, unit_id: UnitID, labels: &[u8]) -> Option<u32> {
        let head_block = (self.blocks.len() as i32 - NUM_TARGET_BLOCKS).max(0) as usize;
        self.blocks.iter().skip(head_block).find_map(|block| {
            for offset in block.find_offset(unit_id, labels) {
                let offset_u32 = (block.id as u32) << 8 | offset as u32;
                if !self.used_offsets.contains(&offset_u32) {
                    return Some(offset_u32);
                }
            }
            None
        })
    }
}

#[derive(Debug)]
struct DoubleArrayBlock {
    id: usize,
    units: [Unit; BLOCK_SIZE],
    is_used: [bool; BLOCK_SIZE],
    head_unused: u8,
    next_unused: [u8; BLOCK_SIZE],
    prev_unused: [u8; BLOCK_SIZE],
}

impl DoubleArrayBlock {
    const fn new(id: usize) -> Self {
        Self {
            id,
            units: DEFAULT_UNITS,
            is_used: DEFAULT_IS_USED,
            head_unused: 0,
            next_unused: DEFAULT_NEXT_UNUSED,
            prev_unused: DEFAULT_PREV_UNUSED,
        }
    }

    fn reserve(&mut self, id: u8) {
        self.is_used[id as usize] = true;
        let prev_id = self.prev_unused[id as usize];
        let next_id = self.next_unused[id as usize];
        if prev_id != INVALID_PREV {
            self.next_unused[prev_id as usize] = next_id;
        }
        self.next_unused[id as usize] = INVALID_NEXT;
        if next_id != INVALID_NEXT {
            self.prev_unused[next_id as usize] = prev_id;
        }
        self.prev_unused[id as usize] = INVALID_PREV;
        if id == self.head_unused {
            self.head_unused = next_id;
        }
    }

    fn find_offset<'a>(&'a self, unit_id: UnitID, labels: &'a [u8]) -> FindOffset<'a> {
        FindOffset {
            unused_id: self.head_unused,
            block: self,
            unit_id,
            labels,
        }
    }
}

struct FindOffset<'a> {
    unused_id: u8,
    block: &'a DoubleArrayBlock,
    unit_id: UnitID,
    labels: &'a [u8],
}

impl FindOffset<'_> {
    fn is_valid_offset(&self, offset: u8) -> bool {
        let offset_u32 = (self.block.id as u32) << 8 | offset as u32;
        let relative_offset = self.unit_id as u32 ^ offset_u32;
        if (relative_offset & (0xFF << 21)) > 0 && (relative_offset & 0xFF) > 0 {
            return false;
        }

        self.labels.iter().skip(1).all(|label| {
            let id = offset ^ label;
            !self.block.is_used[id as usize]
        })
    }
}

impl Iterator for FindOffset<'_> {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        if self.unused_id == INVALID_NEXT && self.block.is_used[self.unused_id as usize] {
            return None;
        }
        if self.block.head_unused == INVALID_NEXT && self.block.is_used[0] {
            return None;
        }
        loop {
            let first_label = *self.labels.first()?;
            let offset = self.unused_id ^ first_label;
            let valid = self.is_valid_offset(offset);
            self.unused_id = self.block.next_unused[self.unused_id as usize];
            if valid {
                return Some(offset);
            }
            if self.unused_id == INVALID_NEXT {
                return None;
            }
        }
    }
}

fn env_path(key: &str, default: &str) -> PathBuf {
    std::env::var_os(key)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default))
}

fn env_paths(key: &str, defaults: &[&str]) -> Vec<PathBuf> {
    std::env::var_os(key)
        .map(|value| {
            value
                .to_string_lossy()
                .split(',')
                .filter(|part| !part.is_empty())
                .map(PathBuf::from)
                .collect()
        })
        .unwrap_or_else(|| defaults.iter().map(PathBuf::from).collect())
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

fn record_prefix_hits(da: &[u8], lines: &[&str]) -> HashMap<Vec<u8>, u32> {
    let mut hits = HashMap::<Vec<u8>, u32>::new();
    let root = offset(unit_at(da, 0).expect("empty trie"));
    for line in lines {
        let input = line.as_bytes();
        for (start, _) in line.char_indices() {
            let mut node_pos = root;
            let mut prefix = Vec::new();
            for &byte in &input[start..] {
                node_pos ^= byte as usize;
                let Some(unit) = unit_at(da, node_pos) else {
                    break;
                };
                if label(unit) != byte as usize {
                    break;
                }
                prefix.push(byte);
                *hits.entry(prefix.clone()).or_default() += 1;
                node_pos ^= offset(unit);
            }
        }
    }
    hits
}

fn collect_lines(path: PathBuf) -> Vec<String> {
    std::fs::read_to_string(path)
        .expect("failed to read corpus")
        .lines()
        .map(str::to_owned)
        .collect()
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

fn collect_sudachi_scalar(da_bytes: &[u8], lines: &[&str]) -> Vec<Hit> {
    let trie = Trie::from_bytes(da_bytes);
    let mut hits = Vec::new();
    for (line_idx, line) in lines.iter().enumerate() {
        let input = line.as_bytes();
        for (start, _) in line.char_indices() {
            for entry in trie.common_prefix_iterator(input, start) {
                hits.push(Hit {
                    line: line_idx as u32,
                    start: start as u32,
                    end: entry.end as u32,
                    value: entry.value,
                });
            }
        }
    }
    hits.sort_unstable();
    hits
}

fn collect_sudachi_batch(da_bytes: &[u8], lines: &[&str]) -> Vec<Hit> {
    let trie = Trie::from_bytes(da_bytes);
    let mut hits = Vec::new();
    for (line_idx, line) in lines.iter().enumerate() {
        let input = line.as_bytes();
        let starts = line
            .char_indices()
            .map(|(start, _)| start)
            .collect::<Vec<_>>();
        trie.common_prefix_batch(input, &starts, |bucket, value, end| {
            hits.push(Hit {
                line: line_idx as u32,
                start: starts[bucket] as u32,
                end: end as u32,
                value,
            });
        });
    }
    hits.sort_unstable();
    hits
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
    let corpus = collect_lines(corpus_path);
    let lines = corpus.iter().map(String::as_str).collect::<Vec<_>>();

    let t0 = Instant::now();
    let keys = load_index_forms(&lexicons);
    println!(
        "# keys: {} loaded in {:.1} ms",
        keys.len(),
        t0.elapsed().as_secs_f64() * 1e3
    );

    let t1 = Instant::now();
    let baseline = yada::builder::DoubleArrayBuilder::build(&keys).expect("baseline build failed");
    println!(
        "# baseline bytes: {} build_ms: {:.1}",
        baseline.len(),
        t1.elapsed().as_secs_f64() * 1e3
    );

    let t2 = Instant::now();
    let prefix_hits = record_prefix_hits(&baseline, &lines);
    println!(
        "# prefix_hits: {} collected_ms: {:.1}",
        prefix_hits.len(),
        t2.elapsed().as_secs_f64() * 1e3
    );

    let t3 = Instant::now();
    let weighted = WeightedDoubleArrayBuilder::new(&prefix_hits)
        .build(&keys)
        .expect("weighted build failed");
    println!(
        "# weighted bytes: {} build_ms: {:.1}",
        weighted.len(),
        t3.elapsed().as_secs_f64() * 1e3
    );

    let t4 = Instant::now();
    validate_exact(&keys, &baseline);
    validate_exact(&keys, &weighted);
    validate_exact_sudachi("baseline", &keys, &baseline);
    validate_exact_sudachi("weighted", &keys, &weighted);
    println!(
        "# exact-match parity ok via yada and current Trie reader in {:.1} ms",
        t4.elapsed().as_secs_f64() * 1e3
    );

    let t5 = Instant::now();
    let base_scalar = collect_sudachi_scalar(&baseline, &lines);
    let weighted_scalar = collect_sudachi_scalar(&weighted, &lines);
    assert_eq!(
        base_scalar, weighted_scalar,
        "current Trie scalar hit stream changed"
    );
    let base_batch = collect_sudachi_batch(&baseline, &lines);
    let weighted_batch = collect_sudachi_batch(&weighted, &lines);
    assert_eq!(base_scalar, base_batch, "baseline scalar/batch mismatch");
    assert_eq!(
        weighted_scalar, weighted_batch,
        "weighted scalar/batch mismatch"
    );
    println!(
        "# current Trie reader parity ok: scalar+batch hits={} checked_ms={:.1}",
        base_scalar.len(),
        t5.elapsed().as_secs_f64() * 1e3
    );

    let (base_count, base_ms) = timed_count("baseline", &baseline, &lines);
    let (weighted_count, weighted_ms) = timed_count("weighted", &weighted, &lines);
    assert_eq!(base_count, weighted_count, "common-prefix count changed");

    for line_size in [64usize, 128] {
        let b = line_footprint(&baseline, &lines, line_size);
        let w = line_footprint(&weighted, &lines, line_size);
        println!(
            "# footprint {line_size}B: baseline {b} lines ({:.2} MiB), weighted {w} lines ({:.2} MiB), delta {:+.1}%",
            b as f64 * line_size as f64 / (1 << 20) as f64,
            w as f64 * line_size as f64 / (1 << 20) as f64,
            (w as f64 / b as f64 - 1.0) * 100.0
        );
    }

    println!(
        "# time delta weighted/baseline: {:+.1}%",
        (weighted_ms / base_ms - 1.0) * 100.0
    );
}
