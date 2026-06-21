/*
 * PROBE (not for upstream). Corpus-frequency-weighted double-array trie builder
 * in the exact yada-compatible serialized format. Used to test eiennohito's
 * issue #117 layout idea END-TO-END: build a full system dictionary whose trie
 * orders child construction by corpus prefix-hit frequency, keep the current
 * `Trie` reader, and measure do_tokenize. Enabled from `build_trie` when
 * `SUDACHI_LAYOUT_CORPUS` points at a corpus file. Lifted from
 * `examples/corpus_trie_layout_probe.rs` (which proves exact lookup parity).
 */
use std::collections::{HashMap, HashSet};

use yada::unit::{Unit, UnitID};

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

struct WeightedDoubleArrayBuilder<'a> {
    blocks: Vec<DoubleArrayBlock>,
    used_offsets: HashSet<u32>,
    prefix_hits: &'a HashMap<Vec<u8>, u32>,
    /// PROBE H2 (eiennohito offset-similarity): when set, bias offset selection
    /// toward a per-(byte-depth % 3) running target, so subarrays for each UTF-8
    /// triplet position get similar relative offsets (modulo values).
    regular: bool,
    targets: [u32; 3],
}

impl<'a> WeightedDoubleArrayBuilder<'a> {
    fn new(prefix_hits: &'a HashMap<Vec<u8>, u32>, regular: bool) -> Self {
        Self {
            blocks: vec![DoubleArrayBlock::new(0)],
            used_offsets: HashSet::new(),
            prefix_hits,
            regular,
            targets: [0; 3],
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
            let label = if depth == key.len() { 0 } else { *key.get(depth)? };
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

        let label_bytes = labels.iter().map(|(l, _, _)| *l).collect::<Vec<_>>();
        let offset = loop {
            if let Some(offset) = self.find_offset(unit_id, &label_bytes, depth) {
                break offset;
            }
            self.extend_block();
        };
        if self.regular {
            self.targets[depth % 3] = offset;
        }
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

        let mut weighted = labels
            .into_iter()
            .map(|(label, begin, end)| {
                let freq = self.child_freq(&prefix, label);
                (label, begin, end, freq)
            })
            .collect::<Vec<_>>();
        weighted.sort_by(|a, b| b.3.cmp(&a.3).then_with(|| a.0.cmp(&b.0)));

        for (label, begin, end, _) in weighted {
            if label == 0 {
                continue;
            }
            let mut child_prefix = prefix.clone();
            child_prefix.push(label);
            let _ = self.build_recursive(keyset, child_prefix, begin, end, (label as u32 ^ offset) as UnitID);
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

    fn find_offset(&self, unit_id: UnitID, labels: &[u8], depth: usize) -> Option<u32> {
        let head_block = (self.blocks.len() as i32 - NUM_TARGET_BLOCKS).max(0) as usize;
        if self.regular {
            // Among the first K valid offsets in the searched blocks, pick the one
            // whose value shares the most high bits with this triplet-position's
            // running target (smallest XOR distance). K-capped so build stays fast.
            const K: usize = 64;
            let target = self.targets[depth % 3];
            let mut best: Option<u32> = None;
            let mut seen = 0usize;
            'outer: for block in self.blocks.iter().skip(head_block) {
                for offset in block.find_offset(unit_id, labels) {
                    let offset_u32 = (block.id as u32) << 8 | offset as u32;
                    if self.used_offsets.contains(&offset_u32) {
                        continue;
                    }
                    if best.is_none() || (offset_u32 ^ target) < (best.unwrap() ^ target) {
                        best = Some(offset_u32);
                    }
                    seen += 1;
                    if seen >= K {
                        break 'outer;
                    }
                }
            }
            return best;
        }
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
    let Some(root_unit) = unit_at(da, 0) else {
        return hits;
    };
    let root = offset(root_unit);
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

/// Build a corpus-frequency-weighted trie in the yada-compatible format.
/// `entries` must be sorted by key (as `build_trie` does). Returns `None` on
/// builder failure (caller falls back to the default builder).
pub fn build_weighted(entries: &[(&str, u32)], corpus: &str) -> Option<Vec<u8>> {
    let baseline = yada::builder::DoubleArrayBuilder::build(entries)?;
    let lines: Vec<&str> = corpus.lines().collect();
    let prefix_hits = record_prefix_hits(&baseline, &lines);
    let keyset: Vec<(Vec<u8>, u32)> = entries
        .iter()
        .map(|(k, v)| (k.as_bytes().to_vec(), *v))
        .collect();
    let regular = std::env::var("SUDACHI_LAYOUT_OFFSET_REGULAR").is_ok();
    WeightedDoubleArrayBuilder::new(&prefix_hits, regular).build(&keyset)
}
