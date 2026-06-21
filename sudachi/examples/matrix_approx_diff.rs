/*
 * PROBE (not for upstream). Tests the OUTPUT half of "relearn / simplify the
 * connection matrix". We emulate "fewer effective connection classes" by
 * co-clustering left/right connection ids into K classes (k-means) and reading
 * cost from a KxK block matrix. KxK is chosen to fit a cache level (K=256 ->
 * 128 KiB = L1d on M4, which is exactly what buys the measured ~7-9%). We then
 * measure how much tokenization OUTPUT changes vs the real matrix.
 *
 * Positive result for eiennohito would be: output barely changes at a K that
 * fits L1 (=> the matrix is compressible, speed is reachable cheaply).
 * Negative result: output changes a lot (=> the matrix is information-dense).
 */
use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
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
                    let diff = p[j] - c[j];
                    d += diff * diff;
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

fn capture(
    tok: &mut StatefulTokenizer<Arc<JapaneseDictionary>>,
    result: &mut MorphemeList<Arc<JapaneseDictionary>>,
    lines: &[&str],
) -> Vec<Vec<(u32, u32, u64)>> {
    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        tok.reset().push_str(line);
        tok.do_tokenize().expect("tokenize failed");
        result.collect_results(tok).expect("collect failed");
        let mut toks = Vec::with_capacity(result.len());
        for i in 0..result.len() {
            let m = result.get(i);
            let mut h = DefaultHasher::new();
            m.surface().as_bytes().hash(&mut h);
            toks.push((m.begin_c() as u32, m.end_c() as u32, h.finish()));
        }
        out.push(toks);
    }
    out
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

    let k = env_usize("SUDACHI_APPROX_K", 256);
    let sample = env_usize("SUDACHI_APPROX_SAMPLE", 256);
    let iters = env_usize("SUDACHI_APPROX_ITERS", 12);

    let mut tok = StatefulTokenizer::new(dict.clone(), Mode::C);
    tok.set_pipelined_lookup(true);
    let mut result = MorphemeList::empty(dict.clone());

    // 1. baseline output with the real matrix (approx still off)
    let baseline = capture(&mut tok, &mut result, &lines);

    // 2. build the approximation reading the REAL costs
    let conn = dict.grammar().conn_matrix();
    let nl = conn.num_left();
    let nr = conn.num_right();
    let sampled_l: Vec<usize> = (0..nl).step_by((nl / sample).max(1)).take(sample).collect();
    let sampled_r: Vec<usize> = (0..nr).step_by((nr / sample).max(1)).take(sample).collect();
    let rpoints: Vec<Vec<f32>> = (0..nr)
        .map(|r| {
            sampled_l
                .iter()
                .map(|&l| conn.cost(l as u16, r as u16) as f32)
                .collect()
        })
        .collect();
    let clr = kmeans(&rpoints, k, iters);
    let lpoints: Vec<Vec<f32>> = (0..nl)
        .map(|l| {
            sampled_r
                .iter()
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
            let v = conn.cost(l as u16, r as u16);
            bsum[a * kr + b] += v as i64;
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

    // 3. install approximation and re-tokenize
    install_approx(ApproxModel {
        cll,
        clr,
        block,
        kr,
    });
    let approx = capture(&mut tok, &mut result, &lines);

    // 4. diff segmentation
    let mut btok = 0u64;
    let mut atok = 0u64;
    let mut preserved = 0u64;
    let mut ident = 0u64;
    for (b, a) in baseline.iter().zip(approx.iter()) {
        btok += b.len() as u64;
        atok += a.len() as u64;
        let aset: HashSet<(u32, u32, u64)> = a.iter().cloned().collect();
        let mut all = true;
        for t in b {
            if aset.contains(t) {
                preserved += 1;
            } else {
                all = false;
            }
        }
        if all && a.len() == b.len() {
            ident += 1;
        }
    }
    let block_kib = (kl * kr * 2) as f64 / 1024.0;
    println!("# approx: K={k} classes (block {kl}x{kr} = {block_kib:.0} KiB), sample={sample}, iters={iters}");
    println!(
        "# block fits: L1d(M4 128KiB)={}  L1d(Zen4 32KiB)={}",
        if block_kib <= 128.0 { "yes" } else { "no" },
        if block_kib <= 32.0 { "yes" } else { "no" }
    );
    println!("  baseline tokens : {btok}");
    println!(
        "  approx tokens   : {atok}  ({:+.2}%)",
        100.0 * (atok as f64 - btok as f64) / btok as f64
    );
    println!(
        "  tokens preserved: {preserved}/{btok}  = {:.2}%  (exact begin/end/surface match)",
        100.0 * preserved as f64 / btok as f64
    );
    println!(
        "  sentences identical: {ident}/{}  = {:.2}%",
        baseline.len(),
        100.0 * ident as f64 / baseline.len() as f64
    );
}
