/*
 *  Copyright (c) 2021-2026 Works Applications Co., Ltd.
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
use crate::analysis::node::{LatticeNode, ResultNode};
use crate::analysis::stateless_tokenizer::{dump_path, split_path};
use crate::analysis::Mode;
use crate::dic::connect::ConnectionMatrix;
use crate::dic::lexicon::LexiconTrieHit;
use crate::dic::lexicon_set::LexiconSet;
use crate::dic::subset::InfoSubset;
use crate::dic::word_info::WordInfo;
use crate::dic::DictionaryAccess;
use crate::error::{SudachiError, SudachiResult};
use crate::input_text::InputBuffer;
use crate::plugin::oov::OovProviderPlugin;
use crate::prelude::MorphemeList;

pub struct StatefulTokenizer<D> {
    dictionary: D,
    input: InputBuffer,
    debug: bool,
    mode: Mode,
    oov: Vec<Node>,
    lattice: Lattice,
    top_path_ids: Vec<NodeIdx>,
    top_path: Option<Vec<ResultNode>>,
    subset: InfoSubset,
    /// Per-boundary dictionary-match cache, reused across sentences, for the
    /// pipelined-prefetch lattice path (issue #117).
    trie_hit_cache: Vec<Vec<LexiconTrieHit>>,
    /// Use the pipelined + prefetched dictionary lookup in `build_lattice`.
    pipelined_lookup: bool,
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
        Self {
            dictionary: dic,
            input: InputBuffer::default(),
            debug,
            mode,
            oov: Vec::with_capacity(10),
            lattice: Lattice::default(),
            top_path_ids: Vec::new(),
            top_path: Some(Vec::new()),
            subset: InfoSubset::all(),
            trie_hit_cache: Vec::new(),
            // Pipelined + prefetched dictionary lookup is the default: it is
            // byte-for-byte identical to the scalar path and ~8-12% faster
            // end-to-end on real SudachiDict tiers (issue #117).
            pipelined_lookup: true,
        }
    }

    /// Enable or disable the pipelined + prefetched dictionary lookup in lattice
    /// construction (issue #117). Returns the previous value. The result is
    /// byte-for-byte identical to the scalar path; this only changes lookup
    /// performance and is intended for benchmarking and tuning.
    pub fn set_pipelined_lookup(&mut self, enabled: bool) -> bool {
        std::mem::replace(&mut self.pipelined_lookup, enabled)
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
            path = plugin.rewrite(&self.input, path, &self.lattice, self.dictionary.lexicon())?;
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
        let lexset = self.dictionary.lexicon();
        let mut path = self.top_path.take().unwrap_or_default();
        self.lattice.fill_top_path(&mut self.top_path_ids);
        self.top_path_ids.reverse();
        for pid in self.top_path_ids.drain(..) {
            let (inner, cost) = self.lattice.node(pid);
            let wi = if inner.word_id().is_oov() {
                let curr_slice = self.input.curr_slice_c(inner.char_range()).to_owned();
                WordInfo::new_oov(
                    inner.word_id().entry().as_raw() as u16,
                    curr_slice.len() as i16,
                    inner.word_id(),
                    curr_slice,
                )
            } else {
                lexset.get_word_info_subset(inner.word_id(), self.subset)?
            };

            let byte_begin = self.input.to_curr_byte_idx(inner.begin());
            let byte_end = self.input.to_curr_byte_idx(inner.end());

            path.push(ResultNode::new(
                inner.clone(),
                cost,
                byte_begin as u16,
                byte_end as u16,
                wi,
            ));
        }
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
            trie_hit_cache: &mut self.trie_hit_cache,
            pipelined: self.pipelined_lookup,
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
    trie_hit_cache: &'a mut Vec<Vec<LexiconTrieHit>>,
    pipelined: bool,
}

impl<'a> LatticeBuilder<'a> {
    #[inline]
    fn build_lattice(&mut self) -> SudachiResult<()> {
        self.lattice.reset(self.input.current_chars().len());
        if self.pipelined {
            self.build_lattice_pipelined()
        } else {
            self.build_lattice_scalar()
        }
    }

    /// Original lattice builder: one scalar common-prefix walk per reachable
    /// boundary, interleaved with node insertion.
    #[inline]
    fn build_lattice_scalar(&mut self) -> SudachiResult<()> {
        let input_bytes = self.input.current().as_bytes();

        for (ch_off, &byte_off) in self.input.curr_byte_offsets().iter().enumerate() {
            if !self.lattice.has_previous_node(ch_off) {
                continue;
            }

            self.node_buffer.clear();
            let mut created = CreatedWords::default();
            for e in self.lexicon.lookup(input_bytes, byte_off) {
                // do we really need input.can_bow condition?
                if (e.end < input_bytes.len()) && !self.input.can_bow(e.end) {
                    continue;
                }
                let (left_id, right_id, cost) = self.lexicon.get_word_param(e.word_id);
                let end_c = self.input.ch_idx(e.end);
                let node = Node::new(
                    ch_off as u16,
                    end_c as u16,
                    left_id as u16,
                    right_id as u16,
                    cost,
                    e.word_id,
                );
                created = created.add_word((end_c - ch_off) as i64);
                self.node_buffer.push(node.clone());
                self.lattice.insert(node, self.matrix);
            }

            self.insert_oovs(ch_off, created)?;
        }
        self.lattice.connect_eos(self.matrix)?;

        Ok(())
    }

    /// Pipelined + prefetched lattice builder (issue #117): first collect raw
    /// trie hits with overlapped trie memory latency, then expand hits and
    /// insert nodes for reachable boundaries in the same order as the scalar
    /// path. Keeping WordIdTable/params/lattice traffic out of the trie phase
    /// avoids competing with the trie array for L1 cache.
    #[inline]
    fn build_lattice_pipelined(&mut self) -> SudachiResult<()> {
        let input_bytes = self.input.current().as_bytes();
        let starts = self.input.curr_byte_offsets();

        self.collect_trie_hits(input_bytes, starts);

        let lexicon = self.lexicon;
        for ch_off in 0..starts.len() {
            if !self.lattice.has_previous_node(ch_off) {
                continue;
            }

            self.node_buffer.clear();
            let mut created = CreatedWords::default();
            // Indexed access keeps each `trie_hit_cache` borrow transient so it
            // does not conflict with the node_buffer/lattice mutations below.
            let count = self.trie_hit_cache[ch_off].len();
            for idx in 0..count {
                let hit = self.trie_hit_cache[ch_off][idx];
                let end = hit.end;
                if (end < input_bytes.len()) && !self.input.can_bow(end) {
                    continue;
                }
                for entry in lexicon.entries_for_trie_hit(hit) {
                    let (left_id, right_id, cost) = lexicon.get_word_param(entry.word_id);
                    let end_c = self.input.ch_idx(entry.end);
                    let node = Node::new(
                        ch_off as u16,
                        end_c as u16,
                        left_id as u16,
                        right_id as u16,
                        cost,
                        entry.word_id,
                    );
                    created = created.add_word((end_c - ch_off) as i64);
                    self.node_buffer.push(node.clone());
                    self.lattice.insert(node, self.matrix);
                }
            }

            self.insert_oovs(ch_off, created)?;
        }
        self.lattice.connect_eos(self.matrix)?;

        Ok(())
    }

    #[inline]
    fn collect_trie_hits(&mut self, input_bytes: &[u8], starts: &[usize]) {
        let cache = &mut *self.trie_hit_cache;
        if cache.len() < starts.len() {
            cache.resize_with(starts.len(), Vec::new);
        }
        for bucket in cache.iter_mut().take(starts.len()) {
            bucket.clear();
        }
        self.lexicon
            .lookup_trie_batch(input_bytes, starts, |bucket, hit| {
                cache[bucket].push(hit);
            });
    }

    /// OOV handling shared by both lattice builders. Mirrors the original
    /// in-loop logic exactly.
    #[inline]
    fn insert_oovs(&mut self, ch_off: usize, mut created: CreatedWords) -> SudachiResult<()> {
        if self.input.can_oov_bow(ch_off) {
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
        let num_provided = plugin.provide_oov(self.input, char_offset, other, self.node_buffer)?;
        for idx in start_size..(start_size + num_provided) {
            let node = self.node_buffer[idx].clone();
            other = other.add_word(node.char_range().len() as i64);
            self.lattice.insert(node, self.matrix);
        }
        Ok(other)
    }
}

#[cfg(test)]
mod issue_117_probe {
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::config::Config;
    use crate::dic::dictionary::JapaneseDictionary;

    #[derive(Default)]
    struct ProbeTotals {
        lines: usize,
        chars: usize,
        starts: usize,
        reachable: usize,
        raw_hits: usize,
        raw_hits_reachable: usize,
        raw_hits_unreachable: usize,
        trie_leaves_expanded: usize,
        expanded_entries: usize,
        can_bow_rejected: usize,
        dict_nodes_inserted: usize,
        dict_prev_nodes_scanned: usize,
        max_prev_nodes: usize,
        oov_boundaries: usize,
        old_batch_entries: usize,
        scalar_all_entries: usize,
        scalar_reachable_entries: usize,
        strict_total: Duration,
        strict_collect: Duration,
        strict_expand_insert: Duration,
        strict_oov: Duration,
        strict_eos: Duration,
        old_batch_all: Duration,
        scalar_all: Duration,
        scalar_reachable: Duration,
    }

    impl ProbeTotals {
        fn ns_per_char(duration: Duration, chars: usize) -> f64 {
            duration.as_secs_f64() * 1e9 / chars.max(1) as f64
        }

        fn pct(part: usize, total: usize) -> f64 {
            part as f64 * 100.0 / total.max(1) as f64
        }

        fn print(&self) {
            println!(
                "# lines={} chars={} starts={} reachable={} ({:.1}%)",
                self.lines,
                self.chars,
                self.starts,
                self.reachable,
                Self::pct(self.reachable, self.starts)
            );
            println!(
                "# raw_hits={} reachable={} unreachable={} ({:.1}% wasted hits)",
                self.raw_hits,
                self.raw_hits_reachable,
                self.raw_hits_unreachable,
                Self::pct(self.raw_hits_unreachable, self.raw_hits)
            );
            println!(
                "# leaves_expanded={} entries={} can_bow_rejected={} inserted={}",
                self.trie_leaves_expanded,
                self.expanded_entries,
                self.can_bow_rejected,
                self.dict_nodes_inserted
            );
            println!(
                "# dict_prev_nodes_scanned={} avg_prev_per_insert={:.2} max_prev_nodes={}",
                self.dict_prev_nodes_scanned,
                self.dict_prev_nodes_scanned as f64 / self.dict_nodes_inserted.max(1) as f64,
                self.max_prev_nodes
            );
            println!(
                "# old_batch_entries={} scalar_all_entries={} scalar_reachable_entries={}",
                self.old_batch_entries, self.scalar_all_entries, self.scalar_reachable_entries
            );
            println!(
                "strict_total        {:>8.2} ms  {:>7.2} ns/char",
                self.strict_total.as_secs_f64() * 1e3,
                Self::ns_per_char(self.strict_total, self.chars)
            );
            println!(
                "  collect_raw      {:>8.2} ms  {:>7.2} ns/char",
                self.strict_collect.as_secs_f64() * 1e3,
                Self::ns_per_char(self.strict_collect, self.chars)
            );
            println!(
                "  expand_insert    {:>8.2} ms  {:>7.2} ns/char",
                self.strict_expand_insert.as_secs_f64() * 1e3,
                Self::ns_per_char(self.strict_expand_insert, self.chars)
            );
            println!(
                "  oov              {:>8.2} ms  {:>7.2} ns/char",
                self.strict_oov.as_secs_f64() * 1e3,
                Self::ns_per_char(self.strict_oov, self.chars)
            );
            println!(
                "  eos              {:>8.2} ms  {:>7.2} ns/char",
                self.strict_eos.as_secs_f64() * 1e3,
                Self::ns_per_char(self.strict_eos, self.chars)
            );
            println!(
                "old_batch_all      {:>8.2} ms  {:>7.2} ns/char",
                self.old_batch_all.as_secs_f64() * 1e3,
                Self::ns_per_char(self.old_batch_all, self.chars)
            );
            println!(
                "scalar_all         {:>8.2} ms  {:>7.2} ns/char",
                self.scalar_all.as_secs_f64() * 1e3,
                Self::ns_per_char(self.scalar_all, self.chars)
            );
            println!(
                "scalar_reachable   {:>8.2} ms  {:>7.2} ns/char",
                self.scalar_reachable.as_secs_f64() * 1e3,
                Self::ns_per_char(self.scalar_reachable, self.chars)
            );
        }
    }

    fn env_path(key: &str, default: &str) -> PathBuf {
        std::env::var_os(key)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(default))
    }

    fn env_usize(key: &str, default: usize) -> usize {
        std::env::var(key)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    }

    fn build_input(dict: &JapaneseDictionary, line: &str) -> SudachiResult<InputBuffer> {
        let mut input = InputBuffer::default();
        input.reset().push_str(line);
        input.start_build()?;
        for plugin in dict.input_text_plugins() {
            plugin.rewrite(&mut input)?;
        }
        input.build(dict.grammar())?;
        Ok(input)
    }

    fn probe_strict_build(
        dict: &JapaneseDictionary,
        input: &InputBuffer,
        node_buffer: &mut Vec<Node>,
        lattice: &mut Lattice,
        trie_hit_cache: &mut Vec<Vec<LexiconTrieHit>>,
        totals: &mut ProbeTotals,
        reachable_out: &mut Vec<usize>,
    ) -> SudachiResult<()> {
        let mut builder = LatticeBuilder {
            node_buffer,
            lattice,
            matrix: dict.grammar().conn_matrix(),
            input,
            lexicon: dict.lexicon(),
            oov_providers: dict.oov_provider_plugins(),
            trie_hit_cache,
            pipelined: true,
        };

        let input_bytes = input.current().as_bytes();
        let starts = input.curr_byte_offsets();
        let total_start = Instant::now();
        builder.lattice.reset(input.current_chars().len());

        let t = Instant::now();
        builder.collect_trie_hits(input_bytes, starts);
        totals.strict_collect += t.elapsed();

        totals.starts += starts.len();
        totals.raw_hits += builder
            .trie_hit_cache
            .iter()
            .take(starts.len())
            .map(Vec::len)
            .sum::<usize>();

        let t = Instant::now();
        for ch_off in 0..starts.len() {
            let hit_count = builder.trie_hit_cache[ch_off].len();
            if !builder.lattice.has_previous_node(ch_off) {
                totals.raw_hits_unreachable += hit_count;
                continue;
            }

            totals.reachable += 1;
            totals.raw_hits_reachable += hit_count;
            reachable_out.push(ch_off);

            builder.node_buffer.clear();
            let mut created = CreatedWords::default();

            let lexicon = builder.lexicon;
            for idx in 0..hit_count {
                let hit = builder.trie_hit_cache[ch_off][idx];
                totals.trie_leaves_expanded += 1;
                let end = hit.end;
                if (end < input_bytes.len()) && !builder.input.can_bow(end) {
                    totals.can_bow_rejected += 1;
                    continue;
                }
                for entry in lexicon.entries_for_trie_hit(hit) {
                    totals.expanded_entries += 1;
                    let (left_id, right_id, cost) = lexicon.get_word_param(entry.word_id);
                    let end_c = builder.input.ch_idx(entry.end);
                    let node = Node::new(
                        ch_off as u16,
                        end_c as u16,
                        left_id as u16,
                        right_id as u16,
                        cost,
                        entry.word_id,
                    );
                    created = created.add_word((end_c - ch_off) as i64);
                    builder.node_buffer.push(node.clone());
                    let prev_nodes = builder.lattice.previous_node_count(ch_off);
                    totals.dict_prev_nodes_scanned += prev_nodes;
                    totals.max_prev_nodes = totals.max_prev_nodes.max(prev_nodes);
                    builder.lattice.insert(node, builder.matrix);
                    totals.dict_nodes_inserted += 1;
                }
            }

            let before = created;
            builder.insert_oovs(ch_off, created)?;
            if before.is_empty() || builder.input.can_oov_bow(ch_off) {
                totals.oov_boundaries += 1;
            }
        }
        totals.strict_expand_insert += t.elapsed();

        let t = Instant::now();
        builder.lattice.connect_eos(builder.matrix)?;
        totals.strict_eos += t.elapsed();
        totals.strict_total += total_start.elapsed();

        Ok(())
    }

    #[test]
    #[ignore = "issue #117 phase probe; set SUDACHI_BENCH_* env vars and run under --release --nocapture"]
    fn pipeline_phase_probe_issue_117() {
        let config_path = env_path("SUDACHI_BENCH_CONFIG", "resources/sudachi.json");
        let resource_dir = config_path
            .parent()
            .map(|p| p.to_path_buf())
            .filter(|p| !p.as_os_str().is_empty());
        let dict_override = std::env::var_os("SUDACHI_BENCH_DICT").map(PathBuf::from);
        let config = Config::new(Some(config_path.clone()), resource_dir, dict_override)
            .expect("failed to load config");
        let dict = Arc::new(JapaneseDictionary::from_cfg(&config).expect("failed to load dict"));
        let inputs_path = env_path(
            "SUDACHI_BENCH_INPUTS",
            "target/issue-117-corpora/kyoto-leads.txt",
        );
        let limit = env_usize("SUDACHI_BENCH_LIMIT", usize::MAX);
        let text = std::fs::read_to_string(&inputs_path).expect("failed to read inputs");

        let mut totals = ProbeTotals::default();
        let mut reachable = Vec::new();
        let mut node_buffer = Vec::with_capacity(10);
        let mut lattice = Lattice::default();
        let mut trie_hit_cache = Vec::new();
        for line in text.lines().take(limit) {
            let input = build_input(&dict, line).expect("failed to build input");
            if input.current().is_empty() {
                continue;
            }
            totals.lines += 1;
            totals.chars += input.current().chars().count();

            reachable.clear();
            probe_strict_build(
                &dict,
                &input,
                &mut node_buffer,
                &mut lattice,
                &mut trie_hit_cache,
                &mut totals,
                &mut reachable,
            )
            .expect("strict build probe failed");

            let input_bytes = input.current().as_bytes();
            let starts = input.curr_byte_offsets();
            let lexicon = dict.lexicon();

            let t = Instant::now();
            lexicon.lookup_batch(input_bytes, starts, |_, _| {
                totals.old_batch_entries += 1;
            });
            totals.old_batch_all += t.elapsed();

            let t = Instant::now();
            for &start in starts {
                totals.scalar_all_entries += lexicon.lookup(input_bytes, start).count();
            }
            totals.scalar_all += t.elapsed();

            let t = Instant::now();
            for &ch_off in &reachable {
                totals.scalar_reachable_entries +=
                    lexicon.lookup(input_bytes, starts[ch_off]).count();
            }
            totals.scalar_reachable += t.elapsed();
        }

        println!("# config: {}", config_path.display());
        println!("# inputs: {}", inputs_path.display());
        totals.print();
    }
}
