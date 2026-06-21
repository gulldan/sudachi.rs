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

//! In-process A/B of the DENSE connection matrix vs the additive + sparse-residual
//! matrix (`ConnectionMatrix::sparsify`), end-to-end through `do_tokenize`.
//! Tests eiennohito's "simplify the connection matrix" direction as a real
//! runtime structure, not a probe:
//!   - footprint: dense bytes vs sparse heap bytes at the chosen lambda;
//!   - correctness: at lambda=0 the materialized output must be byte-identical;
//!   - speed: smaller working set vs a per-access residual lookup — measured.
//!
//! SUDACHI_BENCH_CONFIG  SUDACHI_BENCH_DICT  SUDACHI_BENCH_INPUTS
//! SUDACHI_BENCH_TRIALS  SUDACHI_SPARSE_LAMBDA (default 0 = byte-identical)

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use sudachi::analysis::mlist::MorphemeList;
use sudachi::analysis::stateful_tokenizer::StatefulTokenizer;
use sudachi::analysis::Mode;
use sudachi::config::Config;
use sudachi::dic::dictionary::JapaneseDictionary;

type Tok = StatefulTokenizer<Arc<JapaneseDictionary>>;
type MList = MorphemeList<Arc<JapaneseDictionary>>;

fn load(config_path: &PathBuf) -> JapaneseDictionary {
    let resource_dir = config_path
        .parent()
        .map(|p| p.to_path_buf())
        .filter(|p| !p.as_os_str().is_empty());
    let dict_override = std::env::var_os("SUDACHI_BENCH_DICT").map(PathBuf::from);
    let config = Config::new(Some(config_path.clone()), resource_dir, dict_override)
        .expect("failed to load config");
    JapaneseDictionary::from_cfg(&config).expect("failed to load dictionary")
}

fn mix(mut s: u64, v: u64) -> u64 {
    s ^= v;
    s.wrapping_mul(1_099_511_628_211)
}

fn checksum(tok: &mut Tok, result: &mut MList, lines: &[&str]) -> u64 {
    let mut sum = 14_695_981_039_346_656_037u64;
    for line in lines {
        tok.reset().push_str(line);
        tok.do_tokenize().expect("tokenize failed");
        result.collect_results(tok).expect("collect failed");
        for i in 0..result.len() {
            let m = result.get(i);
            sum = mix(sum, m.begin() as u64);
            sum = mix(sum, m.end() as u64);
            for &b in m.surface().as_bytes() {
                sum = mix(sum, b as u64);
            }
            sum = mix(sum, m.part_of_speech_id() as u64);
        }
    }
    sum
}

fn time_do_tokenize(tok: &mut Tok, lines: &[&str]) -> f64 {
    let start = Instant::now();
    for line in lines {
        tok.reset().push_str(line);
        tok.do_tokenize().expect("tokenize failed");
    }
    start.elapsed().as_secs_f64() * 1e3
}

fn stats(s: &mut [f64]) -> (f64, f64, f64) {
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    (s[0], s[s.len() / 2], s.iter().sum::<f64>() / s.len() as f64)
}

fn main() {
    let config_path = PathBuf::from(
        std::env::var("SUDACHI_BENCH_CONFIG").unwrap_or_else(|_| "resources/sudachi.json".into()),
    );
    let inputs = std::env::var("SUDACHI_BENCH_INPUTS")
        .unwrap_or_else(|_| "target/issue-117-corpora/kyoto-leads.txt".into());
    let trials: usize = std::env::var("SUDACHI_BENCH_TRIALS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(31);
    let lambda: i32 = std::env::var("SUDACHI_SPARSE_LAMBDA")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    let dense = Arc::new(load(&config_path));
    let mut sparse_dict = load(&config_path);
    let (nl, nr) = {
        let m = sparse_dict.grammar().conn_matrix();
        (m.num_left(), m.num_right())
    };
    let (nnz, heap) = sparse_dict.sparsify_matrix(lambda);
    let sparse = Arc::new(sparse_dict);

    let dense_bytes = nl * nr * 2;
    let total_cells = nl * nr;
    println!(
        "# matrix {nl}x{nr} = {total_cells} cells | dense {} MiB | sparse(lambda={lambda}) {} MiB, nnz={nnz} ({:.1}% kept)",
        dense_bytes / (1 << 20),
        heap / (1 << 20),
        100.0 * nnz as f64 / total_cells as f64
    );

    let text = std::fs::read_to_string(&inputs).expect("failed to read inputs");
    let lines: Vec<&str> = text.lines().collect();
    let total_chars: usize = lines.iter().map(|l| l.chars().count()).sum();

    let mut tok_d = StatefulTokenizer::new(dense.clone(), Mode::C);
    let mut tok_s = StatefulTokenizer::new(sparse.clone(), Mode::C);
    let mut res_d = MorphemeList::empty(dense.clone());
    let mut res_s = MorphemeList::empty(sparse.clone());

    // Correctness: at lambda=0 the sparse matrix must reproduce the dense output.
    let cd = checksum(&mut tok_d, &mut res_d, &lines);
    let cs = checksum(&mut tok_s, &mut res_s, &lines);
    let identical = cd == cs;
    println!(
        "# output byte-identical: {identical}  (dense {cd:016x} | sparse {cs:016x}){}",
        if lambda == 0 && !identical {
            "  !!! REGRESSION at lambda=0"
        } else {
            ""
        }
    );
    assert!(lambda != 0 || identical, "lambda=0 must be byte-identical");

    let mut dms: Vec<f64> = Vec::new();
    let mut sms: Vec<f64> = Vec::new();
    for t in 0..trials {
        if t % 2 == 0 {
            dms.push(time_do_tokenize(&mut tok_d, &lines));
            sms.push(time_do_tokenize(&mut tok_s, &lines));
        } else {
            sms.push(time_do_tokenize(&mut tok_s, &lines));
            dms.push(time_do_tokenize(&mut tok_d, &lines));
        }
    }
    let (dmin, dmed, _) = stats(&mut dms);
    let (smin, smed, _) = stats(&mut sms);
    println!("# sentences {}, chars {}, trials {}", lines.len(), total_chars, trials);
    println!(
        "dense   min {dmin:>7.2}  median {dmed:>7.2} ms  {:>6.2} ns/char",
        dmed * 1e6 / total_chars.max(1) as f64
    );
    println!(
        "sparse  min {smin:>7.2}  median {smed:>7.2} ms  {:>6.2} ns/char",
        smed * 1e6 / total_chars.max(1) as f64
    );
    println!(
        "# sparse speedup (dense/sparse)  median {:.4}x   best-of {:.4}x",
        dmed / smed,
        dmin / smin
    );
}
