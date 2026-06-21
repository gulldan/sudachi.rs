/*
 * PROBE harness (not for upstream). In-process, per-trial-interleaved A/B of the
 * connection-matrix memory footprint. All conditions share the same loaded
 * dictionary, page cache, CPU frequency and thermal state; only the matrix
 * index mask changes between conditions, and conditions are rotated every trial
 * so no condition systematically benefits from warmup/thermal drift. This is
 * the clean causal test of "is the connection matrix memory-latency bound?".
 *
 *   SUDACHI_BENCH_CONFIG=resources/sudachi.json \
 *   SUDACHI_BENCH_DICT=target/bench-lookup/full-v1/system_full.dic \
 *   SUDACHI_BENCH_INPUTS=target/issue-117-corpora/kyoto-leads.txt \
 *   SUDACHI_AB_FOOTPRINTS=2097152,65536,8192,1024 SUDACHI_AB_TRIALS=41 \
 *   cargo run -p sudachi --release --example matrix_probe_ab
 */
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use sudachi::analysis::stateful_tokenizer::StatefulTokenizer;
use sudachi::analysis::Mode;
use sudachi::config::Config;
use sudachi::dic::connect::{
    probe_record_begin, probe_record_end, set_probe_mask, set_probe_nomat,
};
use sudachi::dic::dictionary::JapaneseDictionary;

fn env_path(key: &str, default: &str) -> PathBuf {
    std::env::var_os(key)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default))
}

fn main() {
    let config_path = env_path("SUDACHI_BENCH_CONFIG", "resources/sudachi.json");
    let resource_dir = config_path
        .parent()
        .map(|p| p.to_path_buf())
        .filter(|p| !p.as_os_str().is_empty());
    let dict_override = std::env::var_os("SUDACHI_BENCH_DICT").map(PathBuf::from);
    let config = Config::new(Some(config_path.clone()), resource_dir, dict_override)
        .expect("failed to load config");
    let dict = Arc::new(JapaneseDictionary::from_cfg(&config).expect("failed to load dictionary"));

    let inputs_path = env_path(
        "SUDACHI_BENCH_INPUTS",
        "target/issue-117-corpora/kyoto-leads.txt",
    );
    let text = std::fs::read_to_string(&inputs_path).expect("failed to read inputs");
    let lines: Vec<&str> = text.lines().collect();
    let total_chars: usize = lines.iter().map(|l| l.chars().count()).sum();
    let trials: usize = std::env::var("SUDACHI_AB_TRIALS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(41);

    // Conditions: condition 0 is always the real full matrix (mask = usize::MAX).
    let mut conditions: Vec<(String, usize)> = vec![("baseline(71MB)".to_string(), usize::MAX)];
    let footprints = std::env::var("SUDACHI_AB_FOOTPRINTS").unwrap_or_else(|_| "8192".to_string());
    for tok_str in footprints.split(',') {
        if let Ok(entries) = tok_str.trim().parse::<usize>() {
            let bytes = entries * 2;
            let label = if entries == 0 {
                "nomat(no load)".to_string()
            } else if bytes >= 1 << 20 {
                format!("{}MB", bytes >> 20)
            } else {
                format!("{}KB", bytes >> 10)
            };
            conditions.push((label, entries));
        }
    }
    let nconds = conditions.len();

    // Apply a condition: footprint 0 => nomat (no load); usize::MAX => full
    // matrix; otherwise confine the index to a small power-of-two footprint.
    let apply = |fp: usize| {
        if fp == 0 {
            set_probe_nomat(true);
            set_probe_mask(usize::MAX);
        } else {
            set_probe_nomat(false);
            set_probe_mask(fp);
        }
    };

    let mut tok = StatefulTokenizer::new(dict.clone(), Mode::C);
    tok.set_pipelined_lookup(true);

    let tokenize_pass = |tok: &mut StatefulTokenizer<Arc<JapaneseDictionary>>| {
        for line in &lines {
            tok.reset().push_str(line);
            tok.do_tokenize().expect("tokenization failed");
        }
    };

    // Working-set measurement: one recording pass, report distinct cells /
    // cache lines / bytes actually touched. This is the figure that decides
    // which cache level the matrix lives in on a given CPU.
    if std::env::var_os("SUDACHI_AB_WORKINGSET").is_some() {
        set_probe_mask(usize::MAX);
        probe_record_begin();
        tokenize_pass(&mut tok);
        let mut touched = probe_record_end();
        touched.sort_unstable();
        touched.dedup();
        let cells = touched.len();
        let uniq = |shift: u32| {
            let mut v: Vec<u32> = touched.iter().map(|&i| (i * 2) / (1 << shift)).collect();
            v.sort_unstable();
            v.dedup();
            v.len()
        };
        let lines64 = uniq(6); // 64-byte lines (x86: Zen4, Intel)
        let lines128 = uniq(7); // 128-byte lines (Apple M-series)
        println!("# WORKING SET of connection matrix over the full corpus:");
        println!(
            "  distinct cells touched : {cells} of {} ({:.2}% of matrix)",
            // matrix entry count inferred from the largest index seen
            touched.last().map(|&m| m as usize + 1).unwrap_or(0),
            100.0 * cells as f64 / touched.last().map(|&m| m as f64 + 1.0).unwrap_or(1.0)
        );
        println!(
            "  64B-line footprint     : {lines64} lines = {:.2} MiB  (Zen4/Intel model)",
            lines64 as f64 * 64.0 / (1 << 20) as f64
        );
        println!(
            "  128B-line footprint    : {lines128} lines = {:.2} MiB  (Apple M model)",
            lines128 as f64 * 128.0 / (1 << 20) as f64
        );
        println!("# cache fit:  M4 L2=16MB  |  Zen4 L2=1MB, L3=32MB(per CCD)  |  Zen4 L1d=32KB, M4 L1d=128KB");
        return;
    }

    // Warm up every condition (fault in pages, reach steady frequency).
    for (_, fp) in &conditions {
        apply(*fp);
        tokenize_pass(&mut tok);
        tokenize_pass(&mut tok);
    }

    // Per-trial interleave: rotate the starting condition each trial.
    let mut samples: Vec<Vec<f64>> = vec![Vec::with_capacity(trials); nconds];
    for t in 0..trials {
        for k in 0..nconds {
            let ci = (t + k) % nconds;
            apply(conditions[ci].1);
            let start = Instant::now();
            tokenize_pass(&mut tok);
            let ms = start.elapsed().as_secs_f64() * 1e3;
            samples[ci].push(ms);
        }
    }

    fn median(s: &mut [f64]) -> f64 {
        s.sort_by(|a, b| a.partial_cmp(b).unwrap());
        s[s.len() / 2]
    }
    fn cv(s: &[f64]) -> f64 {
        let m = s.iter().sum::<f64>() / s.len() as f64;
        let v = s.iter().map(|x| (x - m).powi(2)).sum::<f64>() / s.len() as f64;
        v.sqrt() / m * 100.0
    }

    println!("# config: {}", config_path.display());
    println!(
        "# sentences: {}, chars: {}, trials: {}, path: pipelined+prefetch, do_tokenize only",
        lines.len(),
        total_chars,
        trials
    );
    let nspc = |ms: f64| ms * 1e6 / total_chars as f64;
    let base_med = median(&mut samples[0].clone());
    println!(
        "{:<16} median {:>7.2} ns/char  cv {:>4.1}%   (reference)",
        conditions[0].0,
        nspc(base_med),
        cv(&samples[0])
    );
    for ci in 1..nconds {
        let med = median(&mut samples[ci].clone());
        // Paired sign test vs baseline: in how many trials did this condition beat baseline?
        let wins = (0..trials)
            .filter(|&t| samples[ci][t] < samples[0][t])
            .count();
        println!(
            "{:<16} median {:>7.2} ns/char  cv {:>4.1}%   delta {:+5.1}%   beats baseline {}/{} trials",
            conditions[ci].0,
            nspc(med),
            cv(&samples[ci]),
            (med / base_med - 1.0) * 100.0,
            wins,
            trials
        );
    }
}
