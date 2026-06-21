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

//! Issue #117 CALIBRATION: dependent-load (pointer-chase) latency of this machine
//! as a function of working-set size. A single random cycle over an array of N
//! u64 is chased serially, so each load depends on the previous — exactly the
//! double-array trie walk's access pattern, with NO memory-level parallelism.
//! The ns/load at each size names the cache level. Placing the measured trie walk
//! (~3.76 ns/load over a 7.76 MiB working set, from `trie_workingset`) on this
//! curve proves which level the walk is bound by, and thus the absolute ceiling
//! of any prefetch-friendly trie layout.
//!
//! Deterministic (LCG shuffle, no RNG). SUDACHI_CHASE_ACCESSES (per size).

use std::time::Instant;

fn main() {
    let accesses: u64 = std::env::var("SUDACHI_CHASE_ACCESSES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200_000_000);

    // Working-set sizes spanning L1 (128 KiB) .. L2 (16 MiB) .. DRAM, on M4.
    let sizes_kib: [usize; 11] = [8, 32, 64, 128, 256, 1024, 4096, 8192, 16384, 65536, 262144];

    println!("# dependent pointer-chase latency (single random cycle, no MLP)");
    println!("# working-set     ns/load     note");
    for &kib in &sizes_kib {
        let n = (kib * 1024) / std::mem::size_of::<u64>();
        if n < 2 {
            continue;
        }
        // Build a single cycle covering all n slots: start from identity, shuffle
        // with a deterministic LCG (Sattolo-style → guaranteed single cycle).
        let mut next: Vec<u64> = (0..n as u64).collect();
        let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
        for i in (1..n).rev() {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let j = (state >> 33) as usize % i; // 0 <= j < i  (Sattolo: strict)
            next.swap(i, j);
        }
        // Turn the permutation into a linked cycle: pos -> next_pos.
        // `next` is currently a permutation; build the chase array `link` where
        // link[p] = the successor of p along the single cycle.
        let mut link = vec![0u64; n];
        for w in 0..n {
            link[next[w] as usize] = next[(w + 1) % n];
        }

        // Warm, then time `accesses` dependent loads.
        let mut p = 0u64;
        let warm = (n as u64).min(accesses);
        for _ in 0..warm {
            p = link[p as usize];
        }
        let start = Instant::now();
        for _ in 0..accesses {
            p = link[p as usize];
        }
        let ns = start.elapsed().as_secs_f64() * 1e9 / accesses as f64;
        std::hint::black_box(p);

        let note = if kib <= 128 {
            "L1"
        } else if kib <= 16384 {
            "L2"
        } else {
            "L3/DRAM"
        };
        let label = if kib >= 1024 {
            format!("{} MiB", kib / 1024)
        } else {
            format!("{kib} KiB")
        };
        println!("  {label:>10}   {ns:>8.2}    {note}");
    }
    println!("# (trie walk measured ~3.76 ns/load over a 7.76 MiB working set)");
}
