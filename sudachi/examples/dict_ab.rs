/*
 * PROBE (not for upstream). In-process, per-trial-interleaved A/B of two system
 * dictionaries that differ ONLY in trie layout (e.g. baseline vs corpus-
 * frequency weighted). Same process / page cache / thermal state, paired sign
 * test — the only reliable way to see a sub-1% end-to-end effect. Also verifies
 * the two dicts produce identical tokenization (layout must not change output).
 *
 *   SUDACHI_DICT_A=baseline.dic SUDACHI_DICT_B=weighted.dic \
 *   SUDACHI_BENCH_CONFIG=resources/sudachi.json \
 *   SUDACHI_BENCH_INPUTS=.../kyoto-leads.txt SUDACHI_AB_TRIALS=41 \
 *   [SUDACHI_BENCH_NOCOLLECT=1] cargo run -p sudachi --release --example dict_ab
 */
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

fn load(config_path: &PathBuf, dict: PathBuf) -> Arc<JapaneseDictionary> {
    let resource_dir = config_path.parent().map(|p| p.to_path_buf()).filter(|p| !p.as_os_str().is_empty());
    let config = Config::new(Some(config_path.clone()), resource_dir, Some(dict)).expect("config");
    Arc::new(JapaneseDictionary::from_cfg(&config).expect("dict"))
}

fn main() {
    let config_path = env_path("SUDACHI_BENCH_CONFIG", "resources/sudachi.json");
    let dict_a = load(&config_path, env_path("SUDACHI_DICT_A", "target/bench-lookup/layout/baseline.dic"));
    let dict_b = load(&config_path, env_path("SUDACHI_DICT_B", "target/bench-lookup/layout/weighted.dic"));

    let inputs = env_path("SUDACHI_BENCH_INPUTS", "target/issue-117-corpora/kyoto-leads.txt");
    let text = std::fs::read_to_string(&inputs).expect("inputs");
    let lines: Vec<&str> = text.lines().collect();
    let total_chars: usize = lines.iter().map(|l| l.chars().count()).sum();
    let trials: usize = std::env::var("SUDACHI_AB_TRIALS").ok().and_then(|v| v.parse().ok()).unwrap_or(41);
    let collect = std::env::var_os("SUDACHI_BENCH_NOCOLLECT").is_none();

    let mut ta = StatefulTokenizer::new(dict_a.clone(), Mode::C);
    ta.set_pipelined_lookup(true);
    let mut tb = StatefulTokenizer::new(dict_b.clone(), Mode::C);
    tb.set_pipelined_lookup(true);
    let mut ra = MorphemeList::empty(dict_a.clone());
    let mut rb = MorphemeList::empty(dict_b.clone());

    // parity: identical tokenization (count + spans hash) over the corpus
    let collect_sig = |tok: &mut StatefulTokenizer<Arc<JapaneseDictionary>>, res: &mut MorphemeList<Arc<JapaneseDictionary>>| -> (usize, u64) {
        let mut n = 0usize;
        let mut h: u64 = 1469598103934665603;
        for line in &lines {
            tok.reset().push_str(line);
            tok.do_tokenize().expect("tok");
            res.collect_results(tok).expect("collect");
            n += res.len();
            for i in 0..res.len() {
                let m = res.get(i);
                for v in [m.begin_c() as u64, m.end_c() as u64] {
                    h ^= v;
                    h = h.wrapping_mul(1099511628211);
                }
            }
        }
        (n, h)
    };
    let sig_a = collect_sig(&mut ta, &mut ra);
    let sig_b = collect_sig(&mut tb, &mut rb);
    println!("# parity: A morphs={} hash={:016x} | B morphs={} hash={:016x} | identical={}",
        sig_a.0, sig_a.1, sig_b.0, sig_b.1, sig_a == sig_b);

    let pass = |tok: &mut StatefulTokenizer<Arc<JapaneseDictionary>>, res: &mut MorphemeList<Arc<JapaneseDictionary>>| {
        for line in &lines {
            tok.reset().push_str(line);
            tok.do_tokenize().expect("tok");
            if collect {
                res.collect_results(tok).expect("collect");
            }
        }
    };
    // warm
    for _ in 0..2 { pass(&mut ta, &mut ra); pass(&mut tb, &mut rb); }

    let mut sa = Vec::with_capacity(trials);
    let mut sb = Vec::with_capacity(trials);
    for t in 0..trials {
        // alternate order each trial
        if t % 2 == 0 {
            let s = Instant::now(); pass(&mut ta, &mut ra); sa.push(s.elapsed().as_secs_f64() * 1e3);
            let s = Instant::now(); pass(&mut tb, &mut rb); sb.push(s.elapsed().as_secs_f64() * 1e3);
        } else {
            let s = Instant::now(); pass(&mut tb, &mut rb); sb.push(s.elapsed().as_secs_f64() * 1e3);
            let s = Instant::now(); pass(&mut ta, &mut ra); sa.push(s.elapsed().as_secs_f64() * 1e3);
        }
    }
    let med = |v: &mut [f64]| { v.sort_by(|a, b| a.partial_cmp(b).unwrap()); v[v.len() / 2] };
    let cv = |v: &[f64]| { let m = v.iter().sum::<f64>() / v.len() as f64; (v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / v.len() as f64).sqrt() / m * 100.0 };
    let nspc = |ms: f64| ms * 1e6 / total_chars as f64;
    let wins_b = (0..trials).filter(|&t| sb[t] < sa[t]).count();
    let ma = med(&mut sa.clone());
    let mb = med(&mut sb.clone());
    println!("# {} ({} trials, {} chars)", if collect { "full pipeline" } else { "do_tokenize only" }, trials, total_chars);
    println!("  A (baseline) : {:.2} ns/char  cv {:.1}%", nspc(ma), cv(&sa));
    println!("  B (weighted) : {:.2} ns/char  cv {:.1}%   delta {:+.2}%   B beats A {}/{}", nspc(mb), cv(&sb), (mb / ma - 1.0) * 100.0, wins_b, trials);
}
