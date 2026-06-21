/*
 * PROBE (not for upstream). Direct test of eiennohito's literal claim: "L1
 * regularization, so the noise from weird features can be skipped." A faithful
 * from-scratch CRF retrain is impossible in this repo (no trainer/templates/
 * training corpus). The closest non-misleading in-repo proxy: take the REAL
 * matrix, model the additive base a[l]+b[r] (the bulk, ~68%), and soft-threshold
 * the residual "feature" deviations — zero every |residual| < lambda. That is
 * exactly what L1 does: drive small feature contributions to zero. Then score
 * segmentation on UD_Japanese-GSD gold and report quality vs sparsity.
 *
 * lambda=0 must reproduce the real matrix (sanity). Sweep lambda externally.
 */
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use sudachi::analysis::mlist::MorphemeList;
use sudachi::analysis::stateful_tokenizer::StatefulTokenizer;
use sudachi::analysis::Mode;
use sudachi::config::Config;
use sudachi::dic::connect::install_replace;
use sudachi::dic::dictionary::JapaneseDictionary;

fn env_path(key: &str, default: &str) -> PathBuf {
    std::env::var_os(key).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(default))
}

fn parse_gsd(path: &PathBuf) -> Vec<(String, Vec<(u32, u32)>)> {
    let raw = std::fs::read_to_string(path).expect("conllu");
    let mut out = Vec::new();
    let mut text: Option<String> = None;
    let mut forms: Vec<String> = Vec::new();
    let mut flush = |text: &mut Option<String>, forms: &mut Vec<String>, out: &mut Vec<(String, Vec<(u32, u32)>)>| {
        if let Some(t) = text.take() {
            let chars: Vec<char> = t.chars().collect();
            let mut spans = Vec::new();
            let mut cur = 0usize;
            let mut ok = true;
            for f in forms.iter() {
                let fc: Vec<char> = f.chars().collect();
                let mut found = None;
                if !fc.is_empty() {
                    let mut i = cur;
                    while i + fc.len() <= chars.len() {
                        if chars[i..i + fc.len()] == fc[..] { found = Some(i); break; }
                        i += 1;
                    }
                }
                match found { Some(p) => { spans.push((p as u32, (p + fc.len()) as u32)); cur = p + fc.len(); } None => { ok = false; break; } }
            }
            if ok && !spans.is_empty() { out.push((t, spans)); }
        }
        forms.clear();
    };
    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix("# text = ") { text = Some(rest.to_string()); }
        else if line.is_empty() { flush(&mut text, &mut forms, &mut out); }
        else if line.starts_with('#') { continue; }
        else {
            let mut it = line.split('\t');
            let id = it.next().unwrap_or("");
            if id.contains('-') || id.contains('.') { continue; }
            if let Some(form) = it.next() { forms.push(form.to_string()); }
        }
    }
    flush(&mut text, &mut forms, &mut out);
    out
}

fn segment(tok: &mut StatefulTokenizer<Arc<JapaneseDictionary>>, res: &mut MorphemeList<Arc<JapaneseDictionary>>, sents: &[(String, Vec<(u32, u32)>)]) -> Vec<Vec<(u32, u32)>> {
    let mut out = Vec::with_capacity(sents.len());
    for (text, _) in sents {
        tok.reset().push_str(text);
        tok.do_tokenize().expect("tok");
        res.collect_results(tok).expect("collect");
        let mut spans = Vec::with_capacity(res.len());
        for i in 0..res.len() { let m = res.get(i); spans.push((m.begin_c() as u32, m.end_c() as u32)); }
        out.push(spans);
    }
    out
}

fn f1(sents: &[(String, Vec<(u32, u32)>)], pred: &[Vec<(u32, u32)>]) -> f64 {
    let (mut c, mut g, mut p) = (0u64, 0u64, 0u64);
    for ((_, gold), pr) in sents.iter().zip(pred.iter()) {
        let gs: HashSet<(u32, u32)> = gold.iter().cloned().collect();
        g += gold.len() as u64; p += pr.len() as u64;
        for s in pr { if gs.contains(s) { c += 1; } }
    }
    let (pr, rc) = (c as f64 / p as f64, c as f64 / g as f64);
    2.0 * pr * rc / (pr + rc)
}

fn main() {
    let config_path = env_path("SUDACHI_BENCH_CONFIG", "resources/sudachi.json");
    let resource_dir = config_path.parent().map(|p| p.to_path_buf()).filter(|p| !p.as_os_str().is_empty());
    let dict_override = std::env::var_os("SUDACHI_BENCH_DICT").map(PathBuf::from);
    let config = Config::new(Some(config_path.clone()), resource_dir, dict_override).expect("config");
    let dict = Arc::new(JapaneseDictionary::from_cfg(&config).expect("dict"));
    let sents = parse_gsd(&env_path("SUDACHI_GSD", "bench/quality/ja_gsd-ud-test.conllu"));
    let lambda: f64 = std::env::var("SUDACHI_L1_THRESH").ok().and_then(|v| v.parse().ok()).unwrap_or(0.0);

    let mut tok = StatefulTokenizer::new(dict.clone(), Mode::A);
    tok.set_pipelined_lookup(true);
    let mut res = MorphemeList::empty(dict.clone());

    // baseline (real matrix) BEFORE replacement
    let base = segment(&mut tok, &mut res, &sents);
    let base_f1 = f1(&sents, &base);

    // additive model a[l]+b[r] + soft-threshold the residual
    let conn = dict.grammar().conn_matrix();
    let nl = conn.num_left();
    let nr = conn.num_right();
    let mut a = vec![0f64; nl];
    let mut b = vec![0f64; nr];
    let grand = {
        let mut s = 0f64;
        for l in 0..nl { for r in 0..nr { s += conn.cost(l as u16, r as u16) as f64; } }
        s / (nl * nr) as f64
    };
    for _ in 0..3 {
        for l in 0..nl { let mut s = 0f64; for r in 0..nr { s += conn.cost(l as u16, r as u16) as f64 - b[r]; } a[l] = s / nr as f64 - grand; }
        for r in 0..nr { let mut s = 0f64; for l in 0..nl { s += conn.cost(l as u16, r as u16) as f64 - a[l]; } b[r] = s / nl as f64; }
    }
    let mut zeroed = 0u64;
    let mut repl = vec![0i16; nl * nr];
    for l in 0..nl {
        for r in 0..nr {
            let m = conn.cost(l as u16, r as u16) as f64;
            let resid = m - (a[l] + b[r]);
            let kept = if resid.abs() < lambda { zeroed += 1; 0.0 } else { resid };
            let v = (a[l] + b[r] + kept).round().clamp(-32768.0, 32767.0) as i16;
            repl[r * nl + l] = v;
        }
    }
    let frac = 100.0 * zeroed as f64 / (nl * nr) as f64;
    install_replace(repl);
    let after = segment(&mut tok, &mut res, &sents);
    let after_f1 = f1(&sents, &after);

    println!("# L1-style residual soft-threshold on the REAL matrix (post-hoc proxy for L1-CRF)");
    println!("  lambda={lambda}  residuals zeroed = {:.1}% (= 'noisy features skipped')", frac);
    println!("  GSD SEG-F1: real {:.3} -> thresholded {:.3}  (delta {:+.3})", 100.0 * base_f1, 100.0 * after_f1, 100.0 * (after_f1 - base_f1));
}
