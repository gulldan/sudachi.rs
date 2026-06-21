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

//! Quality of the co-clustered class BLOCK matrix (`ConnectionMatrix::blockify`)
//! vs the dense matrix, across modes A/B/C and including POS.
//!
//! - vs DENSE output (same dict + POS table, so scheme-correct): segmentation and
//!   segmentation+POS agreement F1 over a large corpus. This is the real "how
//!   lossy is the approximation relative to Sudachi itself" measure.
//! - vs GSD gold: segmentation F1 (POS scheme differs from gold, so POS is not
//!   scored against gold — only the dense-vs-block POS agreement above is).
//!
//! SUDACHI_BENCH_CONFIG  SUDACHI_BENCH_DICT  SUDACHI_BENCH_INPUTS  SUDACHI_GSD
//! SUDACHI_BLOCK_K (256)  SUDACHI_BLOCK_SAMPLE (256)  SUDACHI_BLOCK_ITERS (12)
//! SUDACHI_QUALITY_LIMIT (cap corpus sentences for the agreement pass)

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

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

/// Tokenize each text into (begin_c, end_c, pos_id) per token.
fn tokenize(tok: &mut Tok, result: &mut MList, texts: &[&str]) -> Vec<Vec<(u32, u32, u16)>> {
    let mut out = Vec::with_capacity(texts.len());
    for text in texts {
        tok.reset().push_str(text);
        tok.do_tokenize().expect("tokenize");
        result.collect_results(tok).expect("collect");
        let mut toks = Vec::with_capacity(result.len());
        for i in 0..result.len() {
            let m = result.get(i);
            toks.push((m.begin_c() as u32, m.end_c() as u32, m.part_of_speech_id()));
        }
        out.push(toks);
    }
    out
}

/// F1 of `pred` token set vs `gold` token set, summed over sentences. `with_pos`
/// keys on (span, pos_id); otherwise on span only.
fn f1(gold: &[Vec<(u32, u32, u16)>], pred: &[Vec<(u32, u32, u16)>], with_pos: bool) -> f64 {
    let key = |t: &(u32, u32, u16)| -> (u32, u32, u16) {
        if with_pos {
            *t
        } else {
            (t.0, t.1, 0)
        }
    };
    let (mut correct, mut gold_total, mut pred_total) = (0u64, 0u64, 0u64);
    for (g, p) in gold.iter().zip(pred.iter()) {
        let gset: HashSet<(u32, u32, u16)> = g.iter().map(key).collect();
        gold_total += g.len() as u64;
        pred_total += p.len() as u64;
        for t in p {
            if gset.contains(&key(t)) {
                correct += 1;
            }
        }
    }
    let pr = correct as f64 / pred_total.max(1) as f64;
    let rc = correct as f64 / gold_total.max(1) as f64;
    2.0 * pr * rc / (pr + rc)
}

/// Parse conllu -> (text, gold seg spans). POS not extracted (scheme differs).
fn parse_gsd(path: &PathBuf) -> Vec<(String, Vec<(u32, u32, u16)>)> {
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
                            spans.push((p as u32, (p + fc.len()) as u32, 0u16));
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

fn main() {
    let config_path = PathBuf::from(
        std::env::var("SUDACHI_BENCH_CONFIG").unwrap_or_else(|_| "resources/sudachi.json".into()),
    );
    let inputs = std::env::var("SUDACHI_BENCH_INPUTS")
        .unwrap_or_else(|_| "target/issue-117-corpora/kyoto-leads.txt".into());
    let gsd_path = std::env::var("SUDACHI_GSD").unwrap_or_else(|_| "bench/quality/ja_gsd-ud-test.conllu".into());
    let k = env_usize("SUDACHI_BLOCK_K", 256);
    let sample = env_usize("SUDACHI_BLOCK_SAMPLE", 256);
    let iters = env_usize("SUDACHI_BLOCK_ITERS", 12);
    let limit = env_usize("SUDACHI_QUALITY_LIMIT", 4000);

    let dense = Arc::new(load(&config_path));
    let mut block_dict = load(&config_path);
    let heap = block_dict.blockify_matrix(k, sample, iters);
    let block = Arc::new(block_dict);
    println!("# block K={k}, sample={sample}, iters={iters}, block heap {} KiB", heap / 1024);

    let text = std::fs::read_to_string(&inputs).expect("read inputs");
    let corpus: Vec<&str> = text.lines().take(limit).collect();
    let gsd = parse_gsd(&PathBuf::from(&gsd_path));
    let gsd_texts: Vec<&str> = gsd.iter().map(|(t, _)| t.as_str()).collect();
    let gsd_gold: Vec<Vec<(u32, u32, u16)>> = gsd.iter().map(|(_, s)| s.clone()).collect();
    println!(
        "# corpus agreement on {} sents | GSD seg-F1 on {} sents",
        corpus.len(),
        gsd.len()
    );
    println!("# (agreement = block vs DENSE Sudachi output, same POS table — scheme-correct)");
    println!("mode | seg-agree | seg+POS-agree | GSD seg-F1 dense | GSD seg-F1 block | dF1");

    for mode in [Mode::A, Mode::B, Mode::C] {
        let mut td = StatefulTokenizer::new(dense.clone(), mode);
        let mut tb = StatefulTokenizer::new(block.clone(), mode);
        let mut rd = MorphemeList::empty(dense.clone());
        let mut rb = MorphemeList::empty(block.clone());

        // Agreement with dense over the corpus (seg and seg+POS).
        let cd = tokenize(&mut td, &mut rd, &corpus);
        let cb = tokenize(&mut tb, &mut rb, &corpus);
        let seg_agree = f1(&cd, &cb, false) * 100.0;
        let pos_agree = f1(&cd, &cb, true) * 100.0;

        // GSD segmentation F1 vs gold for both.
        let gd = tokenize(&mut td, &mut rd, &gsd_texts);
        let gb = tokenize(&mut tb, &mut rb, &gsd_texts);
        let f1d = f1(&gsd_gold, &gd, false) * 100.0;
        let f1b = f1(&gsd_gold, &gb, false) * 100.0;

        let mname = match mode {
            Mode::A => "A",
            Mode::B => "B",
            Mode::C => "C",
        };
        println!(
            "  {mname}  |  {seg_agree:6.3}%  |   {pos_agree:6.3}%    |     {f1d:6.3}      |     {f1b:6.3}      | {:+.3}",
            f1b - f1d
        );
    }
}
