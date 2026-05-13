/*
 *  Copyright (c) 2021-2024 Works Applications Co., Ltd.
 *
 *  Licensed under the Apache License, Version 2.0 (the "License");
 *  you may not use this file except in compliance with the License.
 *  You may obtain a copy of the License at
 *
 *      http://www.apache.org/licenses/LICENSE-2.0
 *
 *   Unless required by applicable law or agreed to in writing, software
 *  distributed under the License is distributed on an "AS IS" BASIS,
 *  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 *  See the License for the specific language governing permissions and
 *  limitations under the License.
 */

use crate::analysis::inner::{Node, NodeIdx};
use crate::analysis::node::{LatticeNode, PathCost, RightId};
use crate::dic::connect::ConnectionMatrix;
use crate::dic::grammar::Grammar;
use crate::dic::lexicon_set::LexiconSet;
use crate::dic::subset::InfoSubset;
use crate::dic::word_id::WordId;
use crate::error::SudachiResult;
use crate::input_text::InputBuffer;
use crate::prelude::SudachiError;
use std::fmt::{Display, Formatter};
use std::io::Write;

/// Lattice Node for Viterbi Search.
/// Extremely small for better cache locality.
/// Current implementation has 25% efficiency loss because of padding :(
/// Maybe we should use array-of-structs layout instead, but I want to try to measure the
/// efficiency of that without the effects of the current rewrite.
struct VNode {
    total_cost: i32,
    right_id: u16,
}

#[derive(Clone, Copy)]
struct BestPrev {
    cost: i32,
    left_id: u16,
    index: u16,
}

impl RightId for VNode {
    #[inline]
    fn right_id(&self) -> u16 {
        self.right_id
    }
}

impl PathCost for VNode {
    #[inline]
    fn total_cost(&self) -> i32 {
        self.total_cost
    }
}

impl VNode {
    #[inline]
    fn new(right_id: u16, total_cost: i32) -> VNode {
        VNode {
            right_id,
            total_cost,
        }
    }
}

/// Lattice which is constructed for performing the Viterbi search.
/// Contain several parallel arrays.
/// First level of parallel arrays is indexed by end word boundary.
/// Word boundaries are always aligned to codepoint boundaries, not to byte boundaries.
///
/// During the successive analysis, we do not drop inner vectors, so
/// the size of vectors never shrink.
/// You must use the size parameter to check the current size and never
/// access vectors after the end.
#[derive(Default)]
pub struct Lattice {
    ends: Vec<Vec<VNode>>,
    ends_full: Vec<Vec<Node>>,
    indices: Vec<Vec<NodeIdx>>,
    best_prev_cache: Vec<Vec<BestPrev>>,
    eos: Option<(NodeIdx, i32)>,
    size: usize,
}

impl Lattice {
    const DIRECT_SCAN_THRESHOLD: usize = 4;

    fn reset_vec<T>(
        data: &mut Vec<Vec<T>>,
        target: usize,
        clear_len: usize,
    ) -> (usize, usize, usize) {
        #[cfg(feature = "profile")]
        let mut items_cleared = 0usize;
        let visited = clear_len.min(data.len());
        for v in data.iter_mut().take(visited) {
            #[cfg(feature = "profile")]
            {
                items_cleared += v.len();
            }
            v.clear();
        }
        let cur_len = data.len();
        let _new_boundaries = target.saturating_sub(cur_len);
        if cur_len <= target {
            data.reserve(target - cur_len);
            for _ in cur_len..target {
                data.push(Vec::with_capacity(16))
            }
        }
        #[cfg(feature = "profile")]
        {
            (visited, items_cleared, _new_boundaries)
        }
        #[cfg(not(feature = "profile"))]
        {
            (0, 0, 0)
        }
    }

    /// Prepare lattice for the next analysis of a sentence with the
    /// specified length (in codepoints)
    pub fn reset(&mut self, length: usize) {
        #[cfg(feature = "profile")]
        let mut vecs_visited = 0usize;
        #[cfg(feature = "profile")]
        let mut items_cleared = 0usize;
        #[cfg(feature = "profile")]
        let mut new_boundaries = 0usize;

        let target = length + 1;
        let clear_len = self.size;
        for (_visited, _cleared, _new) in [
            Self::reset_vec(&mut self.ends, target, clear_len),
            Self::reset_vec(&mut self.ends_full, target, clear_len),
            Self::reset_vec(&mut self.indices, target, clear_len),
            Self::reset_vec(&mut self.best_prev_cache, target, clear_len),
        ] {
            #[cfg(feature = "profile")]
            {
                vecs_visited += _visited;
                items_cleared += _cleared;
                new_boundaries += _new;
            }
        }
        #[cfg(feature = "profile")]
        crate::profiling::count_lattice_reset(vecs_visited, items_cleared, new_boundaries);
        self.eos = None;
        self.size = length + 1;
        self.connect_bos();
    }

    fn connect_bos(&mut self) {
        self.ends[0].push(VNode::new(0, 0));
        #[cfg(feature = "profile")]
        crate::profiling::count_bos_node();
    }

    /// Find EOS node -- finish the lattice construction
    pub fn connect_eos(&mut self, conn: &ConnectionMatrix) -> SudachiResult<()> {
        let len = self.size;
        let eos_start = (len - 1) as u16;
        let eos_end = (len - 1) as u16;
        let node = Node::new(eos_start, eos_end, 0, 0, 0, WordId::EOS);
        let (idx, cost) = self.connect_node(&node, conn);
        if cost == i32::MAX {
            Err(SudachiError::EosBosDisconnect)
        } else {
            self.eos = Some((idx, cost));
            #[cfg(feature = "profile")]
            {
                crate::profiling::count_eos_node();
                self.record_boundary_stats();
            }
            Ok(())
        }
    }

    /// Insert a single node in the lattice, founding the path to the previous node
    /// Assumption: lattice for all previous boundaries is already constructed
    #[inline(always)]
    pub fn insert(&mut self, node: Node, conn: &ConnectionMatrix) -> i32 {
        let (idx, cost) = self.connect_node(&node, conn);
        let end_idx = node.end();
        debug_assert!(end_idx < self.size);

        #[cfg(feature = "profile")]
        let ends_cap = self.ends[end_idx].capacity();
        #[cfg(feature = "profile")]
        let indices_cap = self.indices[end_idx].capacity();
        #[cfg(feature = "profile")]
        let ends_full_cap = self.ends_full[end_idx].capacity();

        self.ends[end_idx].push(VNode::new(node.right_id(), cost));
        self.indices[end_idx].push(idx);
        self.ends_full[end_idx].push(node);

        #[cfg(feature = "profile")]
        {
            crate::profiling::count_inserted_node();
            if self.ends[end_idx].capacity() != ends_cap {
                crate::profiling::count_node_vec_growth();
            }
            if self.ends_full[end_idx].capacity() != ends_full_cap {
                crate::profiling::count_node_vec_growth();
            }
            if self.indices[end_idx].capacity() != indices_cap {
                crate::profiling::count_edge_vec_growth();
            }
        }

        cost
    }

    /// Find the path with the minimal cost through the lattice to the attached node
    /// Assumption: lattice for all previous boundaries is already constructed
    #[inline]
    pub fn connect_node(&mut self, r_node: &Node, conn: &ConnectionMatrix) -> (NodeIdx, i32) {
        let begin = r_node.begin();
        let node_cost = r_node.cost() as i32;
        let (prev_idx, base_cost) = self.best_prev(begin, r_node.left_id(), conn);
        let total_cost = if base_cost == i32::MAX {
            i32::MAX
        } else {
            base_cost + node_cost
        };

        (prev_idx, total_cost)
    }

    #[inline(always)]
    fn best_prev(&mut self, begin: usize, left_id: u16, conn: &ConnectionMatrix) -> (NodeIdx, i32) {
        debug_assert!(begin < self.size);
        let prev_nodes = self.ends[begin].len();
        #[cfg(feature = "profile")]
        {
            crate::profiling::count_best_prev_call();
            crate::profiling::count_lattice_prev_nodes(prev_nodes);
        }

        if prev_nodes == 0 {
            #[cfg(feature = "profile")]
            crate::profiling::count_best_prev_fast_empty();
            return (NodeIdx::empty(), i32::MAX);
        }

        if prev_nodes == 1 {
            #[cfg(feature = "profile")]
            crate::profiling::count_best_prev_fast_single();
            return self.scan_raw_prev(begin, left_id, conn);
        }

        if prev_nodes <= Self::DIRECT_SCAN_THRESHOLD {
            #[cfg(feature = "profile")]
            crate::profiling::count_best_prev_direct_small();
            return self.scan_raw_prev(begin, left_id, conn);
        }

        for entry in &self.best_prev_cache[begin] {
            if entry.left_id == left_id {
                #[cfg(feature = "profile")]
                crate::profiling::count_left_id_cache_hit(prev_nodes);
                let index = if entry.index == u16::MAX {
                    NodeIdx::empty()
                } else {
                    NodeIdx::new(begin as u16, entry.index)
                };
                return (index, entry.cost);
            }
        }

        let (prev_idx, base_cost) = self.scan_raw_prev(begin, left_id, conn);
        self.best_prev_cache[begin].push(BestPrev {
            cost: base_cost,
            left_id,
            index: prev_idx.index(),
        });

        #[cfg(feature = "profile")]
        {
            crate::profiling::count_left_id_cache_miss(prev_nodes);
            crate::profiling::count_best_prev_cache_push();
        }

        (prev_idx, base_cost)
    }

    #[inline(always)]
    fn scan_raw_prev(&self, begin: usize, left_id: u16, conn: &ConnectionMatrix) -> (NodeIdx, i32) {
        let mut min_cost = i32::MAX;
        let mut prev_idx = NodeIdx::empty();
        let costs = conn.costs_for_right(left_id);
        #[cfg(feature = "profile")]
        let mut checks = 0u64;
        #[cfg(feature = "profile")]
        let mut updates = 0u64;
        for (i, l_node) in self.ends[begin].iter().enumerate() {
            #[cfg(feature = "profile")]
            {
                checks += 1;
            }

            if !l_node.is_connected_to_bos() {
                continue;
            }

            let connect_cost = *unsafe { costs.get_unchecked(l_node.right_id() as usize) } as i32;
            let new_cost = l_node.total_cost() + connect_cost;
            if new_cost < min_cost {
                min_cost = new_cost;
                prev_idx = NodeIdx::new(begin as u16, i as u16);
                #[cfg(feature = "profile")]
                {
                    updates += 1;
                }
            }
        }

        #[cfg(feature = "profile")]
        {
            crate::profiling::count_connection_checks(checks, updates);
        }

        (prev_idx, min_cost)
    }

    #[cfg(feature = "profile")]
    fn record_boundary_stats(&self) {
        for (idx, nodes) in self.ends.iter().take(self.size).enumerate() {
            crate::profiling::count_boundary_nodes(nodes.len());
            crate::profiling::count_lattice_storage_capacity(
                nodes.capacity(),
                self.ends_full[idx].capacity(),
                self.indices[idx].capacity(),
                self.best_prev_cache[idx].capacity(),
            );
        }
        for cached in self.best_prev_cache.iter().take(self.size) {
            crate::profiling::count_left_id_boundary(cached.len());
        }
    }

    /// Checks if there exist at least one at the word end boundary
    pub fn has_previous_node(&self, i: usize) -> bool {
        self.ends.get(i).map(|d| !d.is_empty()).unwrap_or(false)
    }

    /// Lookup a node for the index
    pub fn node(&self, id: NodeIdx) -> (&Node, i32) {
        debug_assert!((id.end() as usize) < self.size);
        let node = &self.ends_full[id.end() as usize][id.index() as usize];
        let cost = self.ends[id.end() as usize][id.index() as usize].total_cost;
        (node, cost)
    }

    /// Fill the path with the minimum cost (node indices only).
    /// **Attention**: the path will be reversed (end to beginning) and will need to be traversed
    /// in the reverse order.
    pub fn fill_top_path(&self, result: &mut Vec<NodeIdx>) {
        if self.eos.is_none() {
            return;
        }
        // start with EOS
        let (mut idx, _) = self.eos.unwrap();
        result.push(idx);
        loop {
            let prev_idx = self.indices[idx.end() as usize][idx.index() as usize];
            if prev_idx.end() != 0 {
                // add if not BOS
                result.push(prev_idx);
                idx = prev_idx;
            } else {
                // finish if BOS
                break;
            }
        }
    }
}

#[cfg(feature = "profile")]
pub(crate) fn vnode_size() -> usize {
    std::mem::size_of::<VNode>()
}

#[cfg(feature = "profile")]
pub(crate) fn best_prev_size() -> usize {
    std::mem::size_of::<BestPrev>()
}

impl Lattice {
    pub fn dump<W: Write>(
        &self,
        input: &InputBuffer,
        grammar: &Grammar,
        lexicon: &LexiconSet,
        out: &mut W,
    ) -> SudachiResult<()> {
        enum PosData<'a> {
            Bos,
            Borrow(&'a [String]),
        }

        impl Display for PosData<'_> {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                match self {
                    PosData::Bos => write!(f, "BOS/EOS"),
                    PosData::Borrow(data) => {
                        for (i, s) in data.iter().enumerate() {
                            write!(f, "{}", s)?;
                            if i + 1 != data.len() {
                                write!(f, ", ")?;
                            }
                        }
                        Ok(())
                    }
                }
            }
        }

        let mut dump_idx = 0;

        for boundary in (0..self.size).rev() {
            for r_node in &self.ends_full[boundary] {
                let (surface, pos) = if r_node.is_special_node() {
                    ("(null)", PosData::Bos)
                } else if r_node.is_oov() {
                    let pos_id = r_node.word_id().word() as usize;
                    (
                        input.curr_slice_c(r_node.begin()..r_node.end()),
                        PosData::Borrow(&grammar.pos_list[pos_id]),
                    )
                } else {
                    let winfo =
                        lexicon.get_word_info_subset(r_node.word_id(), InfoSubset::POS_ID)?;
                    (
                        input.orig_slice_c(r_node.begin()..r_node.end()),
                        PosData::Borrow(&grammar.pos_list[winfo.pos_id() as usize]),
                    )
                };

                write!(
                    out,
                    "{}: {} {} {}{} {} {} {} {}:",
                    dump_idx,
                    r_node.begin(),
                    r_node.end(),
                    surface,
                    r_node.word_id(),
                    pos,
                    r_node.left_id(),
                    r_node.right_id(),
                    r_node.cost()
                )?;

                let conn = grammar.conn_matrix();

                for l_node in &self.ends[r_node.begin()] {
                    let connect_cost = conn.cost(l_node.right_id(), r_node.left_id());
                    write!(out, " {}", connect_cost)?;
                }

                writeln!(out)?;

                dump_idx += 1;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matrix_bytes(values: &[i16]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(values.len() * 2);
        for value in values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn active_range_reset_clears_previous_active_buckets() {
        let bytes = matrix_bytes(&[0]);
        let conn = ConnectionMatrix::from_offset_size(&bytes, 0, 1, 1).unwrap();
        let mut lattice = Lattice::default();

        lattice.reset(8);
        for cost in 1..=5 {
            lattice.insert(Node::new(0, 1, 0, 0, cost, WordId::oov(0)), &conn);
        }
        lattice.insert(Node::new(1, 8, 0, 0, 1, WordId::oov(0)), &conn);

        assert_eq!(lattice.size, 9);
        assert_eq!(lattice.ends[1].len(), 5);
        assert_eq!(lattice.ends_full[8].len(), 1);
        assert_eq!(lattice.indices[8].len(), 1);
        assert_eq!(lattice.best_prev_cache[1].len(), 1);
        let allocated_boundaries = lattice.ends.len();

        lattice.reset(2);

        assert_eq!(lattice.size, 3);
        assert_eq!(lattice.ends.len(), allocated_boundaries);
        assert_eq!(lattice.ends[0].len(), 1);
        assert!(lattice.ends[1].is_empty());
        assert!(lattice.ends[8].is_empty());
        assert!(lattice.ends_full[8].is_empty());
        assert!(lattice.indices[8].is_empty());
        assert!(lattice.best_prev_cache[1].is_empty());
    }
}
