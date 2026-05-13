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

use crate::analysis::created::CreatedWords;
use crate::analysis::inner::{Node, NodeIdx};
use crate::analysis::lattice::Lattice;
#[cfg(feature = "profile")]
use crate::analysis::node::RightId;
use crate::analysis::node::{LatticeNode, ResultNode};
use crate::analysis::stateless_tokenizer::{dump_path, split_path, DictionaryAccess};
use crate::analysis::Mode;
use crate::dic::category_type::CategoryType;
use crate::dic::connect::ConnectionMatrix;
use crate::dic::lexicon::word_infos::WordInfoData;
use crate::dic::lexicon_set::LexiconSet;
use crate::dic::subset::InfoSubset;
use crate::error::{SudachiError, SudachiResult};
use crate::input_text::InputBuffer;
use crate::input_text::InputTextIndex;
use crate::plugin::oov::OovProviderPlugin;
use crate::prelude::MorphemeList;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TokenizerOptimization {
    pub exact_right_id_pruning: bool,
    pub beam_width: Option<usize>,
    pub beam_margin: Option<i32>,
    pub oov_limit: Option<usize>,
}

impl Default for TokenizerOptimization {
    fn default() -> Self {
        Self {
            exact_right_id_pruning: false,
            beam_width: None,
            beam_margin: None,
            oov_limit: None,
        }
    }
}

pub struct StatefulTokenizer<D> {
    dictionary: D,
    input: InputBuffer,
    debug: bool,
    mode: Mode,
    oov: Vec<Node>,
    oov_needs_buffer_context: bool,
    lattice: Lattice,
    top_path_ids: Vec<NodeIdx>,
    top_path: Option<Vec<ResultNode>>,
    subset: InfoSubset,
    optimization: TokenizerOptimization,
}

impl<D: DictionaryAccess + Clone> StatefulTokenizer<D> {
    /// Get a clone of current dictionary
    pub fn dict_clone(&self) -> D {
        self.dictionary.clone()
    }
}

impl<D: DictionaryAccess> StatefulTokenizer<D> {
    /// Create a new non-debug stateful tokenizer
    pub fn new(dic: D, mode: Mode) -> Self {
        Self::create(dic, false, mode)
    }

    /// Create a new debug stateful tokenizer with the following options
    pub fn create(dic: D, debug: bool, mode: Mode) -> Self {
        let oov_needs_buffer_context = dic
            .oov_provider_plugins()
            .iter()
            .any(|provider| provider.needs_oov_buffer_context());
        Self {
            dictionary: dic,
            input: InputBuffer::default(),
            debug,
            mode,
            oov: Vec::with_capacity(10),
            oov_needs_buffer_context,
            lattice: Lattice::default(),
            top_path_ids: Vec::new(),
            top_path: Some(Vec::new()),
            subset: InfoSubset::all(),
            optimization: TokenizerOptimization::default(),
        }
    }

    /// Set debug flag and returns the current one
    pub fn set_debug(&mut self, debug: bool) -> bool {
        std::mem::replace(&mut self.debug, debug)
    }

    /// Set the analysis mode and returns the current one
    pub fn set_mode(&mut self, mode: Mode) -> Mode {
        self.subset |= match mode {
            Mode::A => InfoSubset::SPLIT_A,
            Mode::B => InfoSubset::SPLIT_B,
            _ => InfoSubset::empty(),
        };
        std::mem::replace(&mut self.mode, mode)
    }

    /// Return current analysis mode
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Analyzer will read only following [`WordInfo`] field subset
    pub fn set_subset(&mut self, subset: InfoSubset) -> InfoSubset {
        let mode_subset = match self.mode {
            Mode::A => InfoSubset::SPLIT_A,
            Mode::B => InfoSubset::SPLIT_B,
            _ => InfoSubset::empty(),
        };
        let new_subset = (subset | mode_subset).normalize();
        std::mem::replace(&mut self.subset, new_subset | mode_subset)
    }

    pub fn set_optimization(
        &mut self,
        optimization: TokenizerOptimization,
    ) -> TokenizerOptimization {
        std::mem::replace(&mut self.optimization, optimization)
    }

    /// Prepare StatefulTokenizer for the next data.
    /// Data must be written in the returned reference.
    pub fn reset(&mut self) -> &mut String {
        if let Some(p) = self.top_path.as_mut() {
            p.clear()
        }
        self.oov.clear();
        self.input.reset()
    }

    /// Borrow current dictionary
    pub fn dict(&self) -> &D {
        &self.dictionary
    }

    /// Perform the actual tokenization so the analysis result will be available
    /// for consumption
    pub fn do_tokenize(&mut self) -> SudachiResult<()> {
        self.input.start_build()?;
        self.rewrite_input()?;
        self.input.build(self.dictionary.grammar())?;

        if self.input.current().is_empty() {
            return Ok(());
        }

        let debug = self.debug;

        if debug {
            println!("=== Input dump:\n{}", self.input.current());
        }

        self.build_lattice()?;

        if debug {
            println!("=== Lattice dump:");
            let dict = &self.dictionary;
            let mut writer = std::io::stdout();
            self.lattice
                .dump(&self.input, dict.grammar(), dict.lexicon(), &mut writer)?;
        };

        let mut path = self.resolve_best_path()?;

        if debug {
            println!("=== Before Rewriting:");
            dump_path(&path);
        };

        for plugin in self.dictionary.path_rewrite_plugins() {
            path = plugin.rewrite(&self.input, path, &self.lattice)?;
        }

        path = split_path(&self.dictionary, path, self.mode, self.subset, &self.input)?;

        if debug {
            println!("=== After Rewriting:");
            dump_path(&path);
            println!("===");
        };

        self.top_path = Some(path);

        Ok(())
    }

    /// Resolve the path (as ResultNodes) with the smallest cost
    fn resolve_best_path(&mut self) -> SudachiResult<Vec<ResultNode>> {
        let lex = self.dictionary.lexicon();
        let mut path = self.top_path.take().unwrap_or_default();
        self.lattice.fill_top_path(&mut self.top_path_ids);
        for &pid in self.top_path_ids.iter().rev() {
            let (inner, cost) = self.lattice.node(pid);
            let word_id = inner.word_id();
            #[cfg(feature = "profile")]
            if word_id.is_oov() && !word_id.is_special() {
                crate::profiling::count_oov_best_path_node();
            }
            let wi = if word_id.is_oov() {
                let curr_slice = self.input.curr_slice_c(inner.char_range()).to_owned();
                WordInfoData {
                    pos_id: word_id.word() as u16,
                    surface: curr_slice,
                    ..Default::default()
                }
                .into()
            } else {
                lex.get_word_info_subset(word_id, self.subset)?
            };

            let byte_begin = self.input.to_curr_byte_idx(inner.begin());
            let byte_end = self.input.to_curr_byte_idx(inner.end());

            path.push(ResultNode::new(
                *inner,
                cost,
                byte_begin as u16,
                byte_end as u16,
                wi,
            ));
        }
        self.top_path_ids.clear();
        Ok(path)
    }

    /// Swap result data with the current analyzer
    pub fn swap_result(
        &mut self,
        input: &mut InputBuffer,
        result: &mut Vec<ResultNode>,
        subset: &mut InfoSubset,
    ) {
        std::mem::swap(&mut self.input, input);
        std::mem::swap(self.top_path.as_mut().unwrap(), result);
        *subset = self.subset;
    }

    fn rewrite_input(&mut self) -> SudachiResult<()> {
        for p in self.dictionary.input_text_plugins() {
            p.rewrite(&mut self.input)?;
        }
        Ok(())
    }

    fn build_lattice(&mut self) -> SudachiResult<()> {
        let mut builder = LatticeBuilder {
            node_buffer: &mut self.oov,
            lattice: &mut self.lattice,
            matrix: self.dictionary.grammar().conn_matrix(),
            oov_providers: self.dictionary.oov_provider_plugins(),
            lexicon: self.dictionary.lexicon(),
            input: &self.input,
            oov_needs_buffer_context: self.oov_needs_buffer_context,
            optimization: self.optimization,
            debug: self.debug,
        };
        builder.build_lattice()
    }

    /// Consume the Tokenizer and produce MorphemeList
    pub fn into_morpheme_list(self) -> SudachiResult<MorphemeList<D>> {
        match self.top_path {
            None => Err(SudachiError::EosBosDisconnect),
            Some(path) => Ok(MorphemeList::from_components(
                self.dictionary,
                self.input,
                path,
                self.subset,
            )),
        }
    }
}

// This structure is purely for Rust.
// Otherwise splitting code into functions fails to compile with double borrow errors
struct LatticeBuilder<'a> {
    node_buffer: &'a mut Vec<Node>,
    lattice: &'a mut Lattice,
    matrix: &'a ConnectionMatrix<'a>,
    input: &'a InputBuffer,
    lexicon: &'a LexiconSet<'a>,
    oov_providers: &'a [Box<dyn OovProviderPlugin + Sync + Send>],
    oov_needs_buffer_context: bool,
    optimization: TokenizerOptimization,
    debug: bool,
}

impl<'a> LatticeBuilder<'a> {
    #[inline]
    fn build_lattice(&mut self) -> SudachiResult<()> {
        self.lattice.reset(self.input.current_chars().len());
        let input_bytes = self.input.current().as_bytes();
        let oov_needs_buffer_context = self.oov_needs_buffer_context;

        for (ch_off, &byte_off) in self.input.curr_byte_offsets().iter().enumerate() {
            if self.optimization.exact_right_id_pruning && !self.debug {
                self.lattice.prune_exact_boundary(ch_off, self.matrix);
            }
            self.lattice.prune_approx_boundary(
                ch_off,
                self.optimization.beam_width,
                self.optimization.beam_margin,
            );

            if !self.lattice.has_previous_node(ch_off) {
                #[cfg(feature = "profile")]
                crate::profiling::count_token_position_unreachable_skipped();
                continue;
            }
            #[cfg(feature = "profile")]
            crate::profiling::count_token_position_reachable();

            self.node_buffer.clear();
            let mut created = CreatedWords::default();
            self.lexicon.for_each_entry_with_params(
                input_bytes,
                byte_off,
                |word_id, end, left_id, right_id, cost| {
                    #[cfg(feature = "profile")]
                    crate::profiling::count_candidate_checked();

                    // do we really need input.can_bow condition?
                    if (end < input_bytes.len()) && !self.input.can_bow(end) {
                        #[cfg(feature = "profile")]
                        crate::profiling::count_rejected_candidate();
                        return;
                    }
                    let end_c = self.input.ch_idx(end);
                    let node = Node::new(
                        ch_off as u16,
                        end_c as u16,
                        left_id as u16,
                        right_id as u16,
                        cost,
                        word_id,
                    );
                    created = created.add_word_usize(end_c - ch_off);
                    if oov_needs_buffer_context {
                        self.node_buffer.push(node);
                    }
                    self.lattice.insert(node, self.matrix);
                    #[cfg(feature = "profile")]
                    crate::profiling::count_lattice_direct_candidate();
                },
            );

            // OOV
            if !oov_needs_buffer_context {
                self.node_buffer.clear();
            }
            if !self
                .input
                .cat_at_char(ch_off)
                .intersects(CategoryType::NOOOVBOW | CategoryType::NOOOVBOW2)
            {
                for provider in self.oov_providers {
                    created = self.provide_oovs(ch_off, created, provider.as_ref())?;
                }
            }

            if created.is_empty() {
                let provider = self.oov_providers.last().unwrap();
                created = self.provide_oovs(ch_off, created, provider.as_ref())?;
            }

            if created.is_empty() {
                return Err(SudachiError::EosBosDisconnect);
            }
        }
        let eos_boundary = self.input.current_chars().len();
        if self.optimization.exact_right_id_pruning && !self.debug {
            self.lattice.prune_exact_boundary(eos_boundary, self.matrix);
        }
        self.lattice.prune_approx_boundary(
            eos_boundary,
            self.optimization.beam_width,
            self.optimization.beam_margin,
        );
        self.lattice.connect_eos(self.matrix)?;

        Ok(())
    }

    #[inline]
    fn provide_oovs<P>(
        &mut self,
        char_offset: usize,
        mut other: CreatedWords,
        plugin: &P,
    ) -> SudachiResult<CreatedWords>
    where
        P: OovProviderPlugin + 'a + ?Sized,
    {
        let start_size = self.node_buffer.len();
        #[cfg(feature = "profile")]
        let provider_kind = plugin.profile_kind();
        #[cfg(feature = "profile")]
        crate::profiling::count_oov_provider_call_kind(provider_kind);
        #[cfg(feature = "profile")]
        {
            crate::profiling::count_oov_buffered_provider_call();
            let cap = self.node_buffer.capacity();
            crate::profiling::count_oov_temp_buffer_max_len(self.node_buffer.len());
            let num_provided =
                plugin.provide_oov(self.input, char_offset, other, self.node_buffer)?;
            if self.node_buffer.capacity() != cap {
                crate::profiling::count_oov_temp_buffer_growth();
            }
            crate::profiling::count_oov_buffered_candidates(num_provided);
            crate::profiling::count_oov_candidates_by_provider(provider_kind, num_provided);
            crate::profiling::count_oov_range(num_provided);
            crate::profiling::count_oov_temp_buffer_max_len(self.node_buffer.len());
            crate::profiling::count_oov_duplicate_candidates(count_duplicate_oov_candidates(
                self.node_buffer,
                start_size,
                num_provided,
            ));
            count_oov_dominance(self.input, self.node_buffer, start_size, num_provided);

            let num_inserted = self.limit_oov_candidates(num_provided);
            for idx in start_size..(start_size + num_inserted) {
                let node = self.node_buffer[idx];
                other = other.add_word_usize(node.char_range().len());
                self.lattice.insert(node, self.matrix);
                crate::profiling::count_lattice_direct_candidate();
                crate::profiling::count_oov_inserted_node();
            }
            self.node_buffer.truncate(start_size + num_inserted);
            return Ok(other);
        }

        #[cfg(not(feature = "profile"))]
        {
            let num_provided =
                plugin.provide_oov(self.input, char_offset, other, self.node_buffer)?;

            let num_inserted = self.limit_oov_candidates(num_provided);
            for idx in start_size..(start_size + num_inserted) {
                let node = self.node_buffer[idx];
                other = other.add_word_usize(node.char_range().len());
                self.lattice.insert(node, self.matrix);
            }
            self.node_buffer.truncate(start_size + num_inserted);
            Ok(other)
        }
    }

    #[inline]
    fn limit_oov_candidates(&self, num_provided: usize) -> usize {
        match self.optimization.oov_limit {
            Some(limit) => num_provided.min(limit),
            None => num_provided,
        }
    }
}

#[cfg(feature = "profile")]
fn count_duplicate_oov_candidates(nodes: &[Node], start: usize, len: usize) -> usize {
    let end = start + len;
    let mut duplicates = 0;
    for i in start..end {
        let node = &nodes[i];
        for prev in &nodes[start..i] {
            if node.begin() == prev.begin()
                && node.end() == prev.end()
                && node.left_id() == prev.left_id()
                && node.right_id() == prev.right_id()
                && node.cost() == prev.cost()
                && node.word_id() == prev.word_id()
            {
                duplicates += 1;
                break;
            }
        }
    }
    duplicates
}

#[cfg(feature = "profile")]
fn count_oov_dominance(input: &InputBuffer, nodes: &[Node], start: usize, len: usize) {
    let end = start + len;
    let mut dominated = 0usize;

    for i in start..end {
        let node = nodes[i];
        if nodes[start..i]
            .iter()
            .any(|prev| same_oov_dominance_key(node, *prev))
        {
            continue;
        }

        let mut group_len = 0usize;
        let mut min_cost = i16::MAX;
        for candidate in &nodes[start..end] {
            if same_oov_dominance_key(node, *candidate) {
                group_len += 1;
                min_cost = min_cost.min(candidate.cost());
            }
        }

        if group_len > 1 {
            crate::profiling::count_oov_dominance_group();
        }

        let mut min_cost_ties = 0usize;
        for candidate in &nodes[start..end] {
            if !same_oov_dominance_key(node, *candidate) {
                continue;
            }

            if candidate.cost() == min_cost {
                min_cost_ties += 1;
            } else {
                dominated += 1;
                crate::profiling::count_oov_strict_dominated_candidate(
                    oov_primary_category(input, candidate.begin()),
                    candidate.num_codepts(),
                );
            }
        }

        if min_cost_ties > 1 {
            crate::profiling::count_oov_equal_cost_ties(min_cost_ties - 1);
        }
    }

    crate::profiling::count_oov_unique_after_dominance(len - dominated);
}

#[cfg(feature = "profile")]
fn same_oov_dominance_key(a: Node, b: Node) -> bool {
    a.begin() == b.begin()
        && a.end() == b.end()
        && a.left_id() == b.left_id()
        && a.right_id() == b.right_id()
}

#[cfg(feature = "profile")]
fn oov_primary_category(input: &InputBuffer, offset: usize) -> CategoryType {
    let mut category = input.cat_at_char(offset);
    category.remove(CategoryType::NOOOVBOW | CategoryType::NOOOVBOW2);
    category.iter().next().unwrap_or(CategoryType::DEFAULT)
}
