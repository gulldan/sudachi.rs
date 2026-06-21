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

//! A long-running full-pipeline tokenization loop, so an external sampler
//! (`sample`/Instruments on macOS, `perf` on Linux) has a steady hot loop to
//! attribute. Prints its PID and loops for SUDACHI_LOOP_SECS seconds.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use sudachi::analysis::mlist::MorphemeList;
use sudachi::analysis::stateful_tokenizer::StatefulTokenizer;
use sudachi::analysis::Mode;
use sudachi::config::Config;
use sudachi::dic::dictionary::JapaneseDictionary;

fn env_path(key: &str, default: &str) -> PathBuf {
    std::env::var_os(key).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(default))
}

fn main() {
    let config_path = env_path("SUDACHI_BENCH_CONFIG", "resources/sudachi.json");
    let resource_dir = config_path.parent().map(|p| p.to_path_buf()).filter(|p| !p.as_os_str().is_empty());
    let dict_override = std::env::var_os("SUDACHI_BENCH_DICT").map(PathBuf::from);
    let config = Config::new(Some(config_path), resource_dir, dict_override).expect("config");
    let dict = Arc::new(JapaneseDictionary::from_cfg(&config).expect("dict"));

    let inputs = env_path("SUDACHI_BENCH_INPUTS", "target/issue-117-corpora/kyoto-leads.txt");
    let text = std::fs::read_to_string(&inputs).expect("inputs");
    let lines: Vec<&str> = text.lines().collect();
    let secs: f64 = std::env::var("SUDACHI_LOOP_SECS").ok().and_then(|v| v.parse().ok()).unwrap_or(20.0);

    let mut tok = StatefulTokenizer::new(dict.clone(), Mode::C);
    let mut result = MorphemeList::empty(dict.clone());

    println!("PID {} — full-pipeline do_tokenize loop for {secs}s", std::process::id());
    let start = Instant::now();
    let mut sink = 0u64;
    let mut passes = 0u64;
    while start.elapsed().as_secs_f64() < secs {
        for line in &lines {
            tok.reset().push_str(line);
            tok.do_tokenize().expect("tok");
            result.collect_results(&mut tok).expect("collect");
            for i in 0..result.len() {
                let m = result.get(i);
                sink = sink.wrapping_add(m.surface().len() as u64);
                sink = sink.wrapping_add(m.normalized_form().len() as u64);
                sink = sink.wrapping_add(m.part_of_speech_id() as u64);
            }
        }
        passes += 1;
    }
    std::hint::black_box(sink);
    println!("done: {passes} passes in {:.1}s", start.elapsed().as_secs_f64());
}
