/*
 * Copyright (c) 2021-2024 Works Applications Co., Ltd.
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

use std::env;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use criterion::{black_box, criterion_group, criterion_main, Criterion};

use sudachi::analysis::stateful_tokenizer::StatefulTokenizer;
use sudachi::analysis::Mode;
use sudachi::config::Config;
use sudachi::dic::dictionary::JapaneseDictionary;
use sudachi::prelude::MorphemeList;
use sudachi::sentence_splitter::{SentenceSplitter, SplitSentences};

const BASE_CORPUS: &[&str] = &[
    "東京都",
    "日本語形態素解析器Sudachiの高速化。",
    "1. 概要。2. 詳細。",
    "3.141は円周率です。",
    "モーニング娘。の楽曲一覧",
    "あ（いう。え）お",
    "Rust版とJava版、Python版の結果を比較する。",
    "正規化された表記と辞書形を取得する。",
    "項目A.と項目B.が並ぶ。",
    "複数の文を含むタイトル。追加の文です！",
    "改行相当<br><br>次の文",
    "中黒・・・次の文",
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("sudachi crate must be inside workspace")
        .to_path_buf()
}

fn load_dictionary() -> JapaneseDictionary {
    let root = workspace_root();
    let config_file = env::var_os("SUDACHI_BENCH_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("resources/sudachi.json"));
    let resource_dir = env::var_os("SUDACHI_BENCH_RESOURCE_DIR")
        .map(PathBuf::from)
        .or_else(|| Some(root.join("resources")));
    let dictionary_path = env::var_os("SUDACHI_BENCH_DICT").map(PathBuf::from);
    let config = Config::new(Some(config_file), resource_dir, dictionary_path)
        .expect("failed to load benchmark config");

    JapaneseDictionary::from_cfg(&config).expect("failed to load benchmark dictionary")
}

fn mode_from_env() -> Mode {
    env::var("SUDACHI_BENCH_MODE")
        .ok()
        .as_deref()
        .map(Mode::from_str)
        .transpose()
        .expect("SUDACHI_BENCH_MODE must be A, B, or C")
        .unwrap_or(Mode::C)
}

fn bench_corpus() -> Vec<String> {
    let repeat = env::var("SUDACHI_BENCH_REPEAT")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(256);
    let mut corpus = Vec::with_capacity(BASE_CORPUS.len() * repeat);
    for _ in 0..repeat {
        corpus.extend(BASE_CORPUS.iter().map(|line| (*line).to_owned()));
    }
    corpus
}

fn tokenize_line(
    analyzer: &mut StatefulTokenizer<&JapaneseDictionary>,
    result: &mut MorphemeList<&JapaneseDictionary>,
    input: &str,
) {
    analyzer.reset().push_str(input);
    analyzer.do_tokenize().expect("tokenization failed");
    result
        .collect_results(analyzer)
        .expect("result collection failed");
}

fn exercise_accessors(result: &MorphemeList<&JapaneseDictionary>) -> usize {
    let mut checksum = 0usize;
    let mut split_a = result.empty_clone();
    let mut split_b = result.empty_clone();

    for morpheme in result.iter() {
        checksum = checksum.wrapping_add(morpheme.surface().len());
        checksum = checksum.wrapping_add(morpheme.dictionary_form().len());
        checksum = checksum.wrapping_add(morpheme.normalized_form().len());

        let word_info = morpheme.get_word_info();
        checksum = checksum.wrapping_add(word_info.a_unit_split().len());
        checksum = checksum.wrapping_add(word_info.b_unit_split().len());

        split_a.clear();
        if morpheme
            .split_into(Mode::A, &mut split_a)
            .expect("A split failed")
        {
            checksum = checksum.wrapping_add(split_a.len());
        }

        split_b.clear();
        if morpheme
            .split_into(Mode::B, &mut split_b)
            .expect("B split failed")
        {
            checksum = checksum.wrapping_add(split_b.len());
        }
    }

    checksum
}

fn bench_pipeline(c: &mut Criterion) {
    let dict = load_dictionary();
    let corpus = bench_corpus();
    let mode = mode_from_env();
    let plain_splitter = SentenceSplitter::new();
    let splitter = SentenceSplitter::new().with_checker(dict.lexicon());

    let mut group = c.benchmark_group("pipeline");
    group.bench_function("split_only_plain", |b| {
        b.iter(|| {
            let mut bytes = 0usize;
            for line in &corpus {
                for (_, sentence) in plain_splitter.split(black_box(line.as_str())) {
                    bytes = bytes.wrapping_add(sentence.len());
                }
            }
            black_box(bytes)
        })
    });

    group.bench_function("split_only_with_checker", |b| {
        b.iter(|| {
            let mut bytes = 0usize;
            for line in &corpus {
                for (_, sentence) in splitter.split(black_box(line.as_str())) {
                    bytes = bytes.wrapping_add(sentence.len());
                }
            }
            black_box(bytes)
        })
    });

    group.bench_function("tokenize_only", |b| {
        let mut analyzer = StatefulTokenizer::create(&dict, false, mode);
        let mut result = MorphemeList::empty(&dict);
        b.iter(|| {
            let mut morphemes = 0usize;
            for line in &corpus {
                tokenize_line(&mut analyzer, &mut result, black_box(line.as_str()));
                morphemes = morphemes.wrapping_add(result.len());
            }
            black_box(morphemes)
        })
    });

    group.bench_function("tokenize_accessors_splits", |b| {
        let mut analyzer = StatefulTokenizer::create(&dict, false, mode);
        let mut result = MorphemeList::empty(&dict);
        b.iter(|| {
            let mut checksum = 0usize;
            for line in &corpus {
                tokenize_line(&mut analyzer, &mut result, black_box(line.as_str()));
                checksum = checksum.wrapping_add(exercise_accessors(&result));
            }
            black_box(checksum)
        })
    });

    group.bench_function("split_tokenize_accessors_splits", |b| {
        let mut analyzer = StatefulTokenizer::create(&dict, false, mode);
        let mut result = MorphemeList::empty(&dict);
        b.iter(|| {
            let mut checksum = 0usize;
            for line in &corpus {
                for (_, sentence) in splitter.split(black_box(line.as_str())) {
                    tokenize_line(&mut analyzer, &mut result, sentence);
                    checksum = checksum.wrapping_add(exercise_accessors(&result));
                }
            }
            black_box(checksum)
        })
    });
    group.finish();
}

criterion_group!(benches, bench_pipeline);
criterion_main!(benches);
