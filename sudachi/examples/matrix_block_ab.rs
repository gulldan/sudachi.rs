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

//! In-process A/B of the DENSE connection matrix vs the co-clustered class BLOCK
//! (`ConnectionMatrix::blockify`) — the small-dense, L1-resident speed candidate.
//! Tests it as a real runtime structure: footprint, do_tokenize speed, and the
//! quality cost on UD_Japanese-GSD segmentation (the block is lossy).
//!
//! SUDACHI_BENCH_CONFIG  SUDACHI_BENCH_DICT  SUDACHI_BENCH_INPUTS
//! SUDACHI_BENCH_TRIALS  SUDACHI_GSD  SUDACHI_BLOCK_K (256)  SUDACHI_BLOCK_SAMPLE
//! (256)  SUDACHI_BLOCK_ITERS (12)

use std::collections::HashSet;
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

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

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

fn time_do_tokenize(tok: &mut Tok, lines: &[&str]) -> f64 {
    let start = Instant::now();
    for line in lines {
        tok.reset().push_str(line);
        tok.do_tokenize().expect("tokenize failed");
    }
    start.elapsed().as_secs_f64() * 1e3
}

fn stats(s: &mut [f64]) -> (f64, f64) {
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    (s[0], s[s.len() / 2])
}

/// Parse conllu -> (text, gold char spans). Skips sentences whose gold forms
/// cannot be located in the text.
fn parse_gsd(path: &PathBuf) -> Vec<(String, Vec<(u32, u32)>)> {
    let raw = std::fs::read_to_string(path).expect("read conllu");
    let mut out = Vec::new();
    let mut text: Option<String> = None;
    let mut forms: Vec<String> = Vec::new();
    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix("# text = ") {
            text = Some(rest.to_string());
        } else if line.is_empty() {
            if let Some(t) = text.take() {
                let chars: Vec<char> = t.chars().collect();
                let mut spans = Vec::with_capacity(forms.len());
                let mut cursor = 0usize;
                let mut ok = true;
                for f in forms.iter() {
                    let fc: Vec<char> = f.chars().collect();
                    let mut found = None;
                    if !fc.is_empty() {
                        let mut i = cursor;
                        while i + fc.len() <= chars.len() {
                            if chars[i..i + fc.len()] == fc[..] {
                                found = Some(i);
                                break;
                            }
                            i += 1;
                        }
                    }
                    match found {
                        Some(p) => {
                            spans.push((p as u32, (p + fc.len()) as u32));
                            cursor = p + fc.len();
                        }
                        None => {
                            ok = false;
                            break;
                        }
                    }
                }
                if ok && !spans.is_empty() {
                    out.push((t, spans));
                }
            }
            forms.clear();
        } else if line.starts_with('#') {
            continue;
        } else {
            let mut it = line.split('\t');
            let id = it.next().unwrap_or("");
            if id.contains('-') || id.contains('.') {
                continue;
            }
            if let Some(form) = it.next() {
                forms.push(form.to_string());
            }
        }
    }
    out
}

fn segment(tok: &mut Tok, result: &mut MList, sents: &[(String, Vec<(u32, u32)>)]) -> Vec<Vec<(u32, u32)>> {
    let mut out = Vec::with_capacity(sents.len());
    for (text, _) in sents {
        tok.reset().push_str(text);
        tok.do_tokenize().expect("tokenize");
        result.collect_results(tok).expect("collect");
        let mut spans = Vec::with_capacity(result.len());
        for i in 0..result.len() {
            let m = result.get(i);
            spans.push((m.begin_c() as u32, m.end_c() as u32));
        }
        out.push(spans);
    }
    out
}

fn seg_f1(sents: &[(String, Vec<(u32, u32)>)], pred: &[Vec<(u32, u32)>]) -> f64 {
    let (mut correct, mut gold_total, mut pred_total) = (0u64, 0u64, 0u64);
    for ((_, gold), p) in sents.iter().zip(pred.iter()) {
        let gset: HashSet<(u32, u32)> = gold.iter().cloned().collect();
        gold_total += gold.len() as u64;
        pred_total += p.len() as u64;
        for s in p {
            if gset.contains(s) {
                correct += 1;
            }
        }
    }
    let pr = correct as f64 / pred_total.max(1) as f64;
    let rc = correct as f64 / gold_total.max(1) as f64;
    2.0 * pr * rc / (pr + rc)
}

fn main() {
    let config_path = PathBuf::from(
        std::env::var("SUDACHI_BENCH_CONFIG").unwrap_or_else(|_| "resources/sudachi.json".into()),
    );
    let inputs = std::env::var("SUDACHI_BENCH_INPUTS")
        .unwrap_or_else(|_| "target/issue-117-corpora/kyoto-leads.txt".into());
    let trials = env_usize("SUDACHI_BENCH_TRIALS", 25);
    let k = env_usize("SUDACHI_BLOCK_K", 256);
    let sample = env_usize("SUDACHI_BLOCK_SAMPLE", 256);
    let iters = env_usize("SUDACHI_BLOCK_ITERS", 12);

    let dense = Arc::new(load(&config_path));
    let mut block_dict = load(&config_path);
    let (nl, nr) = {
        let m = block_dict.grammar().conn_matrix();
        (m.num_left(), m.num_right())
    };
    let heap = block_dict.blockify_matrix(k, sample, iters);
    let block = Arc::new(block_dict);

    println!(
        "# matrix {nl}x{nr} | dense {} MiB | block K={k} {} KiB ({}x smaller)",
        nl * nr * 2 / (1 << 20),
        heap / 1024,
        (nl * nr * 2) / heap.max(1)
    );

    // Speed A/B over the corpus.
    let text = std::fs::read_to_string(&inputs).expect("read inputs");
    let lines: Vec<&str> = text.lines().collect();
    let total_chars: usize = lines.iter().map(|l| l.chars().count()).sum();
    let mut tok_d = StatefulTokenizer::new(dense.clone(), Mode::C);
    let mut tok_b = StatefulTokenizer::new(block.clone(), Mode::C);
    let mut dms: Vec<f64> = Vec::new();
    let mut bms: Vec<f64> = Vec::new();
    for t in 0..trials {
        if t % 2 == 0 {
            dms.push(time_do_tokenize(&mut tok_d, &lines));
            bms.push(time_do_tokenize(&mut tok_b, &lines));
        } else {
            bms.push(time_do_tokenize(&mut tok_b, &lines));
            dms.push(time_do_tokenize(&mut tok_d, &lines));
        }
    }
    let (dmin, dmed) = stats(&mut dms);
    let (bmin, bmed) = stats(&mut bms);
    println!(
        "dense  median {dmed:>7.2} ms {:>6.2} ns/char | block  median {bmed:>7.2} ms {:>6.2} ns/char",
        dmed * 1e6 / total_chars.max(1) as f64,
        bmed * 1e6 / total_chars.max(1) as f64
    );
    println!(
        "# block speedup (dense/block)  median {:.4}x   best-of {:.4}x",
        dmed / bmed,
        dmin / bmin
    );

    // Quality on GSD (lossy block -> must measure).
    if let Ok(gsd) = std::env::var("SUDACHI_GSD") {
        let sents = parse_gsd(&PathBuf::from(gsd));
        let mut rd = MorphemeList::empty(dense.clone());
        let mut rb = MorphemeList::empty(block.clone());
        let f1_d = seg_f1(&sents, &segment(&mut tok_d, &mut rd, &sents));
        let f1_b = seg_f1(&sents, &segment(&mut tok_b, &mut rb, &sents));
        println!(
            "# GSD seg-F1: dense {:.3} | block {:.3}  (delta {:+.3})  over {} sents",
            f1_d * 100.0,
            f1_b * 100.0,
            (f1_b - f1_d) * 100.0,
            sents.len()
        );
    }
}
