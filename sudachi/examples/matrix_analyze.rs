/*
 * PROBE (not for upstream). Measures the *redundancy* of the connection matrix:
 * value distribution, exact and coarse row duplication, and the value
 * concentration of the cells that natural text actually touches. This tests the
 * load-bearing premise of "relearn / simplify the matrix" (eiennohito): is there
 * noise / redundancy that a sparser CRF could drop, and how concentrated is the
 * real working set?
 */
use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;

use sudachi::analysis::stateful_tokenizer::StatefulTokenizer;
use sudachi::analysis::Mode;
use sudachi::config::Config;
use sudachi::dic::connect::{probe_record_begin, probe_record_end};
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

    let conn = dict.grammar().conn_matrix();
    let nl = conn.num_left();
    let nr = conn.num_right();
    let total = (nl * nr) as f64;
    println!("# matrix: {nl} left x {nr} right = {} cells", nl * nr);

    // ---- H-A.1: global value distribution -------------------------------
    let mut hist = vec![0u64; 65536];
    let mut min = i16::MAX;
    let mut max = i16::MIN;
    for r in 0..nr {
        for l in 0..nl {
            let v = conn.cost(l as u16, r as u16);
            hist[v as u16 as usize] += 1;
            min = min.min(v);
            max = max.max(v);
        }
    }
    let distinct = hist.iter().filter(|&&c| c > 0).count();
    let zeros = hist[0];
    let inhibited = hist[i16::MAX as u16 as usize];
    let mut top: Vec<(i16, u64)> = hist
        .iter()
        .enumerate()
        .filter(|(_, &c)| c > 0)
        .map(|(i, &c)| (i as u16 as i16, c))
        .collect();
    top.sort_by(|a, b| b.1.cmp(&a.1));
    println!("## H-A.1 global value distribution");
    println!("  distinct values : {distinct} of 65536 possible");
    println!("  value range     : [{min}, {max}]");
    println!(
        "  == 0            : {zeros} ({:.1}%)",
        100.0 * zeros as f64 / total
    );
    println!(
        "  == INHIBITED    : {inhibited} ({:.1}%)",
        100.0 * inhibited as f64 / total
    );
    let mut cum = 0u64;
    print!("  top-8 values    : ");
    for (v, c) in top.iter().take(8) {
        cum += *c;
        print!("{v}({:.1}%) ", 100.0 * *c as f64 / total);
    }
    println!(
        "\n  top-8 cover     : {:.1}% of all cells",
        100.0 * cum as f64 / total
    );

    // ---- H-A.2: row redundancy (exact and coarse) -----------------------
    // Row r = [cost(l, r) for l]. Coarse buckets quantize the row to +/- q so
    // "noisy but effectively equal" rows collapse together (what an L1-sparse
    // model would merge).
    let hash_row = |q: i16| -> usize {
        let mut seen = HashSet::new();
        let mut row = vec![0i16; nl];
        for r in 0..nr {
            for l in 0..nl {
                let v = conn.cost(l as u16, r as u16);
                row[l] = if q > 1 { v / q } else { v };
            }
            let mut h = DefaultHasher::new();
            row.hash(&mut h);
            seen.insert(h.finish());
        }
        seen.len()
    };
    println!("## H-A.2 row redundancy (of {nr} rows)");
    println!("  distinct rows exact      : {}", hash_row(1));
    println!("  distinct rows quantized/8 : {}", hash_row(8));
    println!("  distinct rows quantized/64: {}", hash_row(64));

    // ---- H-B: value concentration of the real working set ---------------
    let inputs_path = env_path(
        "SUDACHI_BENCH_INPUTS",
        "target/issue-117-corpora/kyoto-leads.txt",
    );
    let text = std::fs::read_to_string(&inputs_path).expect("failed to read inputs");
    let lines: Vec<&str> = text.lines().collect();
    let mut tok = StatefulTokenizer::new(dict.clone(), Mode::C);
    tok.set_pipelined_lookup(true);
    probe_record_begin();
    for line in &lines {
        tok.reset().push_str(line);
        tok.do_tokenize().expect("tokenize failed");
    }
    let touched = probe_record_end();
    let mut whist = vec![0u64; 65536];
    for &idx in &touched {
        let idx = idx as usize;
        let l = idx % nl;
        let r = idx / nl;
        let v = conn.cost(l as u16, r as u16);
        whist[v as u16 as usize] += 1;
    }
    let wdistinct = whist.iter().filter(|&&c| c > 0).count();
    let wtotal = touched.len() as f64;
    let mut wtop: Vec<(i16, u64)> = whist
        .iter()
        .enumerate()
        .filter(|(_, &c)| c > 0)
        .map(|(i, &c)| (i as u16 as i16, c))
        .collect();
    wtop.sort_by(|a, b| b.1.cmp(&a.1));
    println!(
        "## H-B working-set value concentration ({} distinct cells touched)",
        touched.len()
    );
    println!("  distinct values touched : {wdistinct}");
    let mut wcum = 0u64;
    let mut n_for_90 = 0;
    for (i, (_, c)) in wtop.iter().enumerate() {
        wcum += *c;
        if wcum as f64 / wtotal >= 0.90 {
            n_for_90 = i + 1;
            break;
        }
    }
    println!("  values covering 90% of touched cells: {n_for_90}");
}
