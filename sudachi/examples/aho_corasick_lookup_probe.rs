/*
 * Copyright (c) 2026 Works Applications Co., Ltd.
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

//! Research probe for a radical dictionary-lookup alternative: build a
//! single-pass Aho-Corasick automaton over dictionary keys and scan each
//! sentence once. By default this uses public headwords; set
//! `SUDACHI_BENCH_LEXICONS` to a comma-separated list of raw dictionary CSVs
//! to benchmark real `index_form` keys instead.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use aho_corasick::{AhoCorasick, AhoCorasickBuilder, MatchKind};
use daachorse::{CharwiseDoubleArrayAhoCorasick, DoubleArrayAhoCorasick};
use sudachi::config::Config;
use sudachi::dic::dictionary::JapaneseDictionary;
use sudachi::dic::subset::InfoSubset;

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

fn load_dict() -> (PathBuf, Arc<JapaneseDictionary>) {
    let config_path = env_path("SUDACHI_BENCH_CONFIG", "resources/sudachi.json");
    let resource_dir = config_path
        .parent()
        .map(|p| p.to_path_buf())
        .filter(|p| !p.as_os_str().is_empty());
    let dict_override = std::env::var_os("SUDACHI_BENCH_DICT").map(PathBuf::from);
    let config = Config::new(Some(config_path.clone()), resource_dir, dict_override)
        .expect("failed to load config");
    let dict = JapaneseDictionary::from_cfg(&config).expect("failed to load dictionary");
    (config_path, Arc::new(dict))
}

fn collect_headwords(dict: &JapaneseDictionary) -> Vec<String> {
    let mut seen = HashSet::<String>::new();
    let mut patterns = Vec::new();
    for wid in dict.lexicon().word_ids() {
        let wid = wid.expect("invalid word id while collecting headwords");
        let info = dict
            .lexicon()
            .get_word_info_subset(wid, InfoSubset::HEADWORD)
            .expect("failed to get word info");
        let headword = info.headword(dict.lexicon()).to_owned();
        if !headword.is_empty() && seen.insert(headword.clone()) {
            patterns.push(headword);
        }
    }
    patterns
}

fn collect_index_forms(paths: &[PathBuf]) -> Vec<String> {
    let mut seen = HashSet::<String>::new();
    let mut patterns = Vec::new();
    for path in paths {
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(true)
            .from_path(path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
        for record in reader.records() {
            let record = record.unwrap_or_else(|err| {
                panic!("failed to parse {}: {err}", path.display());
            });
            let Some(index_form) = record.get(0) else {
                continue;
            };
            if !index_form.is_empty() && seen.insert(index_form.to_owned()) {
                patterns.push(index_form.to_owned());
            }
        }
    }
    patterns
}

fn env_paths(key: &str) -> Option<Vec<PathBuf>> {
    std::env::var_os(key).map(|value| {
        value
            .to_string_lossy()
            .split(',')
            .filter(|part| !part.is_empty())
            .map(PathBuf::from)
            .collect()
    })
}

fn build_ac(patterns: &[String]) -> AhoCorasick {
    AhoCorasickBuilder::new()
        .match_kind(MatchKind::Standard)
        .build(patterns)
        .expect("failed to build Aho-Corasick automaton")
}

fn run_trials<F>(trials: usize, mut count_matches: F) -> (f64, f64, f64, f64, usize)
where
    F: FnMut() -> usize,
{
    let mut times = Vec::with_capacity(trials);
    let mut total_matches = 0usize;
    for _ in 0..trials {
        let start = Instant::now();
        let matches = count_matches();
        times.push(start.elapsed().as_secs_f64() * 1e3);
        total_matches = matches;
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let min = times[0];
    let median = times[times.len() / 2];
    let mean = times.iter().sum::<f64>() / times.len() as f64;
    let var = times.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / times.len() as f64;
    let cv = var.sqrt() / mean * 100.0;
    (min, median, mean, cv, total_matches)
}

fn main() {
    let (config_path, dict) = load_dict();
    let inputs_path = env_path(
        "SUDACHI_BENCH_INPUTS",
        "target/issue-117-corpora/kyoto-leads.txt",
    );
    let limit = env_usize("SUDACHI_BENCH_LIMIT", usize::MAX);
    let trials = env_usize("SUDACHI_BENCH_TRIALS", 10);

    let text = std::fs::read_to_string(&inputs_path).expect("failed to read inputs");
    let lines: Vec<&str> = text.lines().take(limit).collect();
    let chars: usize = lines.iter().map(|line| line.chars().count()).sum();
    let bytes: usize = lines.iter().map(|line| line.len()).sum();

    let build_start = Instant::now();
    let lexicon_csvs = env_paths("SUDACHI_BENCH_LEXICONS");
    let patterns = match &lexicon_csvs {
        Some(paths) => collect_index_forms(paths),
        None => collect_headwords(&dict),
    };
    let collect_ms = build_start.elapsed().as_secs_f64() * 1e3;

    let ac_start = Instant::now();
    let ac = build_ac(&patterns);
    let build_ms = ac_start.elapsed().as_secs_f64() * 1e3;

    let daac_start = Instant::now();
    let daac: DoubleArrayAhoCorasick<u32> =
        DoubleArrayAhoCorasick::new(&patterns).expect("failed to build daachorse automaton");
    let daac_build_ms = daac_start.elapsed().as_secs_f64() * 1e3;

    let char_daac_start = Instant::now();
    let char_daac: CharwiseDoubleArrayAhoCorasick<u32> =
        CharwiseDoubleArrayAhoCorasick::new(&patterns)
            .expect("failed to build charwise daachorse automaton");
    let char_daac_build_ms = char_daac_start.elapsed().as_secs_f64() * 1e3;

    let warm_ac_matches: usize = lines
        .iter()
        .map(|line| ac.find_overlapping_iter(line.as_bytes()).count())
        .sum();
    let warm_daac_matches: usize = lines
        .iter()
        .map(|line| daac.find_overlapping_iter(line.as_bytes()).count())
        .sum();
    let warm_char_daac_matches: usize = lines
        .iter()
        .map(|line| char_daac.find_overlapping_iter(*line).count())
        .sum();

    let (ac_min, ac_median, ac_mean, ac_cv, ac_matches) = run_trials(trials, || {
        lines
            .iter()
            .map(|line| ac.find_overlapping_iter(line.as_bytes()).count())
            .sum()
    });
    let (daac_min, daac_median, daac_mean, daac_cv, daac_matches) = run_trials(trials, || {
        lines
            .iter()
            .map(|line| daac.find_overlapping_iter(line.as_bytes()).count())
            .sum()
    });
    let (char_daac_min, char_daac_median, char_daac_mean, char_daac_cv, char_daac_matches) =
        run_trials(trials, || {
            lines
                .iter()
                .map(|line| char_daac.find_overlapping_iter(*line).count())
                .sum()
        });

    println!("# config: {}", config_path.display());
    println!("# inputs: {}", inputs_path.display());
    match &lexicon_csvs {
        Some(paths) => println!(
            "# patterns_source: index_form csvs={}",
            paths
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(",")
        ),
        None => println!("# patterns_source: public headwords"),
    }
    println!(
        "# patterns={} lines={} chars={} bytes={} warm_ac_matches={} warm_daac_matches={} warm_char_daac_matches={}",
        patterns.len(),
        lines.len(),
        chars,
        bytes,
        warm_ac_matches,
        warm_daac_matches,
        warm_char_daac_matches
    );
    println!(
        "build_patterns     {:>8.2} ms  build_ac {:>8.2} ms  build_daac {:>8.2} ms  build_char_daac {:>8.2} ms",
        collect_ms, build_ms, daac_build_ms, char_daac_build_ms
    );
    println!(
        "ac_overlap_scan    min {:>7.2}  median {:>7.2}  mean {:>7.2} ms  cv {:>4.1}%  | {:>6.2} ns/char  matches={}",
        ac_min,
        ac_median,
        ac_mean,
        ac_cv,
        ac_median * 1e6 / chars.max(1) as f64,
        ac_matches
    );
    println!(
        "daac_overlap_scan  min {:>7.2}  median {:>7.2}  mean {:>7.2} ms  cv {:>4.1}%  | {:>6.2} ns/char  matches={}",
        daac_min,
        daac_median,
        daac_mean,
        daac_cv,
        daac_median * 1e6 / chars.max(1) as f64,
        daac_matches
    );
    println!(
        "char_daac_scan     min {:>7.2}  median {:>7.2}  mean {:>7.2} ms  cv {:>4.1}%  | {:>6.2} ns/char  matches={}",
        char_daac_min,
        char_daac_median,
        char_daac_mean,
        char_daac_cv,
        char_daac_median * 1e6 / chars.max(1) as f64,
        char_daac_matches
    );
}
