# Speeding up sudachi.rs: what we measured — executive summary

*One-page summary (full study: `optimization-study.md`; maintainer proposal: `paradigm-proposal.md`). All figures [meas] on an Apple M4 Max; corpus UD_Japanese-GSD (543 sentences / 21,328 codepoints / 13,034 SUW tokens). Provenance: [meas] measured by us, [doc] prior measurements, [lit] peer-reviewed literature.*

## TL;DR

Can the Sudachi hot path (double-array trie + Viterbi over a 71 MB connection matrix) be made 3× faster? **Not while staying compatible with Java Sudachi.** sudachi.rs contractually reproduces Java-Sudachi output (A/B/C granularity, exact POS and segmentation) — that compatibility is its reason to exist. Under that constraint:

- the **byte-identical ceiling is ≈ 1.35×** (prefetch + varint + #39 + #40 ≈ 9% **+ H7b +20–21%**, #52); the lattice *kernel* is at its floor, but WordInfo materialization wasn't (H7);
- **throughput comes only from multithreading (~8.9×)**, which preserves compatibility;
- a **pointwise paradigm (Vaporetto + dict) is ~3.7×** (real fused binary, measured) **but breaks Java compatibility** (no A/B/C, POS −1.74, different segmentation) → not an option for sudachi.rs itself, only a separate tool.

As a by-product we built the **first golden accuracy benchmark for Sudachi** (mode A: SEG-F1 **97.99**, POS-top **97.46**).

## Results map (full output seg+POS+normalized, GSD)

| approach | ns/char | × | Java-compatible? |
|---|---:|---:|:---:|
| Sudachi lattice (prefetch+varint+#39+#40+H7b) | ~290 | 1.0× (ceiling ~1.35×) | ✅ yes |
| + multithreading (orthogonal) | — | ~8.9× [doc] | ✅ yes |
| **Vaporetto+dict (real fused, warm cache)** | **~78** | **~3.7×** | ❌ **no** |
| Vaporetto bare seg+POS (no normalized) | ~71 | ~4.1× | ❌ no |
| Vaporetto seg-only (no features) | ~27 | ~10.7× | ❌ (output useless alone) |

## Three findings

1. **The byte-identical ceiling is ~1.35× — and the lattice kernel is not where the lever was.** prefetch (+16.7%) + varint (+2.7%) shipped; this study added #39 (gate the `normalized_form` decode in JoinNumeric) and #40 (WordInfoParser early-out), **~9%**; **and H7 (#52) — sharing `WordInfo` via an `Arc`-cache — another +20–21%** (WordInfo materialization is ~17% of `do_tokenize`; the single largest byte-identical lever, byte-identical across A/B/C × 2 corpora). The lattice *kernel* is the wall: an 18-agent analysis (§5.11) proved the **connection matrix is L2-resident, not DRAM-bound** (3.77 MiB working set, 0 capacity misses, ≤7.2% of `do_tokenize`), codegen optimal — **do not touch the matrix/lattice** (explains why #24 prefetch hurt −12%). The real headroom was in materialization, not the kernel.

2. **The pointwise paradigm, measured for real, is ~3.7× — not 6.5×.** We built a runnable fused Vaporetto+dict binary (§5.12). The earlier 6.5–8× projection mixed build profiles and used a Vaporetto number that never materialized POS tags; the real seg+POS is ~71 ns/char (not 36), and the full replacement is **~78 ns/char = ~3.7×**. Robust at 71–113 ns/char (GSD-test/dev/kyoto-leads). Quality: SEG −0.92, POS-top −1.74 (hybrid: dictionary POS candidates disambiguated by Vaporetto's POS; taking the first dict entry alone gives POS ≈ 70 — a homograph trap).

3. **Java compatibility is the binding (decisive) constraint.** sudachi.rs output is locked to Java Sudachi: A/B/C from one pass, exact POS, exact boundaries. The paradigm changes all of these → it **cannot be a default or a "replacement" inside sudachi.rs**; at best a separate opt-out tool for users who do not need Java compatibility. Realistically the maintainer will decline it for the main repository.

## Production & negative lessons

- **Decode cache under multithreading (measured, 12 cores):** a global `Mutex` **collapses to 0.3×** (lock convoy); `DashMap` gives 1.3×; **`thread_local` scales 8.6–9.5×** → any production cache must be per-thread.
- **Dead ends (§5.4, §5.11):** the connection matrix is L2-resident and at its floor (not DRAM/volume-bound — premise corrected); the trie-layout tricks we tested (vEB, code-dividing, first-char sidecar) are dead/counterproductive for Japanese, and a charwise trie is +1.37× isolated but only ~+6–10% e2e and changes the format. **NB:** a corpus-frequency prefetch relayout of the double-array itself (the maintainer's #117 idea) was **not prototyped on Sudachi** — only assessed from the literature (N7/N8: ≤1.10× lookup for ~14× build) — and remains an open direction rather than a measured dead end.

## Recommendations for the maintainer

1. **Ship exact:** prefetch + varint + #39 + #40 **+ H7b (#52, +20–21%)** (byte-identical, Java-compatible) → ~1.35× ceiling. For throughput, **multithreading (~8.9×)** — the safest large win.
2. **Do not touch the matrix/lattice** — proven at its floor (§5.11); a regression guard is the nomat probe + LRU sim.
3. **Keep the paradigm out of sudachi.rs** (it breaks Java compatibility). If a throughput-oriented tool without A/B/C is ever wanted, that is a separate project — the numbers and a runnable prototype are ready (`paradigm-proposal.md`, `/tmp/vapo-sud`).
