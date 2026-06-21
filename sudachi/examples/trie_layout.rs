/*
 * PROBE (not for upstream). Tests the HEADROOM of eiennohito's #117 idea — a
 * prefetch-friendlier / cache-friendlier trie LAYOUT. We cannot relayout a
 * compressed double-array in place (overlapping base blocks => needs a full
 * rebuild, the note's ~14x build cost), but we can bound the best-case payoff:
 *
 *   1. record the exact sequence of trie array indices the corpus walk loads;
 *   2. report the working set (does the trie fit a cache level?);
 *   3. pointer-chase the touched nodes (a) at their real scattered addresses and
 *      (b) packed contiguously. The packed chase is the best ANY layout could
 *      do (perfect locality). scattered/packed latency ratio = the ceiling on
 *      what trie relayout could buy for the dependent-load chain.
 */
use std::collections::HashSet;
use std::hint::black_box;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use sudachi::analysis::mlist::MorphemeList;
use sudachi::analysis::stateful_tokenizer::StatefulTokenizer;
use sudachi::analysis::Mode;
use sudachi::config::Config;
use sudachi::dic::dictionary::JapaneseDictionary;
use sudachi::dic::lexicon::trie::{trie_rec_begin, trie_rec_end};

fn env_path(key: &str, default: &str) -> PathBuf {
    std::env::var_os(key)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default))
}

fn chase(buf: &[u32], start: usize, steps: usize) -> (f64, u64) {
    // warm
    let mut cur = start;
    for _ in 0..(steps / 10).max(1) {
        cur = buf[cur] as usize;
    }
    let mut best = f64::MAX;
    let mut sink = 0u64;
    for _ in 0..5 {
        let mut cur = start;
        let t = Instant::now();
        for _ in 0..steps {
            cur = buf[cur] as usize;
        }
        let ns = t.elapsed().as_secs_f64() * 1e9 / steps as f64;
        sink = sink.wrapping_add(cur as u64);
        best = best.min(ns);
    }
    (best, sink)
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

    let inputs = env_path(
        "SUDACHI_BENCH_INPUTS",
        "target/issue-117-corpora/kyoto-leads.txt",
    );
    let text = std::fs::read_to_string(&inputs).expect("inputs");
    let lines: Vec<&str> = text.lines().collect();

    let mut tok = StatefulTokenizer::new(dict.clone(), Mode::C);
    tok.set_pipelined_lookup(true);
    let mut result = MorphemeList::empty(dict.clone());

    // record the trie load sequence over the whole corpus
    trie_rec_begin();
    for line in &lines {
        tok.reset().push_str(line);
        tok.do_tokenize().expect("tokenize");
        result.collect_results(&mut tok).expect("collect");
    }
    let seq = trie_rec_end();
    let total = seq.len();
    let maxidx = *seq.iter().max().unwrap() as usize;

    // working set
    let mut seen = HashSet::new();
    let mut distinct: Vec<u32> = Vec::new();
    for &x in &seq {
        if seen.insert(x) {
            distinct.push(x);
        }
    }
    let n = distinct.len();
    let lines64: HashSet<u32> = distinct.iter().map(|&i| (i * 4) / 64).collect();
    let lines128: HashSet<u32> = distinct.iter().map(|&i| (i * 4) / 128).collect();
    println!(
        "# trie: array span ~{} MiB (max index {})",
        (maxidx * 4) >> 20,
        maxidx
    );
    println!("## working set over corpus");
    println!(
        "  trie loads total      : {total}  ({:.2} loads/char)",
        total as f64 / 461815.0
    );
    println!("  distinct nodes touched: {n}");
    println!(
        "  64B-line footprint    : {} lines = {:.2} MiB  (Zen4/Intel)",
        lines64.len(),
        lines64.len() as f64 * 64.0 / (1 << 20) as f64
    );
    println!(
        "  128B-line footprint   : {} lines = {:.2} MiB  (Apple M)",
        lines128.len(),
        lines128.len() as f64 * 128.0 / (1 << 20) as f64
    );
    println!("# cache: M4 L2=16MB L1d=128KB | Zen4 L2=1MB L3=32MB L1d=32KB");

    // pointer chase: scattered (real addresses) vs packed (contiguous), same
    // dependent-load chain length, over the distinct touched nodes.
    let steps = total; // same number of dependent loads as the real walk
                       // A single RANDOM visit order, used by BOTH chases, so neither benefits from
                       // sequential hardware prefetch — the only difference is the address span
                       // (scattered 68 MiB real vs packed ~1 MiB contiguous).
    let mut order: Vec<u32> = (0..n as u32).collect();
    let mut st: u64 = 0x9e3779b97f4a7c15;
    for i in (1..n).rev() {
        st = st
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let j = (st >> 33) as usize % (i + 1);
        order.swap(i, j);
    }
    let mut scattered = vec![0u32; maxidx + 1];
    let mut packed = vec![0u32; n];
    for i in 0..n {
        let a = order[i] as usize;
        let b = order[(i + 1) % n] as usize;
        scattered[distinct[a] as usize] = distinct[b];
        packed[a] = b as u32;
    }
    let (s_ns, s1) = chase(&scattered, distinct[order[0] as usize] as usize, steps);
    let (p_ns, p2) = chase(&packed, order[0] as usize, steps);
    black_box(s1.wrapping_add(p2));

    println!("## pointer-chase latency (dependent loads, {steps} steps)");
    println!("  scattered (real layout) : {s_ns:.2} ns/load");
    println!("  packed   (ideal layout) : {p_ns:.2} ns/load");
    println!(
        "  layout headroom (scattered/packed): {:.2}x  ({:.1}% of trie-load time removable)",
        s_ns / p_ns,
        100.0 * (s_ns - p_ns) / s_ns
    );
}
