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
use std::fs;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::{Duration, Instant};

use sudachi::analysis::stateful_tokenizer::StatefulTokenizer;
use sudachi::analysis::Mode;
use sudachi::config::Config;
use sudachi::dic::dictionary::JapaneseDictionary;
use sudachi::dic::subset::InfoSubset;
use sudachi::prelude::{Morpheme, MorphemeList, SudachiResult};
use sudachi::sentence_splitter::{SentenceSplitter, SplitSentences};

const DEFAULT_CORPUS: &[&str] = &[
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

const OOV_ASCII_CORPUS: &[&str] = &[
    "OpenAI GPT-5 benchmark run",
    "RustPythonJavaTokenizer",
    "SUDACHI_RS_164_FAST_PATH",
    "abcXYZ1234567890",
    "HTTP2QUICWebTransport",
    "x86_64-apple-darwin-release",
];

const OOV_MIXED_CORPUS: &[&str] = &[
    "東京OpenAIベンチ2026",
    "Rust版SudachiPy互換チェック",
    "高速化PR#164の回帰確認",
    "GPUクラスタA100-80GB検証",
    "Wikipediaタイトル100kサンプル",
    "形態素AnalyzerV2プロトタイプ",
];

const OOV_SYMBOL_CORPUS: &[&str] = &[
    "⚙️Sudachi.rs🚀速度検証",
    "αβγΔΕΖ mixed symbols",
    "価格は¥12,345.67です",
    "C++/Rust/Python比較",
    "A/Bテストとp<0.05",
    "メールfoo.bar+baz@example.com",
];

const LONG_KATAKANA_CORPUS: &[&str] = &[
    "コンピューターアーキテクチャーパフォーマンスエンジニアリング",
    "インターナショナライゼーションローカライゼーション",
    "ニューラルネットワークアクセラレーターランタイム",
    "ソフトウェアデファインドネットワーキングプラットフォーム",
];

const REWRITE_NUMERIC_CORPUS: &[&str] = &[
    "123万4567円",
    "二〇二六年五月十二日",
    "1,234,567.890",
    "第123456789号",
    "三千二百四十五億六千七百八十九万",
    "3.1415926535は円周率です",
];

const REWRITE_NORMALIZED_CORPUS: &[&str] = &[
    "ｶﾀｶﾅとカタカナ",
    "ＡＢＣ１２３とABC123",
    "㍻から令和へ",
    "ヴァイオリンとバイオリン",
    "髙島屋と高島屋",
    "ローマ字ﾛｰﾏ字Ｒｏｍａ",
];

const CANDIDATE_HEAVY_CORPUS: &[&str] = &[
    "東京都市大学",
    "日本語形態素解析",
    "国際連合安全保障理事会",
    "情報処理推進機構",
    "日本経済新聞電子版",
    "東京大学大学院情報理工学系研究科",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CorpusPreset {
    Base,
    OovAscii,
    OovMixed,
    OovSymbol,
    LongKatakana,
    RewriteNumeric,
    RewriteNormalized,
    CandidateHeavy,
}

impl CorpusPreset {
    fn lines(self) -> &'static [&'static str] {
        match self {
            CorpusPreset::Base => DEFAULT_CORPUS,
            CorpusPreset::OovAscii => OOV_ASCII_CORPUS,
            CorpusPreset::OovMixed => OOV_MIXED_CORPUS,
            CorpusPreset::OovSymbol => OOV_SYMBOL_CORPUS,
            CorpusPreset::LongKatakana => LONG_KATAKANA_CORPUS,
            CorpusPreset::RewriteNumeric => REWRITE_NUMERIC_CORPUS,
            CorpusPreset::RewriteNormalized => REWRITE_NORMALIZED_CORPUS,
            CorpusPreset::CandidateHeavy => CANDIDATE_HEAVY_CORPUS,
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "base" => Ok(Self::Base),
            "oov-ascii" => Ok(Self::OovAscii),
            "oov-mixed" => Ok(Self::OovMixed),
            "oov-symbol" | "symbols" => Ok(Self::OovSymbol),
            "long-katakana" | "katakana" => Ok(Self::LongKatakana),
            "rewrite-numeric" | "numeric" => Ok(Self::RewriteNumeric),
            "rewrite-normalized" | "normalized-corpus" => Ok(Self::RewriteNormalized),
            "candidate-heavy" | "candidates" => Ok(Self::CandidateHeavy),
            _ => Err(format!("unknown --corpus value: {value}")),
        }
    }

    fn label(self) -> &'static str {
        match self {
            CorpusPreset::Base => "base",
            CorpusPreset::OovAscii => "oov-ascii",
            CorpusPreset::OovMixed => "oov-mixed",
            CorpusPreset::OovSymbol => "oov-symbol",
            CorpusPreset::LongKatakana => "long-katakana",
            CorpusPreset::RewriteNumeric => "rewrite-numeric",
            CorpusPreset::RewriteNormalized => "rewrite-normalized",
            CorpusPreset::CandidateHeavy => "candidate-heavy",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AccessorPlan {
    None,
    Surface,
    Pos,
    Normalized,
    Dictionary,
    Reading,
    WordInfo,
    Splits,
    All,
}

impl AccessorPlan {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "none" | "no" => Ok(Self::None),
            "surface" => Ok(Self::Surface),
            "pos" | "part-of-speech" => Ok(Self::Pos),
            "normalized" | "normalized-form" => Ok(Self::Normalized),
            "dictionary" | "dictionary-form" => Ok(Self::Dictionary),
            "reading" | "reading-form" => Ok(Self::Reading),
            "word-info" | "wordinfo" => Ok(Self::WordInfo),
            "splits" | "split" => Ok(Self::Splits),
            "all" => Ok(Self::All),
            _ => Err(format!("unknown --accessors value: {value}")),
        }
    }

    fn label(self) -> &'static str {
        match self {
            AccessorPlan::None => "none",
            AccessorPlan::Surface => "surface",
            AccessorPlan::Pos => "pos",
            AccessorPlan::Normalized => "normalized",
            AccessorPlan::Dictionary => "dictionary",
            AccessorPlan::Reading => "reading",
            AccessorPlan::WordInfo => "word-info",
            AccessorPlan::Splits => "splits",
            AccessorPlan::All => "all",
        }
    }
}

struct Args {
    config_file: PathBuf,
    resource_dir: Option<PathBuf>,
    dictionary_path: Option<PathBuf>,
    input: Option<PathBuf>,
    corpus: CorpusPreset,
    repeat: usize,
    mode: Mode,
    iterations: usize,
    split_sentences: bool,
    accessors: AccessorPlan,
    subset: InfoSubset,
    skip_errors: bool,
}

#[derive(Clone, Default)]
struct Trace {
    total: Duration,
    split: Duration,
    reset_push: Duration,
    tokenize: Duration,
    collect: Duration,
    accessors: Duration,
    lines: usize,
    sentences: usize,
    morphemes: usize,
    checksum: usize,
    errors: usize,
}

fn main() {
    let args = parse_args().unwrap_or_else(|e| {
        eprintln!("{e}");
        usage_and_exit();
    });

    let started = Instant::now();
    let config = Config::new(
        Some(args.config_file.clone()),
        args.resource_dir.clone(),
        args.dictionary_path.clone(),
    )
    .expect("failed to load config");
    let dict = JapaneseDictionary::from_cfg(&config).expect("failed to load dictionary");
    let dictionary_load = started.elapsed();

    let read_started = Instant::now();
    let corpus = load_corpus(args.input.as_deref(), args.corpus, args.repeat);
    let corpus_label = args
        .input
        .as_deref()
        .map(|path| format!("input:{}", path.display()))
        .unwrap_or_else(|| args.corpus.label().to_owned());
    let input_bytes: usize = corpus.iter().map(|line| line.len()).sum();
    let input_chars: usize = corpus.iter().map(|line| line.chars().count()).sum();
    let input_read = read_started.elapsed();

    #[cfg(feature = "profile")]
    sudachi::profiling::reset();

    let mut total_trace = Trace::default();
    for _ in 0..args.iterations {
        total_trace.add(run_pipeline(
            &dict,
            &corpus,
            args.mode,
            args.split_sentences,
            args.accessors,
            args.subset,
            args.skip_errors,
        ));
    }
    let trace = total_trace.average(args.iterations);
    #[cfg(feature = "profile")]
    let counters = sudachi::profiling::snapshot();

    println!("lines\t{}", corpus.len());
    println!("input_bytes\t{}", input_bytes);
    println!("input_chars\t{}", input_chars);
    println!("iterations\t{}", args.iterations);
    println!("corpus\t{}", corpus_label);
    println!("mode\t{}", args.mode);
    println!("split_sentences\t{}", args.split_sentences);
    println!("accessors\t{}", args.accessors.label());
    println!("subset\t{}", format_subset(args.subset));
    println!("skip_errors\t{}", args.skip_errors);
    println!("sentences_per_iter\t{}", trace.sentences);
    println!("morphemes_per_iter\t{}", trace.morphemes);
    println!("errors_per_iter\t{}", trace.errors);
    println!("checksum\t{}", trace.checksum);
    println!();
    print_duration("dictionary_load", dictionary_load, None);
    print_duration("input_read", input_read, None);
    print_duration("pipeline_total_avg", trace.total, Some(trace.total));
    print_duration("split_next_avg", trace.split, Some(trace.total));
    print_duration("reset_push_avg", trace.reset_push, Some(trace.total));
    print_duration("do_tokenize_avg", trace.tokenize, Some(trace.total));
    print_duration("collect_results_avg", trace.collect, Some(trace.total));
    print_duration("accessors_splits_avg", trace.accessors, Some(trace.total));

    #[cfg(feature = "profile")]
    print_profile_counters(counters, args.iterations);
}

fn run_pipeline(
    dict: &JapaneseDictionary,
    corpus: &[String],
    mode: Mode,
    split_sentences: bool,
    accessors: AccessorPlan,
    subset: InfoSubset,
    skip_errors: bool,
) -> Trace {
    let splitter = SentenceSplitter::new().with_checker(dict.lexicon());
    let mut analyzer = StatefulTokenizer::create(dict, false, mode);
    analyzer.set_subset(subset);
    let mut result = MorphemeList::empty(dict);
    let mut trace = Trace {
        lines: corpus.len(),
        ..Default::default()
    };

    let total_started = Instant::now();
    for line in corpus {
        if split_sentences {
            let mut iter = splitter.split(line);
            loop {
                let split_started = Instant::now();
                let next = iter.next();
                trace.split += split_started.elapsed();

                let Some((_, sentence)) = next else {
                    break;
                };
                handle_analyze_result(
                    analyze_sentence(&mut analyzer, &mut result, sentence, accessors, &mut trace),
                    skip_errors,
                    &mut trace,
                );
            }
        } else {
            handle_analyze_result(
                analyze_sentence(&mut analyzer, &mut result, line, accessors, &mut trace),
                skip_errors,
                &mut trace,
            );
        }
    }
    trace.total = total_started.elapsed();
    trace
}

fn analyze_sentence(
    analyzer: &mut StatefulTokenizer<&JapaneseDictionary>,
    result: &mut MorphemeList<&JapaneseDictionary>,
    input: &str,
    accessors: AccessorPlan,
    trace: &mut Trace,
) -> SudachiResult<()> {
    trace.sentences += 1;

    let reset_started = Instant::now();
    analyzer.reset().push_str(input);
    trace.reset_push += reset_started.elapsed();

    let tokenize_started = Instant::now();
    analyzer.do_tokenize()?;
    trace.tokenize += tokenize_started.elapsed();

    let collect_started = Instant::now();
    result.collect_results(analyzer)?;
    trace.collect += collect_started.elapsed();
    trace.morphemes += result.len();

    if accessors != AccessorPlan::None {
        let access_started = Instant::now();
        trace.checksum = trace
            .checksum
            .wrapping_add(exercise_accessors(result, accessors)?);
        trace.accessors += access_started.elapsed();
    } else {
        trace.checksum = trace.checksum.wrapping_add(result.len());
    }

    Ok(())
}

fn handle_analyze_result(result: SudachiResult<()>, skip_errors: bool, trace: &mut Trace) {
    match result {
        Ok(()) => {}
        Err(e) if skip_errors => {
            trace.errors += 1;
            eprintln!("skipping failed sentence: {e}");
        }
        Err(e) => panic!("tokenization failed: {e}"),
    }
}

fn exercise_accessors(
    result: &MorphemeList<&JapaneseDictionary>,
    accessors: AccessorPlan,
) -> SudachiResult<usize> {
    let mut checksum = 0usize;
    let mut split_a = result.empty_clone();
    let mut split_b = result.empty_clone();

    for morpheme in result.iter() {
        match accessors {
            AccessorPlan::None => {}
            AccessorPlan::Surface => {
                checksum = checksum.wrapping_add(morpheme.surface().len());
            }
            AccessorPlan::Pos => {
                checksum = checksum.wrapping_add(morpheme.part_of_speech().len());
            }
            AccessorPlan::Normalized => {
                checksum = checksum.wrapping_add(morpheme.normalized_form().len());
            }
            AccessorPlan::Dictionary => {
                checksum = checksum.wrapping_add(morpheme.dictionary_form().len());
            }
            AccessorPlan::Reading => {
                checksum = checksum.wrapping_add(morpheme.reading_form().len());
            }
            AccessorPlan::WordInfo => {
                let word_info = morpheme.get_word_info();
                checksum = checksum.wrapping_add(word_info.surface().len());
                checksum = checksum.wrapping_add(word_info.normalized_form().len());
                checksum = checksum.wrapping_add(word_info.dictionary_form().len());
                checksum = checksum.wrapping_add(word_info.reading_form().len());
                checksum = checksum.wrapping_add(word_info.a_unit_split().len());
                checksum = checksum.wrapping_add(word_info.b_unit_split().len());
                checksum = checksum.wrapping_add(word_info.word_structure().len());
                checksum = checksum.wrapping_add(word_info.synonym_group_ids().len());
            }
            AccessorPlan::Splits => {
                checksum =
                    checksum.wrapping_add(exercise_splits(&morpheme, &mut split_a, &mut split_b)?);
            }
            AccessorPlan::All => {
                checksum = checksum.wrapping_add(morpheme.surface().len());
                checksum = checksum.wrapping_add(morpheme.part_of_speech().len());
                checksum = checksum.wrapping_add(morpheme.dictionary_form().len());
                checksum = checksum.wrapping_add(morpheme.normalized_form().len());
                checksum =
                    checksum.wrapping_add(exercise_splits(&morpheme, &mut split_a, &mut split_b)?);
            }
        }
    }

    Ok(black_box(checksum))
}

fn exercise_splits<'a>(
    morpheme: &Morpheme<'_, &'a JapaneseDictionary>,
    split_a: &mut MorphemeList<&'a JapaneseDictionary>,
    split_b: &mut MorphemeList<&'a JapaneseDictionary>,
) -> SudachiResult<usize> {
    let mut checksum = 0usize;
    split_a.clear();
    if morpheme.split_into(Mode::A, split_a)? {
        checksum = checksum.wrapping_add(split_a.len());
    }

    split_b.clear();
    if morpheme.split_into(Mode::B, split_b)? {
        checksum = checksum.wrapping_add(split_b.len());
    }
    Ok(checksum)
}

fn load_corpus(input: Option<&Path>, corpus: CorpusPreset, repeat: usize) -> Vec<String> {
    match input {
        Some(path) => fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("failed to read input {}: {e}", path.display()))
            .lines()
            .map(|line| line.strip_suffix('\r').unwrap_or(line).to_owned())
            .collect(),
        None => {
            let lines = corpus.lines();
            let mut corpus = Vec::with_capacity(lines.len() * repeat);
            for _ in 0..repeat {
                corpus.extend(lines.iter().map(|line| (*line).to_owned()));
            }
            corpus
        }
    }
}

impl Trace {
    fn add(&mut self, other: Trace) {
        self.total += other.total;
        self.split += other.split;
        self.reset_push += other.reset_push;
        self.tokenize += other.tokenize;
        self.collect += other.collect;
        self.accessors += other.accessors;
        self.lines += other.lines;
        self.sentences += other.sentences;
        self.morphemes += other.morphemes;
        self.checksum = self.checksum.wrapping_add(other.checksum);
        self.errors += other.errors;
    }

    fn average(mut self, iterations: usize) -> Trace {
        let n = iterations as f64;
        self.total = div_duration(self.total, n);
        self.split = div_duration(self.split, n);
        self.reset_push = div_duration(self.reset_push, n);
        self.tokenize = div_duration(self.tokenize, n);
        self.collect = div_duration(self.collect, n);
        self.accessors = div_duration(self.accessors, n);
        self.lines /= iterations;
        self.sentences /= iterations;
        self.morphemes /= iterations;
        self.errors /= iterations;
        self
    }
}

fn div_duration(duration: Duration, divisor: f64) -> Duration {
    Duration::from_secs_f64(duration.as_secs_f64() / divisor)
}

fn print_duration(label: &str, duration: Duration, total: Option<Duration>) {
    match total {
        Some(total) if total.as_nanos() > 0 => {
            let pct = duration.as_secs_f64() / total.as_secs_f64() * 100.0;
            println!(
                "{label}\t{:.3} ms\t{pct:.1}%",
                duration.as_secs_f64() * 1000.0
            );
        }
        _ => println!("{label}\t{:.3} ms", duration.as_secs_f64() * 1000.0),
    }
}

#[cfg(feature = "profile")]
fn print_profile_counters(counters: sudachi::profiling::Counters, iterations: usize) {
    let iterations = iterations as u64;
    let avg = |value: u64| value / iterations;
    let word_info = counters.word_info;
    let lattice = counters.lattice;
    let oov = counters.oov;
    let type_sizes = counters.type_sizes;

    println!();
    println!("profile_counters\ttrue");
    println!("type_size_node\t{}", type_sizes.node);
    println!("type_size_vnode\t{}", type_sizes.vnode);
    println!("type_size_best_prev\t{}", type_sizes.best_prev);
    println!("type_size_created_words\t{}", type_sizes.created_words);
    println!("word_info_requests\t{}", avg(word_info.word_info_requests));
    println!(
        "word_info_cache_hits\t{}",
        avg(word_info.word_info_cache_hits)
    );
    println!("word_info_decodes\t{}", avg(word_info.word_info_decodes));
    println!("pos_decodes\t{}", avg(word_info.pos_decodes));
    println!(
        "normalized_form_decodes\t{}",
        avg(word_info.normalized_form_decodes)
    );
    println!(
        "dictionary_form_decodes\t{}",
        avg(word_info.dictionary_form_decodes)
    );
    println!(
        "reading_form_decodes\t{}",
        avg(word_info.reading_form_decodes)
    );
    println!("split_a_decodes\t{}", avg(word_info.split_a_decodes));
    println!("split_b_decodes\t{}", avg(word_info.split_b_decodes));
    println!("split_c_decodes\t{}", avg(word_info.split_c_decodes));
    println!(
        "owned_string_allocations\t{}",
        avg(word_info.owned_string_allocations)
    );
    println!("vec_allocations\t{}", avg(word_info.vec_allocations));

    println!("lattice_inserted_nodes\t{}", avg(lattice.inserted_nodes));
    println!("lattice_bos_nodes\t{}", avg(lattice.bos_nodes));
    println!("lattice_eos_nodes\t{}", avg(lattice.eos_nodes));
    println!("lattice_reset_calls\t{}", avg(lattice.reset_calls));
    println!(
        "lattice_reset_vecs_visited\t{}",
        avg(lattice.reset_vecs_visited)
    );
    println!(
        "lattice_reset_items_cleared\t{}",
        avg(lattice.reset_items_cleared)
    );
    println!(
        "lattice_reset_new_boundaries\t{}",
        avg(lattice.reset_new_boundaries)
    );
    println!(
        "lattice_boundaries_touched\t{}",
        avg(lattice.boundaries_touched)
    );
    println!(
        "lattice_boundaries_empty\t{}",
        avg(lattice.boundaries_empty)
    );
    println!(
        "lattice_max_nodes_per_boundary\t{}",
        lattice.max_nodes_per_boundary
    );
    println!(
        "lattice_total_nodes_per_boundary\t{}",
        avg(lattice.total_nodes_per_boundary)
    );
    println!(
        "lattice_boundary_width_1\t{}",
        avg(lattice.boundary_width_1)
    );
    println!(
        "lattice_boundary_width_2_4\t{}",
        avg(lattice.boundary_width_2_4)
    );
    println!(
        "lattice_boundary_width_5_8\t{}",
        avg(lattice.boundary_width_5_8)
    );
    println!(
        "lattice_boundary_width_9_16\t{}",
        avg(lattice.boundary_width_9_16)
    );
    println!(
        "lattice_boundary_width_17_32\t{}",
        avg(lattice.boundary_width_17_32)
    );
    println!(
        "lattice_boundary_width_33_64\t{}",
        avg(lattice.boundary_width_33_64)
    );
    println!(
        "lattice_boundary_width_65_128\t{}",
        avg(lattice.boundary_width_65_128)
    );
    println!(
        "lattice_boundary_width_129_plus\t{}",
        avg(lattice.boundary_width_129_plus)
    );
    println!(
        "lattice_left_connection_checks\t{}",
        avg(lattice.left_connection_checks)
    );
    println!(
        "lattice_right_connection_checks\t{}",
        avg(lattice.right_connection_checks)
    );
    println!("lattice_cost_updates\t{}", avg(lattice.cost_updates));
    println!(
        "lattice_node_vec_growths\t{}",
        avg(lattice.node_vec_growths)
    );
    println!(
        "lattice_edge_vec_growths\t{}",
        avg(lattice.edge_vec_growths)
    );
    println!(
        "lattice_rejected_candidates\t{}",
        avg(lattice.rejected_candidates)
    );
    println!("lattice_best_prev_calls\t{}", avg(lattice.best_prev_calls));
    println!(
        "lattice_best_prev_fast_empty\t{}",
        avg(lattice.best_prev_fast_empty)
    );
    println!(
        "lattice_best_prev_fast_single\t{}",
        avg(lattice.best_prev_fast_single)
    );
    println!(
        "lattice_best_prev_direct_small\t{}",
        avg(lattice.best_prev_direct_small)
    );
    println!("lattice_prev_nodes_0\t{}", avg(lattice.prev_nodes_0));
    println!("lattice_prev_nodes_1\t{}", avg(lattice.prev_nodes_1));
    println!("lattice_prev_nodes_2_4\t{}", avg(lattice.prev_nodes_2_4));
    println!("lattice_prev_nodes_5_16\t{}", avg(lattice.prev_nodes_5_16));
    println!(
        "lattice_prev_nodes_17_plus\t{}",
        avg(lattice.prev_nodes_17_plus)
    );
    println!(
        "lattice_left_id_cache_probes\t{}",
        avg(lattice.left_id_cache_probes)
    );
    println!(
        "lattice_left_id_cache_hits\t{}",
        avg(lattice.left_id_cache_hits)
    );
    println!(
        "lattice_left_id_cache_misses\t{}",
        avg(lattice.left_id_cache_misses)
    );
    println!(
        "lattice_left_id_cache_saved_checks\t{}",
        avg(lattice.left_id_cache_saved_checks)
    );
    println!(
        "lattice_left_id_cache_miss_checks\t{}",
        avg(lattice.left_id_cache_miss_checks)
    );
    println!(
        "lattice_left_id_boundaries\t{}",
        avg(lattice.left_id_boundaries)
    );
    println!(
        "lattice_left_id_total_unique\t{}",
        avg(lattice.left_id_total_unique)
    );
    println!(
        "lattice_left_id_max_unique_per_boundary\t{}",
        lattice.left_id_max_unique_per_boundary
    );
    println!(
        "lattice_best_prev_cache_pushes\t{}",
        avg(lattice.best_prev_cache_pushes)
    );
    println!(
        "lattice_best_prev_cache_capacity_total\t{}",
        avg(lattice.best_prev_cache_capacity_total)
    );
    println!(
        "lattice_ends_capacity_total\t{}",
        avg(lattice.ends_capacity_total)
    );
    println!(
        "lattice_ends_full_capacity_total\t{}",
        avg(lattice.ends_full_capacity_total)
    );
    println!(
        "lattice_indices_capacity_total\t{}",
        avg(lattice.indices_capacity_total)
    );
    println!("lattice_positions_total\t{}", avg(lattice.positions_total));
    println!(
        "lattice_positions_reachable\t{}",
        avg(lattice.positions_reachable)
    );
    println!(
        "lattice_positions_unreachable_skipped\t{}",
        avg(lattice.positions_unreachable_skipped)
    );
    println!(
        "lattice_direct_inserted_candidates\t{}",
        avg(lattice.direct_inserted_candidates)
    );

    println!("oov_provider_calls\t{}", avg(oov.provider_calls));
    println!("oov_candidates_provided\t{}", avg(oov.candidates_provided));
    println!("oov_inserted_nodes\t{}", avg(oov.inserted_nodes));
    println!(
        "oov_duplicate_candidates\t{}",
        avg(oov.duplicate_candidates)
    );
    println!("oov_best_path_nodes\t{}", avg(oov.best_path_nodes));
    println!(
        "oov_mecab_provider_calls\t{}",
        avg(oov.mecab_provider_calls)
    );
    println!(
        "oov_simple_provider_calls\t{}",
        avg(oov.simple_provider_calls)
    );
    println!(
        "oov_regex_provider_calls\t{}",
        avg(oov.regex_provider_calls)
    );
    println!(
        "oov_other_provider_calls\t{}",
        avg(oov.other_provider_calls)
    );
    println!("oov_mecab_candidates\t{}", avg(oov.mecab_candidates));
    println!("oov_simple_candidates\t{}", avg(oov.simple_candidates));
    println!("oov_regex_candidates\t{}", avg(oov.regex_candidates));
    println!("oov_other_candidates\t{}", avg(oov.other_candidates));
    println!("oov_single_candidates\t{}", avg(oov.single_candidates));
    println!("oov_grouped_candidates\t{}", avg(oov.grouped_candidates));
    println!("oov_length_1\t{}", avg(oov.length_1));
    println!("oov_length_2\t{}", avg(oov.length_2));
    println!("oov_length_3\t{}", avg(oov.length_3));
    println!("oov_length_4\t{}", avg(oov.length_4));
    println!("oov_length_5_8\t{}", avg(oov.length_5_8));
    println!("oov_length_9_16\t{}", avg(oov.length_9_16));
    println!("oov_length_17_32\t{}", avg(oov.length_17_32));
    println!("oov_length_33_plus\t{}", avg(oov.length_33_plus));
    println!("oov_category_default\t{}", avg(oov.category_default));
    println!("oov_category_space\t{}", avg(oov.category_space));
    println!("oov_category_kanji\t{}", avg(oov.category_kanji));
    println!("oov_category_symbol\t{}", avg(oov.category_symbol));
    println!("oov_category_numeric\t{}", avg(oov.category_numeric));
    println!("oov_category_alpha\t{}", avg(oov.category_alpha));
    println!("oov_category_hiragana\t{}", avg(oov.category_hiragana));
    println!("oov_category_katakana\t{}", avg(oov.category_katakana));
    println!(
        "oov_category_kanjinumeric\t{}",
        avg(oov.category_kanjinumeric)
    );
    println!("oov_category_greek\t{}", avg(oov.category_greek));
    println!("oov_category_cyrillic\t{}", avg(oov.category_cyrillic));
    println!("oov_category_user1\t{}", avg(oov.category_user1));
    println!("oov_category_user2\t{}", avg(oov.category_user2));
    println!("oov_category_user3\t{}", avg(oov.category_user3));
    println!("oov_category_user4\t{}", avg(oov.category_user4));
    println!("oov_category_other\t{}", avg(oov.category_other));
    println!("oov_ranges_total\t{}", avg(oov.ranges_total));
    println!(
        "oov_range_candidates_total\t{}",
        avg(oov.range_candidates_total)
    );
    println!("oov_range_max_candidates\t{}", oov.range_max_candidates);
    println!("oov_range_0\t{}", avg(oov.range_0));
    println!("oov_range_1\t{}", avg(oov.range_1));
    println!("oov_range_2_4\t{}", avg(oov.range_2_4));
    println!("oov_range_5_8\t{}", avg(oov.range_5_8));
    println!("oov_range_9_16\t{}", avg(oov.range_9_16));
    println!("oov_range_17_23\t{}", avg(oov.range_17_23));
    println!("oov_range_24_plus\t{}", avg(oov.range_24_plus));
    println!(
        "oov_suppressed_by_has_other_words\t{}",
        avg(oov.suppressed_by_has_other_words)
    );
    println!(
        "oov_suppressed_by_invoke_false\t{}",
        avg(oov.suppressed_by_invoke_false)
    );
    println!(
        "oov_suppressed_group_by_group_false\t{}",
        avg(oov.suppressed_group_by_group_false)
    );
    println!(
        "oov_buffered_provider_calls\t{}",
        avg(oov.buffered_provider_calls)
    );
    println!("oov_buffered_candidates\t{}", avg(oov.buffered_candidates));
    println!("oov_temp_buffer_growths\t{}", avg(oov.temp_buffer_growths));
    println!("oov_temp_buffer_max_len\t{}", oov.temp_buffer_max_len);
    println!("oov_dominance_groups\t{}", avg(oov.dominance_groups));
    println!(
        "oov_strict_dominated_candidates\t{}",
        avg(oov.strict_dominated_candidates)
    );
    println!("oov_equal_cost_ties\t{}", avg(oov.equal_cost_ties));
    println!(
        "oov_unique_after_dominance\t{}",
        avg(oov.unique_after_dominance)
    );
    println!("oov_dominated_length_1\t{}", avg(oov.dominated_length_1));
    println!("oov_dominated_length_2\t{}", avg(oov.dominated_length_2));
    println!("oov_dominated_length_3\t{}", avg(oov.dominated_length_3));
    println!("oov_dominated_length_4\t{}", avg(oov.dominated_length_4));
    println!(
        "oov_dominated_length_5_8\t{}",
        avg(oov.dominated_length_5_8)
    );
    println!(
        "oov_dominated_length_9_16\t{}",
        avg(oov.dominated_length_9_16)
    );
    println!(
        "oov_dominated_length_17_32\t{}",
        avg(oov.dominated_length_17_32)
    );
    println!(
        "oov_dominated_length_33_plus\t{}",
        avg(oov.dominated_length_33_plus)
    );
    println!(
        "oov_dominated_category_default\t{}",
        avg(oov.dominated_category_default)
    );
    println!(
        "oov_dominated_category_space\t{}",
        avg(oov.dominated_category_space)
    );
    println!(
        "oov_dominated_category_kanji\t{}",
        avg(oov.dominated_category_kanji)
    );
    println!(
        "oov_dominated_category_symbol\t{}",
        avg(oov.dominated_category_symbol)
    );
    println!(
        "oov_dominated_category_numeric\t{}",
        avg(oov.dominated_category_numeric)
    );
    println!(
        "oov_dominated_category_alpha\t{}",
        avg(oov.dominated_category_alpha)
    );
    println!(
        "oov_dominated_category_hiragana\t{}",
        avg(oov.dominated_category_hiragana)
    );
    println!(
        "oov_dominated_category_katakana\t{}",
        avg(oov.dominated_category_katakana)
    );
    println!(
        "oov_dominated_category_kanjinumeric\t{}",
        avg(oov.dominated_category_kanjinumeric)
    );
    println!(
        "oov_dominated_category_greek\t{}",
        avg(oov.dominated_category_greek)
    );
    println!(
        "oov_dominated_category_cyrillic\t{}",
        avg(oov.dominated_category_cyrillic)
    );
    println!(
        "oov_dominated_category_user1\t{}",
        avg(oov.dominated_category_user1)
    );
    println!(
        "oov_dominated_category_user2\t{}",
        avg(oov.dominated_category_user2)
    );
    println!(
        "oov_dominated_category_user3\t{}",
        avg(oov.dominated_category_user3)
    );
    println!(
        "oov_dominated_category_user4\t{}",
        avg(oov.dominated_category_user4)
    );
    println!(
        "oov_dominated_category_other\t{}",
        avg(oov.dominated_category_other)
    );
}

fn parse_args() -> Result<Args, String> {
    let root = workspace_root();
    let mut args = Args {
        config_file: root.join("resources/sudachi.json"),
        resource_dir: Some(root.join("resources")),
        dictionary_path: None,
        input: None,
        corpus: CorpusPreset::Base,
        repeat: 1000,
        mode: Mode::C,
        iterations: 1,
        split_sentences: true,
        accessors: AccessorPlan::All,
        subset: InfoSubset::all(),
        skip_errors: false,
    };

    let mut iter = env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--config" | "--config-file" => args.config_file = value_path(&mut iter, &arg)?,
            "--resource-dir" => args.resource_dir = Some(value_path(&mut iter, &arg)?),
            "--dict" => args.dictionary_path = Some(value_path(&mut iter, &arg)?),
            "--input" => args.input = Some(value_path(&mut iter, &arg)?),
            "--corpus" => {
                let value = value_string(&mut iter, &arg)?;
                args.corpus = CorpusPreset::parse(&value)?;
            }
            "--repeat" => {
                let value = value_string(&mut iter, &arg)?;
                args.repeat = value
                    .parse::<usize>()
                    .map_err(|_| format!("invalid --repeat value: {value}"))?;
                if args.repeat == 0 {
                    return Err("--repeat must be greater than zero".to_owned());
                }
            }
            "--mode" => {
                let value = value_string(&mut iter, &arg)?;
                args.mode = Mode::from_str(&value).map_err(|e| e.to_owned())?;
            }
            "--iterations" => {
                let value = value_string(&mut iter, &arg)?;
                args.iterations = value
                    .parse::<usize>()
                    .map_err(|_| format!("invalid --iterations value: {value}"))?;
                if args.iterations == 0 {
                    return Err("--iterations must be greater than zero".to_owned());
                }
            }
            "--split-sentences" => {
                let value = value_string(&mut iter, &arg)?;
                args.split_sentences = match value.as_str() {
                    "yes" | "true" | "default" => true,
                    "no" | "false" | "none" => false,
                    _ => return Err("--split-sentences must be yes or no".to_owned()),
                };
            }
            "--accessors" => {
                let value = value_string(&mut iter, &arg)?;
                args.accessors = AccessorPlan::parse(&value)?;
            }
            "--no-accessors" => args.accessors = AccessorPlan::None,
            "--subset" => {
                let value = value_string(&mut iter, &arg)?;
                args.subset = parse_subset(&value)?;
            }
            "--skip-errors" => args.skip_errors = true,
            "--help" | "-h" => usage_and_exit(),
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }

    Ok(args)
}

fn parse_subset(value: &str) -> Result<InfoSubset, String> {
    let mut subset = InfoSubset::empty();
    for part in value.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        match part {
            "all" => return Ok(InfoSubset::all()),
            "none" | "empty" | "core" => {}
            "surface" => subset |= InfoSubset::SURFACE,
            "head" | "head-word-length" => subset |= InfoSubset::HEAD_WORD_LENGTH,
            "pos" | "pos-id" => subset |= InfoSubset::POS_ID,
            "normalized" | "normalized-form" => subset |= InfoSubset::NORMALIZED_FORM,
            "dictionary" | "dictionary-form" => {
                subset |= InfoSubset::DIC_FORM_WORD_ID | InfoSubset::SURFACE;
            }
            "reading" | "reading-form" => subset |= InfoSubset::READING_FORM,
            "split-a" => subset |= InfoSubset::SPLIT_A,
            "split-b" => subset |= InfoSubset::SPLIT_B,
            "splits" => subset |= InfoSubset::SPLIT_A | InfoSubset::SPLIT_B,
            "word-structure" => subset |= InfoSubset::WORD_STRUCTURE,
            "synonyms" | "synonym-group-ids" => subset |= InfoSubset::SYNONYM_GROUP_ID,
            "rewrite-min" => {
                subset |= InfoSubset::SURFACE
                    | InfoSubset::HEAD_WORD_LENGTH
                    | InfoSubset::POS_ID
                    | InfoSubset::NORMALIZED_FORM
                    | InfoSubset::DIC_FORM_WORD_ID
                    | InfoSubset::READING_FORM;
            }
            _ => return Err(format!("unknown --subset component: {part}")),
        }
    }
    Ok(subset.normalize())
}

fn format_subset(subset: InfoSubset) -> String {
    if subset == InfoSubset::all() {
        return "all".to_owned();
    }
    if subset.is_empty() {
        return "none".to_owned();
    }

    let mut parts = Vec::new();
    if subset.contains(InfoSubset::SURFACE) {
        parts.push("surface");
    }
    if subset.contains(InfoSubset::HEAD_WORD_LENGTH) {
        parts.push("head");
    }
    if subset.contains(InfoSubset::POS_ID) {
        parts.push("pos");
    }
    if subset.contains(InfoSubset::NORMALIZED_FORM) {
        parts.push("normalized");
    }
    if subset.contains(InfoSubset::DIC_FORM_WORD_ID) {
        parts.push("dictionary");
    }
    if subset.contains(InfoSubset::READING_FORM) {
        parts.push("reading");
    }
    if subset.contains(InfoSubset::SPLIT_A) {
        parts.push("split-a");
    }
    if subset.contains(InfoSubset::SPLIT_B) {
        parts.push("split-b");
    }
    if subset.contains(InfoSubset::WORD_STRUCTURE) {
        parts.push("word-structure");
    }
    if subset.contains(InfoSubset::SYNONYM_GROUP_ID) {
        parts.push("synonyms");
    }
    parts.join(",")
}

fn value_path(iter: &mut impl Iterator<Item = String>, arg: &str) -> Result<PathBuf, String> {
    value_string(iter, arg).map(PathBuf::from)
}

fn value_string(iter: &mut impl Iterator<Item = String>, arg: &str) -> Result<String, String> {
    iter.next().ok_or_else(|| format!("{arg} requires a value"))
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("sudachi crate must be inside workspace")
        .to_path_buf()
}

fn usage_and_exit() -> ! {
    eprintln!(
        "Usage: cargo run -p sudachi --release --example profile_pipeline -- \\
         [--config PATH] [--resource-dir PATH] [--dict PATH] [--input PATH] \\
         [--corpus NAME] [--repeat N] [--mode A|B|C] [--iterations N] \\
         [--split-sentences yes|no] [--accessors none|surface|pos|normalized|dictionary|reading|word-info|splits|all] \\
         [--subset all|none|rewrite-min|FIELD[,FIELD...]] [--skip-errors]"
    );
    std::process::exit(2);
}
