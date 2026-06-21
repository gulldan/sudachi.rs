/*
 * PROBE (not for upstream). Directly tests the COMPRESSIBILITY of the
 * connection matrix, which is the mathematical content of "relearn it with L1
 * regularization": if the matrix is well explained by an additive model
 * a[l]+b[r] plus a low-rank interaction, then a compact regularized model with
 * few parameters exists (=> fits cache). If the singular spectrum decays
 * slowly, no regularization can shrink it without large error.
 *
 * Reports: energy explained by the additive model, the singular-value spectrum
 * of the residual (randomized SVD), cumulative energy vs rank, and — for
 * SUDACHI_LOWRANK_R — the actual tokenization output diff when the matrix is
 * replaced by its rank-R reconstruction.
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
use sudachi::dic::connect::install_replace;
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

// Gram-Schmidt orthonormalize the L columns of an (n x L) column-major-ish
// matrix stored row-major as q[i*l + c].
fn orthonormalize(q: &mut [f32], n: usize, l: usize) {
    for c in 0..l {
        for d in 0..c {
            let mut dot = 0f64;
            for i in 0..n {
                dot += (q[i * l + c] * q[i * l + d]) as f64;
            }
            let dot = dot as f32;
            for i in 0..n {
                q[i * l + c] -= dot * q[i * l + d];
            }
        }
        let mut nrm = 0f64;
        for i in 0..n {
            nrm += (q[i * l + c] * q[i * l + c]) as f64;
        }
        let nrm = (nrm.sqrt() as f32).max(1e-12);
        for i in 0..n {
            q[i * l + c] /= nrm;
        }
    }
}

// Cyclic Jacobi eigen-decomposition of a symmetric LxL matrix `a` (row-major).
// Returns (eigenvalues, eigenvectors as columns in `vec` row-major). Descending.
fn jacobi(a: &mut [f64], l: usize) -> (Vec<f64>, Vec<f64>) {
    let mut v = vec![0f64; l * l];
    for i in 0..l {
        v[i * l + i] = 1.0;
    }
    for _ in 0..60 {
        let mut off = 0f64;
        for p in 0..l {
            for q in (p + 1)..l {
                off += a[p * l + q] * a[p * l + q];
            }
        }
        if off < 1e-12 {
            break;
        }
        for p in 0..l {
            for q in (p + 1)..l {
                let apq = a[p * l + q];
                if apq.abs() < 1e-15 {
                    continue;
                }
                let app = a[p * l + p];
                let aqq = a[q * l + q];
                let phi = 0.5 * (aqq - app).atan2(2.0 * apq);
                let (s, c) = phi.sin_cos();
                for k in 0..l {
                    let akp = a[k * l + p];
                    let akq = a[k * l + q];
                    a[k * l + p] = c * akp - s * akq;
                    a[k * l + q] = s * akp + c * akq;
                }
                for k in 0..l {
                    let apk = a[p * l + k];
                    let aqk = a[q * l + k];
                    a[p * l + k] = c * apk - s * aqk;
                    a[q * l + k] = s * apk + c * aqk;
                }
                for k in 0..l {
                    let vkp = v[k * l + p];
                    let vkq = v[k * l + q];
                    v[k * l + p] = c * vkp - s * vkq;
                    v[k * l + q] = s * vkp + c * vkq;
                }
            }
        }
    }
    let mut idx: Vec<usize> = (0..l).collect();
    let eig: Vec<f64> = (0..l).map(|i| a[i * l + i]).collect();
    idx.sort_by(|&i, &j| eig[j].partial_cmp(&eig[i]).unwrap());
    let evals: Vec<f64> = idx.iter().map(|&i| eig[i].max(0.0)).collect();
    let mut evecs = vec![0f64; l * l];
    for (newc, &oldc) in idx.iter().enumerate() {
        for k in 0..l {
            evecs[k * l + newc] = v[k * l + oldc];
        }
    }
    (evals, evecs)
}

fn capture(
    tok: &mut StatefulTokenizer<Arc<JapaneseDictionary>>,
    result: &mut MorphemeList<Arc<JapaneseDictionary>>,
    lines: &[&str],
) -> Vec<Vec<(u32, u32, u64)>> {
    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        tok.reset().push_str(line);
        tok.do_tokenize().expect("tokenize");
        result.collect_results(tok).expect("collect");
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
    let config =
        Config::new(Some(config_path.clone()), resource_dir, dict_override).expect("config");
    let dict = Arc::new(JapaneseDictionary::from_cfg(&config).expect("dict"));

    let inputs_path = env_path(
        "SUDACHI_BENCH_INPUTS",
        "target/issue-117-corpora/kyoto-leads.txt",
    );
    let text = std::fs::read_to_string(&inputs_path).expect("inputs");
    let lines: Vec<&str> = text.lines().collect();

    let mut tok = StatefulTokenizer::new(dict.clone(), Mode::C);
    tok.set_pipelined_lookup(true);
    let mut result = MorphemeList::empty(dict.clone());

    // baseline output BEFORE we install any replacement
    let baseline = capture(&mut tok, &mut result, &lines);

    let conn = dict.grammar().conn_matrix();
    let nl = conn.num_left();
    let nr = conn.num_right();

    // Load M row-major by left: mp[l*nr + r]
    println!("# loading matrix {nl}x{nr} ...");
    let mut mp = vec![0f32; nl * nr];
    for l in 0..nl {
        for r in 0..nr {
            mp[l * nr + r] = conn.cost(l as u16, r as u16) as f32;
        }
    }
    // total variance around grand mean
    let grand: f64 = mp.iter().map(|&x| x as f64).sum::<f64>() / (nl * nr) as f64;
    let tvar: f64 = mp.iter().map(|&x| (x as f64 - grand).powi(2)).sum();

    // additive model a[l]+b[r] via double-centering (a few alternations)
    let mut a = vec![0f64; nl];
    let mut b = vec![0f64; nr];
    for _ in 0..3 {
        for l in 0..nl {
            let mut s = 0f64;
            for r in 0..nr {
                s += mp[l * nr + r] as f64 - b[r];
            }
            a[l] = s / nr as f64 - grand;
        }
        for r in 0..nr {
            let mut s = 0f64;
            for l in 0..nl {
                s += mp[l * nr + r] as f64 - a[l];
            }
            b[r] = s / nl as f64;
        }
    }
    // residual M' = M - (a[l]+b[r]); also fro^2
    let mut fro2 = 0f64;
    for l in 0..nl {
        for r in 0..nr {
            let v = mp[l * nr + r] as f64 - (a[l] + b[r]);
            mp[l * nr + r] = v as f32;
            fro2 += v * v;
        }
    }
    let additive_explained = 1.0 - fro2 / tvar;
    println!("## additive model a[l]+b[r]");
    println!(
        "  explains {:.1}% of total matrix variance (footprint 2x{nl} i16 = {} KiB)",
        100.0 * additive_explained,
        nl * 2 * 2 / 1024
    );

    // randomized SVD of residual M' (nl x nr)
    let cap = env_usize("SUDACHI_LOWRANK_L", 64);
    let l = cap.min(nl).min(nr);
    // init Q = strided columns of M'
    let mut q = vec![0f32; nl * l];
    for c in 0..l {
        let rcol = c * nr / l;
        for i in 0..nl {
            q[i * l + c] = mp[i * nr + rcol];
        }
    }
    orthonormalize(&mut q, nl, l);
    let mut w = vec![0f32; nr * l];
    for _ in 0..2 {
        // W = M'^T Q  (nr x l)
        for x in w.iter_mut() {
            *x = 0.0;
        }
        for li in 0..nl {
            let base = li * nr;
            let qb = li * l;
            for r in 0..nr {
                let mv = mp[base + r];
                if mv != 0.0 {
                    let wb = r * l;
                    for c in 0..l {
                        w[wb + c] += mv * q[qb + c];
                    }
                }
            }
        }
        // Q = M' W (nl x l)
        for x in q.iter_mut() {
            *x = 0.0;
        }
        for li in 0..nl {
            let base = li * nr;
            let qb = li * l;
            for r in 0..nr {
                let mv = mp[base + r];
                let wb = r * l;
                for c in 0..l {
                    q[qb + c] += mv * w[wb + c];
                }
            }
        }
        orthonormalize(&mut q, nl, l);
    }
    // B = Q^T M' (l x nr); G = B B^T (l x l)
    let mut bmat = vec![0f32; l * nr];
    for li in 0..nl {
        let base = li * nr;
        let qb = li * l;
        for r in 0..nr {
            let mv = mp[base + r];
            if mv != 0.0 {
                for c in 0..l {
                    bmat[c * nr + r] += q[qb + c] * mv;
                }
            }
        }
    }
    let mut g = vec![0f64; l * l];
    for c in 0..l {
        for d in c..l {
            let mut s = 0f64;
            for r in 0..nr {
                s += (bmat[c * nr + r] * bmat[d * nr + r]) as f64;
            }
            g[c * l + d] = s;
            g[d * l + c] = s;
        }
    }
    let (evals, _evecs) = jacobi(&mut g, l);
    println!("## residual singular spectrum (randomized SVD, L={l})");
    let mut cum = 0f64;
    let ranks = [1usize, 2, 4, 8, 16, 32, 48, 64];
    let mut energy_at = vec![];
    for (i, &ev) in evals.iter().enumerate() {
        cum += ev;
        if ranks.contains(&(i + 1)) {
            energy_at.push((i + 1, cum / fro2));
        }
    }
    for (k, frac) in &energy_at {
        let resid_total = additive_explained + (1.0 - additive_explained) * frac;
        let kib = (nl + nr) * (k + 1) * 2 / 1024;
        println!(
            "  rank {:>2}: residual energy {:>5.1}%  | additive+rank{} explains {:>5.1}% of matrix  | factor footprint ~{} KiB",
            k,
            100.0 * frac,
            k,
            100.0 * resid_total,
            kib
        );
    }

    // Optional: reconstruct rank-R, replace matrix, measure output diff.
    if let Ok(rs) = std::env::var("SUDACHI_LOWRANK_R") {
        let rr: usize = rs.parse().unwrap_or(16);
        let r = rr.min(l);
        // top-r left vectors: U_r = Q * E_r ; reconstruction M'_R = U_r U_r^T M' = Q E_r E_r^T B
        let (_e2, evecs) = {
            // recompute G eig to get vectors (g was consumed); rebuild G
            let mut g2 = vec![0f64; l * l];
            for c in 0..l {
                for d in c..l {
                    let mut s = 0f64;
                    for rr2 in 0..nr {
                        s += (bmat[c * nr + rr2] * bmat[d * nr + rr2]) as f64;
                    }
                    g2[c * l + d] = s;
                    g2[d * l + c] = s;
                }
            }
            jacobi(&mut g2, l)
        };
        // ErtB = E_r^T B  (r x nr)
        let mut ertb = vec![0f32; r * nr];
        for rr2 in 0..nr {
            for i in 0..r {
                let mut s = 0f64;
                for c in 0..l {
                    s += evecs[c * l + i] * bmat[c * nr + rr2] as f64;
                }
                ertb[i * nr + rr2] = s as f32;
            }
        }
        // recon[l,r] = a[l]+b[r] + sum_i (Q E_r)[l,i] * ErtB[i,r]
        // (Q E_r)[l,i] = sum_c q[l,c] evecs[c,i]
        let mut repl = vec![0i16; nl * nr];
        let mut qe = vec![0f32; l];
        for li in 0..nl {
            for i in 0..r {
                let mut s = 0f64;
                for c in 0..l {
                    s += q[li * l + c] as f64 * evecs[c * l + i];
                }
                qe[i] = s as f32;
            }
            let base = li * nr;
            for rr2 in 0..nr {
                let mut inter = 0f32;
                for i in 0..r {
                    inter += qe[i] * ertb[i * nr + rr2];
                }
                let val = a[li] + b[rr2] + inter as f64;
                repl[base + rr2] = val.round().clamp(-32768.0, 32767.0) as i16;
            }
        }
        // index layout in matrix is data[right*nl + left]; our repl is [left*nr+right].
        // install_replace expects the matrix layout (right*nl+left), so transpose.
        let mut repl_t = vec![0i16; nl * nr];
        for left in 0..nl {
            for right in 0..nr {
                repl_t[right * nl + left] = repl[left * nr + right];
            }
        }
        install_replace(repl_t);
        let approx = capture(&mut tok, &mut result, &lines);
        let mut btok = 0u64;
        let mut preserved = 0u64;
        let mut ident = 0u64;
        for (bl, ap) in baseline.iter().zip(approx.iter()) {
            btok += bl.len() as u64;
            let aset: HashSet<(u32, u32, u64)> = ap.iter().cloned().collect();
            let mut all = true;
            for t in bl {
                if aset.contains(t) {
                    preserved += 1;
                } else {
                    all = false;
                }
            }
            if all && ap.len() == bl.len() {
                ident += 1;
            }
        }
        println!("## rank-{r} reconstruction output diff");
        println!(
            "  tokens preserved : {:.2}%",
            100.0 * preserved as f64 / btok as f64
        );
        println!(
            "  sentences identical: {:.2}%",
            100.0 * ident as f64 / baseline.len() as f64
        );
    }
}
