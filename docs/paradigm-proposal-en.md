# sudachi.rs performance: two paths, side by side (for the maintainer)

> **Verdict.** Within sudachi.rs's Java-Sudachi compatibility contract: ship the byte-identical wins —
> #39 + #40 (~+9%) **+ H7 (share `WordInfo` via an Arc-cache, +20–21%)** on top of prefetch+varint, for a
> **~1.35× single-thread ceiling** — and use **multithreading (~8.9×)** for throughput. The lattice
> *kernel* (`connect_node`/matrix) is *provably* at its floor — don't touch it; the large lever was in
> WordInfo materialization (H7). A **~3.7×** speedup is real and measured, but only by abandoning
> compatibility (pointwise Vaporetto+dict: no A/B/C, −0.92 SEG, −1.74 POS) — viable as a *separate*
> tool, never as sudachi.rs. **Recommendation: Path A for the library; keep Path B on the shelf as a
> measured option.**

*A decision-support document. Two ways to make sudachi.rs faster are laid out in full, with pros and
cons of each, so the trade-off is visible and the call is yours. Every speed/quality figure is [meas]
on a runnable artifact (lattice: `tokenize_pipeline_bench`; pointwise: the fused `/tmp/vapo-sud`;
golden benchmark `/tmp/gold/`), Apple M4 Max, UD-Japanese-GSD. Full study: `optimization-study.md`.*

## The question, and the one constraint that frames it

How much faster can the tokenizer get? The answer splits on a single question:

> **Must the output stay identical to Java Sudachi** — same A/B/C multi-granularity, same POS, same
> segmentation boundaries — which is the compatibility contract sudachi.rs exists to honor?

- **Path A — keep Java parity.** Optimize the lattice without changing a byte of output.
- **Path B — change the paradigm.** Replace the lattice with pointwise segmentation (Vaporetto) +
  dictionary feature lookup. Much faster, but the output is no longer Java-Sudachi's.

Both are real and measured. The rest of this doc gives each its honest pros and cons.

## At a glance

| | **Path A — keep Java parity** | **Path B — paradigm change (pointwise)** |
|---|---|---|
| Output vs Java Sudachi | **identical** (A/B/C, POS, boundaries) | **different**: one segmentation, −0.92 SEG, −1.74 POS, no A/B/C |
| Single-thread speed | ~290 ns/char; **ceiling ≈ 1.35×** (#39/#40 + H7; lattice kernel at floor) | **~78 ns/char = ~3.7×** (real fused, warm cache) |
| Throughput lever | **multithreading ~8.9×** (orthogonal, compatible) | also multithreads; pointwise scales cleanly |
| Effort | **small** — merge already-built wins | **large** — new tokenizer + model training pipeline |
| Ongoing cost | none (derives all from the dict) | **retrain a ~58 MB model per dictionary** |
| Risk | **none** (byte-identical, proven) | **high** (compat break + model maintenance) |
| Fits *inside* sudachi.rs? | **yes** | **no** — a separate, non-compatible tool |

## Path A — Keep Java-Sudachi parity (optimize the lattice)

**What it is.** The current architecture: double-array trie common-prefix search + Viterbi over the
71 MB connection matrix. Optimize it without changing output.

**Pros**
- **Byte-identical to Java Sudachi** — A/B/C, POS, boundaries all preserved. The compatibility contract
  is untouched; reproducibility across Rust/Java/Python holds.
- **Zero risk, small effort.** The wins are already built and proven byte-identical: prefetch (+16.7%),
  varint (+2.7%), and this study's two transplants — #39 (gate the per-node `normalized_form` decode in
  JoinNumeric) and #40 (WordInfoParser variable-section early-out), together **~+9%** (320→294 ns/char),
  verified across modes A/B/C on two corpora.
- **+ H7 (#52): sharing `WordInfo` via an `Arc` + a bounded thread-local cache is another +20–21%**,
  byte-identical (A/B/C × 2 corpora). WordInfo materialization (parse+resolve) is ~17% of `do_tokenize`,
  recovered by cheap shared access instead of a deep clone. The single largest byte-identical lever.
- **Multithreading is the real throughput lever — ~8.9×, orthogonal and compatible.** It stacks on top
  of the single-thread wins and changes no output.
- **We now know exactly where the floor is** (so no effort is wasted): an 18-agent microarchitectural
  analysis proved the connection matrix is **L2-resident, not DRAM-bound** (live working set 3.77 MiB,
  0 capacity misses, 97.95% L2 hits; the matrix load is only ≤7.2% of `do_tokenize`), and the codegen is
  optimal. The `connect_node` kernel is at its floor.

**Cons**
- **Single-thread ceiling ≈ 1.35×** (#39/#40 + H7b over what's shipped). The lattice *kernel*
  (`connect_node`/matrix) is at its floor — not a lever; the large lever was in WordInfo materialization
  (H7). Beyond ~1.35× the rest is spread thin (trie at its byte-indexed floor, OOV, allocations) —
  squeezing more means architectural surgery for a few %.
- **Throughput gains require threads.** If a workload is single-threaded and latency-bound, Path A is
  near its limit.

**Net:** the safe, correct path. Ship #39/#40 **+ H7** (~+20% total, byte-identical), lean on multithreading, leave the lattice kernel alone.

## Path B — Change the paradigm (Vaporetto pointwise + dictionary)

**What it is.** Drop the lattice. Vaporetto (pointwise linear classification, by Works Applications)
segments and POS-tags in O(n); each segment is then looked up in the Sudachi dictionary to recover
`normalized_form` and POS candidates. A real fused binary (`/tmp/vapo-sud`) implements exactly this.

**Pros**
- **~3.7× faster on the full task** (seg + POS + normalized): **~78 ns/char vs ~290** [meas, real fused
  binary, warm surface cache]. Robust at 71–113 ns/char across GSD-test/dev and news (kyoto-leads).
- **Much simpler, smaller hot path:** no 71 MB matrix, no lattice, no Viterbi. O(n) pointwise work that
  vectorizes and parallelizes cleanly (the per-thread feature cache scales 8.6–9.5× on 12 cores).
- **Reuses the existing dictionary** for feature recovery — the lexicon exact-lookup and
  `normalized_form` resolution already exist; only the segmenter and POS disambiguation are new.

**Cons**
- **Breaks Java-Sudachi compatibility — the decisive one.** This is not a faster Sudachi; it is a
  *different tokenizer*. Specifically:
  - **No A/B/C.** Pointwise yields a single segmentation; the multi-granularity splits are gone — a
    primary Sudachi feature, disqualifying for any consumer that uses them.
  - **Different output even where good:** −0.92 SEG-F1, −1.74 POS-top vs the dictionary lattice, so
    reproducibility against Java/Python Sudachi is lost.
- **POS needs a hybrid, or it's bad.** Taking the first dictionary entry per surface gives POS-top ≈ 70
  (homograph trap); you must keep Vaporetto's predicted POS to disambiguate the dictionary's candidates
  (→ 95.7). The pipeline needs *both* signals — more moving parts.
- **Operational burden:** a separately-trained ~58 MB model, retrained whenever the dictionary changes
  (the lattice derives everything from the dict for free).
- **Speed is cache-dependent.** The ~3.7× assumes warm surface reuse (batch/indexing). On cold or
  highly-diverse short inputs the per-token dict recovery rises from ~7 to ~110 ns/char and the win shrinks.

**Net:** real and substantial speed, but it changes the product. It can only live *outside* sudachi.rs's
compatibility promise — as a separate tool for users who explicitly don't need Java parity.

## Side-by-side: the pros and cons that decide it

| dimension | Path A (keep parity) | Path B (paradigm) |
|---|---|---|
| Java-Sudachi compatibility | ✅ identical | ❌ broken (no A/B/C, different seg/POS) |
| A/B/C multi-granularity | ✅ yes | ❌ no (single segmentation) |
| Speed (single-thread, full output) | ➖ ~1.35× ceiling (#39/#40+H7b) | ✅ ~3.7× |
| Throughput at scale | ✅ ~8.9× via threads | ✅ scales well too |
| Quality vs gold | ✅ SEG 97.99 / POS 97.46 | ➖ SEG 97.07 / POS 95.72 |
| Effort to adopt | ✅ small (merge built wins) | ❌ large (new path + training) |
| Ongoing maintenance | ✅ none extra | ❌ retrain model per dict |
| Risk | ✅ none (byte-identical) | ❌ high (compat + model) |
| Belongs in sudachi.rs | ✅ yes | ❌ no (separate project) |

## How to read the trade-off

- **If Java-Sudachi compatibility is required** (the sudachi.rs contract): **Path A only.** Ship #39/#40 + H7,
  use multithreading for throughput, do not touch the matrix (it's at its floor — proven, with a
  regression guard via the nomat probe + LRU sim). 3× "as sudachi.rs" is not achievable; this is the floor.
- **If a workload genuinely does not need A/B/C or exact Java output** (a throughput-bound, SUW-only
  indexing/search pipeline): **Path B is real and ~3.7×** — but as a *separate* tool, not the library.
  The runnable prototype and numbers are ready to hand off.
- **Most likely correct outcome:** Path A for sudachi.rs (merge the byte-identical wins + document the
  floor so the matrix is never re-litigated), with Path B kept on the shelf as a measured option should a
  non-compatible fast tokenizer ever be wanted.

## Evidence & reproduction

```bash
# Path A — byte-identical wins + floor proof
#   #39/#40 transplants: optimization-study.md §5.6-5.7 (worktree /tmp/sud-vib)
#   matrix-is-at-floor proof (L2-resident, nomat probe + LRU sim): §5.11
SUDACHI_BENCH_SUBSET=min SUDACHI_BENCH_MODE=C cargo run --release --example tokenize_pipeline_bench

# Path B — real fused pipeline: speed (3 variants) + quality
cd /tmp/vapo-sud   # vaporetto 0.6.5 + sudachi path-dep
VAPO_MODEL=/tmp/vapo/model.raw SUDACHI_BENCH_CONFIG=<full>/sudachi.json \
  SUDACHI_BENCH_DICT=<full>/system_full.dic SUDACHI_BENCH_INPUTS=/tmp/gold/gsd_text.txt \
  cargo run --release                            # timing: vaporetto-only / fused / fused+cache
VS_MODE=dump cargo run --release > /tmp/gold/fused_out.txt
python /tmp/gold/score_fused.py                  # SEG + POS (model / dict-first / hybrid) + normalized
```
