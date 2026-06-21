/*
 * PROBE (not for upstream). The noise-vs-signal test for "relearn / simplify the
 * connection matrix". We score segmentation against UD_Japanese-GSD gold with
 * (a) the real matrix and (b) a compact block-256 approximation that fits L1.
 * If the approximation's disagreements with the real matrix move tokens TOWARD
 * gold (fixes > regressions, approx F1 >= baseline F1), the removed structure
 * was overfit noise -> eiennohito's "L1 can drop weird features" is supported.
 * If they move AWAY from gold, that structure was signal -> negative.
 *
 * Segmentation only (token spans); POS scheme differs and is not scored.
 */
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use sudachi::analysis::mlist::MorphemeList;
use sudachi::analysis::stateful_tokenizer::StatefulTokenizer;
use sudachi::analysis::Mode;
use sudachi::config::Config;
use sudachi::dic::connect::{install_approx, ApproxModel};
use sudachi::dic::dictionary::JapaneseDictionary;

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

// Parse conllu -> Vec<(text, gold spans in char offsets)>. Skips sentences whose
// gold forms cannot be located in the text.
fn parse_gsd(path: &PathBuf) -> Vec<(String, Vec<(u32, u32)>)> {
    let raw = std::fs::read_to_string(path).expect("read conllu");
    let mut out = Vec::new();
    let mut text: Option<String> = None;
    let mut forms: Vec<String> = Vec::new();
    let mut flush = |text: &mut Option<String>,
                     forms: &mut Vec<String>,
                     out: &mut Vec<(String, Vec<(u32, u32)>)>| {
        if let Some(t) = text.take() {
            let chars: Vec<char> = t.chars().collect();
            let mut spans = Vec::with_capacity(forms.len());
            let mut cursor = 0usize;
            let mut ok = true;
            for f in forms.iter() {
                let fc: Vec<char> = f.chars().collect();
                // find fc in chars[cursor..]
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
    };
    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix("# text = ") {
            text = Some(rest.to_string());
        } else if line.is_empty() {
            flush(&mut text, &mut forms, &mut out);
        } else if line.starts_with('#') {
            continue;
        } else {
            let mut it = line.split('\t');
            let id = it.next().unwrap_or("");
            if id.contains('-') || id.contains('.') {
                continue; // multiword range / empty node
            }
            if let Some(form) = it.next() {
                forms.push(form.to_string());
            }
        }
    }
    flush(&mut text, &mut forms, &mut out);
    out
}

fn kmeans(points: &[Vec<f32>], k: usize, iters: usize) -> Vec<u16> {
    let n = points.len();
    let dim = points[0].len();
    let k = k.min(n);
    let mut cent: Vec<Vec<f32>> = (0..k).map(|i| points[i * n / k].clone()).collect();
    let mut assign = vec![0u16; n];
    for _ in 0..iters {
        for (pi, p) in points.iter().enumerate() {
            let mut best = 0usize;
            let mut bestd = f32::MAX;
            for (ci, c) in cent.iter().enumerate() {
                let mut d = 0f32;
                for j in 0..dim {
                    let df = p[j] - c[j];
                    d += df * df;
                    if d >= bestd {
                        break;
                    }
                }
                if d < bestd {
                    bestd = d;
                    best = ci;
                }
            }
            assign[pi] = best as u16;
        }
        let mut sums = vec![vec![0f64; dim]; k];
        let mut cnts = vec![0u64; k];
        for (pi, p) in points.iter().enumerate() {
            let a = assign[pi] as usize;
            cnts[a] += 1;
            for j in 0..dim {
                sums[a][j] += p[j] as f64;
            }
        }
        for ci in 0..k {
            if cnts[ci] > 0 {
                for j in 0..dim {
                    cent[ci][j] = (sums[ci][j] / cnts[ci] as f64) as f32;
                }
            }
        }
    }
    assign
}

fn segment(
    tok: &mut StatefulTokenizer<Arc<JapaneseDictionary>>,
    result: &mut MorphemeList<Arc<JapaneseDictionary>>,
    sents: &[(String, Vec<(u32, u32)>)],
) -> Vec<Vec<(u32, u32)>> {
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

fn prf(sents: &[(String, Vec<(u32, u32)>)], pred: &[Vec<(u32, u32)>]) -> (f64, f64, f64) {
    let mut correct = 0u64;
    let mut gold_total = 0u64;
    let mut pred_total = 0u64;
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
    let pr = correct as f64 / pred_total as f64;
    let rc = correct as f64 / gold_total as f64;
    (pr, rc, 2.0 * pr * rc / (pr + rc))
}

fn main() {
    let config_path = env_path("SUDACHI_BENCH_CONFIG", "resources/sudachi.json");
    let resource_dir = config_path
        .parent()
        .map(|p| p.to_path_buf())
        .filter(|p| !p.as_os_str().is_empty());
    let dict_override = std::env::var_os("SUDACHI_BENCH_DICT").map(PathBuf::from);
    let config =
        Config::new(Some(config_path.clone()), resource_dir, dict_override).expect("config");
    let dict = Arc::new(JapaneseDictionary::from_cfg(&config).expect("dict"));

    let gsd = env_path("SUDACHI_GSD", "bench/quality/ja_gsd-ud-test.conllu");
    let sents = parse_gsd(&gsd);
    println!("# GSD sentences scored: {}", sents.len());

    let mode = match std::env::var("SUDACHI_GSD_MODE").as_deref() {
        Ok("C") => Mode::C,
        Ok("B") => Mode::B,
        _ => Mode::A,
    };
    let mut tok = StatefulTokenizer::new(dict.clone(), mode);
    tok.set_pipelined_lookup(true);
    let mut result = MorphemeList::empty(dict.clone());

    // baseline (real matrix)
    let base = segment(&mut tok, &mut result, &sents);
    let (bp, br, bf) = prf(&sents, &base);

    // build block-K approximation
    let k = env_usize("SUDACHI_APPROX_K", 256);
    let sample = env_usize("SUDACHI_APPROX_SAMPLE", 256);
    let iters = env_usize("SUDACHI_APPROX_ITERS", 12);
    let conn = dict.grammar().conn_matrix();
    let nl = conn.num_left();
    let nr = conn.num_right();
    let sl: Vec<usize> = (0..nl).step_by((nl / sample).max(1)).take(sample).collect();
    let sr: Vec<usize> = (0..nr).step_by((nr / sample).max(1)).take(sample).collect();
    let rpoints: Vec<Vec<f32>> = (0..nr)
        .map(|r| {
            sl.iter()
                .map(|&l| conn.cost(l as u16, r as u16) as f32)
                .collect()
        })
        .collect();
    let clr = kmeans(&rpoints, k, iters);
    let lpoints: Vec<Vec<f32>> = (0..nl)
        .map(|l| {
            sr.iter()
                .map(|&r| conn.cost(l as u16, r as u16) as f32)
                .collect()
        })
        .collect();
    let cll = kmeans(&lpoints, k, iters);
    let (kl, kr) = (k, k);
    let mut bsum = vec![0i64; kl * kr];
    let mut bcnt = vec![0u64; kl * kr];
    for r in 0..nr {
        let b = clr[r] as usize;
        for l in 0..nl {
            let a = cll[l] as usize;
            bsum[a * kr + b] += conn.cost(l as u16, r as u16) as i64;
            bcnt[a * kr + b] += 1;
        }
    }
    let block: Vec<i16> = (0..kl * kr)
        .map(|i| {
            if bcnt[i] > 0 {
                (bsum[i] / bcnt[i] as i64) as i16
            } else {
                0
            }
        })
        .collect();
    install_approx(ApproxModel {
        cll,
        clr,
        block,
        kr,
    });

    // approx
    let approx = segment(&mut tok, &mut result, &sents);
    let (ap, ar, af) = prf(&sents, &approx);

    // noise-vs-signal at the gold-token level
    let mut fixes = 0u64; // gold token approx gets right that baseline got wrong
    let mut regress = 0u64; // gold token baseline got right that approx breaks
    for ((_, gold), (b, a)) in sents.iter().zip(base.iter().zip(approx.iter())) {
        let bset: HashSet<(u32, u32)> = b.iter().cloned().collect();
        let aset: HashSet<(u32, u32)> = a.iter().cloned().collect();
        for g in gold {
            let bm = bset.contains(g);
            let am = aset.contains(g);
            if am && !bm {
                fixes += 1;
            } else if bm && !am {
                regress += 1;
            }
        }
    }

    println!("## segmentation vs GSD gold (mode {:?})", mode);
    println!(
        "  baseline (real matrix) : P {:.2}  R {:.2}  F1 {:.3}",
        100.0 * bp,
        100.0 * br,
        100.0 * bf
    );
    println!(
        "  approx  (block-{k}, L1) : P {:.2}  R {:.2}  F1 {:.3}",
        100.0 * ap,
        100.0 * ar,
        100.0 * af
    );
    println!("  delta F1 (approx - baseline): {:+.3}", 100.0 * (af - bf));
    println!("## noise-vs-signal on changed gold tokens");
    println!("  fixes (approx->gold, baseline was wrong) : {fixes}");
    println!("  regressions (baseline->gold, approx breaks): {regress}");
    println!(
        "  net = fixes - regressions = {}  => removed structure was mostly {}",
        fixes as i64 - regress as i64,
        if fixes > regress {
            "NOISE (supports eiennohito)"
        } else {
            "SIGNAL (negative)"
        }
    );
}
