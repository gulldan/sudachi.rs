/*
 *  Copyright (c) 2021 Works Applications Co., Ltd.
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

use super::output::{SudachiOutput, Writer};
use std::io::Write;
use sudachi::analysis::stateful_tokenizer::{StatefulTokenizer, TokenizerOptimization};
use sudachi::analysis::stateless_tokenizer::DictionaryAccess;
use sudachi::analysis::Mode;
use sudachi::dic::subset::InfoSubset;
use sudachi::prelude::MorphemeList;
use sudachi::sentence_splitter::{SentenceSplitter, SplitSentences};

pub trait Analysis {
    fn analyze(&mut self, input: &str, writer: &mut Writer);
}

pub struct SplitSentencesOnly<'a> {
    splitter: SentenceSplitter<'a>,
}

impl<'a> SplitSentencesOnly<'a> {
    pub fn new(dict: &'a impl DictionaryAccess) -> Self {
        let splitter = SentenceSplitter::new().with_checker(dict.lexicon());
        Self { splitter }
    }
}

impl<'a> Analysis for SplitSentencesOnly<'a> {
    fn analyze(&mut self, input: &str, writer: &mut Writer) {
        for (_, sent) in self.splitter.split(input) {
            writer.write_all(sent.as_bytes()).expect("write failed")
        }
    }
}

pub struct AnalyzeNonSplitted<D: DictionaryAccess, O: SudachiOutput<D>> {
    output: O,
    analyzer: StatefulTokenizer<D>,
    morphemes: MorphemeList<D>,
}

impl<D: DictionaryAccess + Clone, O: SudachiOutput<D>> AnalyzeNonSplitted<D, O> {
    pub fn new(output: O, dict: D, mode: Mode, enable_debug: bool) -> Self {
        Self::new_with_optimization(
            output,
            dict,
            mode,
            enable_debug,
            TokenizerOptimization::default(),
        )
    }

    pub fn new_with_optimization(
        output: O,
        dict: D,
        mode: Mode,
        enable_debug: bool,
        optimization: TokenizerOptimization,
    ) -> Self {
        let subset = output.subset() | required_rewrite_subset(&dict);
        let mut analyzer = StatefulTokenizer::create(dict.clone(), enable_debug, mode);
        analyzer.set_subset(subset);
        analyzer.set_optimization(optimization);

        Self {
            output,
            morphemes: MorphemeList::empty(dict),
            analyzer,
        }
    }
}

fn required_rewrite_subset<D: DictionaryAccess>(dict: &D) -> InfoSubset {
    dict.path_rewrite_plugins()
        .iter()
        .fold(InfoSubset::empty(), |subset, plugin| {
            subset | plugin.required_subset()
        })
}

impl<D: DictionaryAccess, O: SudachiOutput<D>> Analysis for AnalyzeNonSplitted<D, O> {
    fn analyze(&mut self, input: &str, writer: &mut Writer) {
        self.analyzer.reset().push_str(input);
        self.analyzer
            .do_tokenize()
            .unwrap_or_else(|e| panic!("tokenization failed, input: {}\n{}", input, e));
        self.morphemes
            .collect_results(&mut self.analyzer)
            .expect("result collection failed");
        self.output
            .write(writer, &self.morphemes)
            .expect("write result failed");
    }
}

pub struct AnalyzeSplitted<'a, D: DictionaryAccess + 'a, O: SudachiOutput<&'a D>> {
    splitter: SentenceSplitter<'a>,
    inner: AnalyzeNonSplitted<&'a D, O>,
}

impl<'a, D: DictionaryAccess + 'a, O: SudachiOutput<&'a D>> AnalyzeSplitted<'a, D, O> {
    pub fn new(output: O, dict: &'a D, mode: Mode, enable_debug: bool) -> Self {
        Self::new_with_optimization(
            output,
            dict,
            mode,
            enable_debug,
            TokenizerOptimization::default(),
        )
    }

    pub fn new_with_optimization(
        output: O,
        dict: &'a D,
        mode: Mode,
        enable_debug: bool,
        optimization: TokenizerOptimization,
    ) -> Self {
        Self {
            inner: AnalyzeNonSplitted::new_with_optimization(
                output,
                dict,
                mode,
                enable_debug,
                optimization,
            ),
            splitter: SentenceSplitter::new().with_checker(dict.lexicon()),
        }
    }
}

impl<'a, D: DictionaryAccess + 'a, O: SudachiOutput<&'a D>> Analysis for AnalyzeSplitted<'a, D, O> {
    fn analyze(&mut self, input: &str, writer: &mut Writer) {
        for (_, sent) in self.splitter.split(input) {
            self.inner.analyze(sent, writer);
        }
    }
}
