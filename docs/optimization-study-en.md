# sudachi.rs — comprehensive optimization and evaluation study (issue #117 and beyond)

> Publication-grade summary report: benchmark methodology (speed + quality),
> every hypothesis tested, with numbers (positive and **negative**), and conclusions.
> The provenance of each number is tagged: **[measured]** — measured by us on the M4 Max in this
> study; **[doc]** — from `docs/performance-investigation.md` (earlier measurements);
> **[literature]** — from peer-reviewed literature (see §8); **[proj]** — Amdahl projection.

## 1. Abstract

We investigated whether the hot path of sudachi.rs (a lattice-Viterbi
Japanese morphological analyzer: common-prefix search over a double-array trie +
Viterbi over a connection matrix) can be sped up **with and without** format-compatibility loss, and
we built a **golden benchmark** (speed + accuracy) that Sudachi did not have before.
46 numbered hypotheses (§5.0), four rounds of literature research,
an assembly-level analysis, and prototypes converged on three conclusions.

**(1) The byte-identical path has a ceiling.** prefetch (+16.7%) and varint
(+2.7%) have been shipped; this study added two new output-preserving ports —
gating the `normalized_form` decode in JoinNumeric (#39) and an early-out for the variable section of
WordInfoParser (#40), together **~9%** on a realistic subset (do_tokenize 320→294
ns/char). But **3× byte-identical is unreachable**: the profile bottoms out on `Lattice::insert`
(connection matrix ~25%, historically impenetrable) + WordInfo materialization per node
(~17%, from which the **WordInfo cache (#52) recovers +27% on default output and +44% on full
`-a` output** [measured against the pre-cache base; the early prototype estimate was +20–21%] via a
shared `Arc` cache); the full-output ceiling is ≈ **1.4–1.6×**.

**(2) The pointwise paradigm (Vaporetto) gives multipliers — but "by doing less".** On a matched
golden benchmark against the optimized Sudachi (293 ns/char): seg+POS **8.1×**.
But as a **full replacement** (with `normalized_form`, which the pipeline reconstructs via
dict-lookup) — **2.3× without cache → 3.3× with a decode cache (#2) → up to 6.5× with a
surface cache** (§5.9, depends on the repetition of surfaces), asymptote → 8×
(= the segmenter itself). The decode cache is the **first lever common to both the lattice and pointwise**
(~40 ns/char); its prod version without leaks + thread-safe survives (6.1×, §5.9-B).

**(3) The cost of the paradigm is less than it seemed.** The POS gap is **not −3.96 but −1.19**:
most of it was an artifact of MODEL-predicted POS; by taking **dict-POS** (the dictionary
is on hand anyway), the gap shrinks to −1.19, with the remainder being segmentation
propagation (§5.10). The final cost of replacement: SEG **−0.92**, POS **−1.19**, hard constraints — **no A/B/C**
+ retraining. **Synthesis:** 3× while preserving the FULL Sudachi output is unreachable either
byte-identical (~1.4–1.6× with the WordInfo cache) or via the paradigm at feature parity without a cache (2.3×); all the
big multipliers = **task reduction** (dropping features / A-B-C / lattice) ± a repeat cache.

## 2. Environment and data

| parameter | value |
|---|---|
| CPU | Apple M4 Max (12 P + 4 E cores), line 128 B, L1d 128 KB/P, **L2 16 MB / 6 P-cores**, page 16 KB, 48 GB |
| toolchain | rustc/cargo 1.96.0, `--release` |
| dictionary | SudachiDict full (small+core+notcore) = **2.59M keys**; trie ≈ 72 MB; connection matrix 5981² × i16 ≈ 71 MB |
| corpus (speed) | kyoto-leads: 16,051 sentences / **461,815 codepoints** |
| corpus (quality) | **UD_Japanese-GSD** test: 543 sentences / **13,034 SUW tokens** (CC BY-SA 4.0) |

## 3. Benchmark methodology

### 3.1 Speed, end-to-end
`examples/tokenize_pipeline_bench`: `reset → do_tokenize → collect_results`
(`InfoSubset::all`), the scalar and pipelined paths **interleaved** on each trial,
median of N (N=31) + coefficient of variation (CV); a win is counted only above
the noise floor. Metric: **ns/char** and sentences/s. ⚠️ Run-to-run noise on a laptop
is real (CV ~3–4%); isolated and relative numbers are reliable.

### 3.2 Isolated lookup
`examples/dictionary_matcher_report` (feature `matcher-comparison`): builds
yada / crawdad / daachorse / fst / rsmarisa over the **real 2.59M keys**,
runs common-prefix from real corpus positions, metric **ns/start** + heap +
serialized bytes; all methods are checked for **match identity** (exact).

### 3.3 Profiling
`sample` profile of the pipelined tokenizer [doc] + a **dependent-load counter**
(an atomic in `step_once`, 1 pass over the corpus) [measured].

### 3.4 Quality (golden benchmark) — net-new
The seminal Sudachi paper (Takaoka et al., LREC 2018) **does not provide a single accuracy
number** [literature] → the benchmark was built from scratch.
- **Corpus:** UD_Japanese-GSD (word-units = slightly-changed **SUW**, UniDic;
  CoNLL-U with FORM/UPOS/**XPOS=UniDic SUW POS**), CC BY-SA 4.0.
- **Metric:** token-span F-measure (Nagata 1994 → Kudo 2004 → KyTea 2011 →
  Vaporetto 2024): a token is correct if **BOTH the boundaries (span) AND the POS tag** match.
  P=#correct/#system, R=#correct/#gold, F=2PR/(P+R). Depths: **seg** (boundaries
  only), **top** (boundaries + POS[0]). [literature]
- **Scheme matching (critical for fairness):** there is NO published method that fairly
  compares DIFFERENT granularities against a single gold [literature]; the principle is **scheme matched to the
  gold**: Sudachi mode A (≈ UniDic SUW) and Vaporetto (the bccwj-suw model) are scored
  natively against GSD-SUW. A token-span comparison of different granularities (Vaporetto-SUW
  vs Sudachi-C) is **not defensible** — for such cases only boundary-level seg-F1 is used.
- **Scorer:** `bench/quality/score.py` (ours), `gold_align_miss=0` (validated).

## 4. Hot-path facts

| fact | value | provenance |
|---|---|---|
| profile: connection matrix / Viterbi | **22.8%** | [doc] |
| profile: trie walk (common-prefix) | **16.9%** | [doc] |
| profile: alloc / WordInfo+UTF16→8 / varint / OOV | 9.6 / 9.6 / 9.5 / 7.5% | [doc] |
| dependent loads in the trie | **7.36 / codepoint** (2.45 / byte), ~3.4M/corpus | [measured] |
| average walk length | ~2.45 codepoints (~7.4 bytes) to dead-end | [measured] |
| asm walk (M4 Max) | 1 dependent load/byte, bounds-checks eliminated, **load-latency-bound** | [measured] |
| matrix | volume-bound, NOT miss-bound (prefetching the matrix hurts) | [doc/measured] |

## 5. Experiments — full table (all hypotheses)

### 5.0 Summary numbered table (all hypotheses in order)

| # | Hypothesis | Number on benchmark | Δ | Result |
|---:|---|---|---|:--:|
| 1 | baseline 0.7 (scalar) | 457.6 ns/char · 75,959 sent/s | — | 📊 base |
| 2 | prefetch K=4 (PR #348, L1 #117) | 391.9 ns/char · 88,682 | +5…17% (median +9%) | ✅ |
| 3 | varint direct decoder | — | +2.7% | ✅ |
| 4 | daac (charwise AC, in-situ) | ~387 ns/char · 90,034 | ~0% | ➖ +41% size |
| 5 | multithread ×16 | 802,669 sent/s | ~8.9× | ✅✅ |
| 6 | Vaporetto seg | 41.6 ns/char · 835,768 | 9.4× vs prefetch | ✅ paradigm |
| 7 | Vaporetto seg+POS | 54.9 ns/char · 632,813 | 7.1× vs prefetch | ✅ paradigm |
| 8 | yada trie (isolated) | 35.25 ns/start | 1.00× | 📊 base |
| 9 | yada + prefetch K=4 | 29.4 ns/start | 1.20–1.25× | ✅ |
| 10 | yada + prefetch K=8/12/16 | — | ≤1.0× | ❌ |
| 11 | crawdad trie | 25.4 ns/start | 1.37–1.39× · 0.76× mem | ✅ lever |
| 12 | crawdad MP-trie | 27.6 ns/start | 1.28–1.37× · 0.67× mem | ➖ memory↓ |
| 13 | daachorse charwise | 25.4 ns/start | 1.29–1.39× · 1.86× mem | ➖ memory↑ |
| 14 | daachorse bytewise | 42.2 ns/start | 0.84–0.89× | ❌ |
| 15 | MARISA (LOUDS succinct) | 303 ns/start | 0.12× (8× slower) · 0.16× mem | ❌ |
| 16 | FST (Burntsushi) | 1237 ns/start | 0.03× (35× slower) | ❌ |
| 17 | prefetch hint L2-keep vs L1 | 1.201 vs 1.245 | −3.5% rel. | ❌ L1 wins |
| 18 | Sudachi A — quality | SEG-F1 97.99 · POS-top 97.46 | — | 📊 quality base |
| 19 | Sudachi B — quality | SEG-F1 95.06 · POS 94.58 | −2.93 | ➖ granul. |
| 20 | Sudachi C — quality | SEG-F1 92.35 · POS 91.88 | −5.64 | ➖ granul. |
| 21 | Vaporetto — quality | SEG-F1 97.07 · POS-top 93.50 | −0.92 SEG / −3.96 POS vs A | ⚖️ cost of paradigm |
| 22 | SoA `connect_node` | −1…4% e2e (exact) | negative | ❌ |
| 23 | conn-ID freq remap (Vibrato) | 0.99× + broke 92/16051 | ~0 + breakage | ❌ |
| 24 | matrix software prefetch | 181→202 ms | −12% | ❌ |
| 25 | matrix row-hoist | — | no-op | ❌ |
| 26 | int8 quantization of the matrix | lossy (breaks output) | — | ❌ |
| 27 | vEB / cache-oblivious relayout | theor. slower than BFS | — | ❌ |
| 28 | build-time node relayout | 14× build / ≤1.10× lookup | — | ❌ |
| 29 | freq-weighted layout (MinWEP) | closed (yada sort-keys) | — | ❌ |
| 30 | path-compression MP (Japanese) | 11→21 ns/start | slower | ❌ |
| 31 | k-byte stride DA | memory 256^k | — | ❌ |
| 32 | code-dividing / D2FA | +14…47% latency | — | ❌ |
| 33 | Cuckoo Trie / MLP front-end | = our prefetch, worse on CJK | — | ❌ |
| 34 | SIMD / ART node-decode | N/A (XOR, no scan) | — | ❌ |
| 35 | MeCab+UniDic (fugashi) — quality | SEG-F1 99.11 · POS-top 97.57 | +1.12 SEG / +0.11 POS vs Sudachi-A | 📊 UniDic reference |
| 36 | Jagger (KWDLC/JUMAN) — boundary+speed | SEG 81.37 (scheme-mismatch) · 272k sent/s [measured py] / >1M [literature C++] | n/a POS | ⚠️ boundary-only; very fast (~3.7× Sudachi even through the py-binding) |
| 37 | Vibrato-UniDic (Rust lattice) | SEG 97.64 · POS 97.03 · 112 ns/char · 226k sent/s | −0.35 SEG vs Sudachi-A; **~3.1× faster** (non-compact) | ✅ Rust-lattice faster than Sudachi at ≈the same accuracy → speed-headroom |
| 38 | esupar (neural BERT, CPU) — speed | 30.6 sent/s · 831k ns/char · SEG 59.17 (LUW≠SUW) | **~2400× slower than Sudachi, ~30000× than Vaporetto** | ⚠️ neural: accuracy at the cost of catastrophic CPU speed; default model is LUW (granularity-mismatch, not quality) |
| 39 | **JoinNumeric: gate the `normalized_form` decode** (port, implements §5.5②③) | do_tokenize mode A 383→354 ns/char (median; min 360→341); mode C 357→337 (min) | **−5…8% do_tokenize** (~20–30 ns/char), pipelined too | ✅ **byte-identical** A/B/C + num-stress; **the first positive format-preserving port** from §5.5 |
| 40 | **WordInfoParser: early-out for the variable section** (port; skip split/synonym/user-data when the subset does not request them) | mode C `min` 301→294 (−2.3% vs #39); `pos` 302→289 (−4.3% vs #39); only fires when there is no SPLIT in the subset | **−2…4% do_tokenize** additional (mode C / CLI-subset); mode A `min` ≈0 (control: SPLIT_A needed → does not fire) | ✅ **byte-identical** (12 combos: ±`-a` × A/B/C × GSD/num-stress); removed `parse_u32/i32_array` from the profile |
| — | **benchmark methodology fix** (not a hypothesis) | `tokenize_pipeline_bench` was running under `InfoSubset::all()` (the default of `StatefulTokenizer`); the prod CLI sets `POS_ID\|NORMALIZED_FORM` (+DICT/READING/SYNONYM under `-a`) | the `all()` artifact is ≈ **+6%** (21 ns/char) to do_tokenize: extra parsing of split/synonym arrays that prod never touches | ⚠️ all earlier numbers (#1–#38) were measured under `all()` → the realistic baseline is lower; added `SUDACHI_BENCH_SUBSET` |
| 41 | **Vaporetto matched vs the optimized Sudachi** (#39+#40), GSD | seg 27.3 / seg+POS 36.4 ns/char vs Sudachi e2e mode C **293.5** | **8.1× (seg+POS), 10.7× (seg)** | ✅ paradigm >3× for seg+POS — but see #42 for full parity |
| 42 | **feature-recovery: Vaporetto+dict as a FULL replacement** (seg+POS+`normalized_form`) | dict-lookup **89.3 ns/char** (143 ns/token, 99.1% hit) → Vaporetto-full ≈ **125 ns/char** | **~2.3× vs Sudachi** (not 8×!) | ⚠️ [measured] **the main honest result**: 8× — only for seg+POS; for output parity ~2.3× + POS −3.96 + no A/B/C |
| 43 | **prototype #2: decode-cache** (per-thread WordId→&str for `normalized_form`, env `SUDACHI_NF_CACHE`) | Vaporetto feature-recovery 89.7→**49.8** (−44%); Sudachi e2e+NF-output 326.8→**285.1** (−13%) | both paths −~40 ns/char (the `from_utf16` decode); **full parity 2.6×→3.3×** | ✅ content-identical (totals match + CLI `-a` diff clean A/B/C); the prototype leaks (prod = owned cache in the dictionary) |
| 44 | **word_id cache (Vaporetto path)** — cache `word_id→normalized` BEFORE the fetch, skip `get_word_info_subset`+decode on a hit | feature-recovery 90.3→**18.1** ns/char; Vaporetto-full = 36.4+18.1 = **54.5** | **5.2× vs decode-cached Sudachi / 6.0× vs uncached** | ✅ [measured] the asymmetry is fair (Sudachi cannot skip WordInfo — the lattice needs it) |
| 45 | **surface→norm cache (Vaporetto path)** — key=surface string, skip even the trie on a hit | feature-recovery 90→**7.6** ns/char; Vaporetto-full = 36.4+7.6 = **44.0** | **6.5× vs cached / 7.4× vs uncached** | ✅ [measured] ~6.8× confirmed; almost at the 8× asymptote (= bare seg+POS); the Sudachi floor ~285 is unavoidable |
| 46 | **cache multithreading** — global Mutex vs DashMap vs thread_local (T=1…12, M4 Max) | Mutex 0.3–0.4× (collapse), DashMap 1.2–1.4×, **thread_local 8.6–9.5×** (aggregate Mchar/s) | per-thread scales almost linearly; the global lock is a convoy | ✅ [measured] **prod recommendation: thread_local** (not Mutex, not DashMap for the tiny critical section) |
| 47 | **`connect_node`/matrix — NOT DRAM-bound (refutation)** [18-agent microarch analysis: LRU-sim + nomat-probe + cycle-arith + disasm] | live working set **3.77 MiB** (30916 row-cache-lines, 0 capacity-misses @4MB and @16MB L2, 97.95% L2-hits); the matrix load = **≤7.2% do_tokenize** (nomat: 6.67→6.19ms); skew: 1474/5981 conn-id, top-16 rows=47% | **0% byte-identical lever** — the kernel is on the floor; the matrix is L2-resident, throughput/issue-bound, not latency | ✅ [measured] **corrects the premise "the matrix is volume/DRAM-bound"**; explains the failure of #24 (prefetch on cache-resident = load-port pressure); batching MLP hits a non-problem |
| 48 | **REAL fused Vaporetto+dict binary** (vaporetto+sudachi in one crate, LTO, Sentence-reuse) — **corrects #41/#42/#45** | seg+POS materialized **~71** (not 36); full-parity+cache **~78 ns/char**; SEG 97.07/96.16, POS-hybrid 95.72/94.40, dict-first 70 (homograph trap), normalized 100% | **~3.7× vs Sudachi** (NOT 6.5×); −0.92 SEG / −1.74 POS | ✅ [measured] **the main correction**: the 6.5× projection mixed profiles + un-materialized v64-seg; robustly 71–113 ns/char on GSD-test/dev/kyoto |
| 49 | **exact min-plus batching of `connect_node` by `left_id` (H1)** — probe + byte-identical prototype | **probe:** work-cut −22% (H1, ratio 0.777) / −39% (H1+H2, 0.609); cross-validated 1.5M lookups. **prototype:** byte-identical (A/B/C+numstress), but **−3.6%/−5.2%** (lean Vec-memo; −15% with HashMap) | the algorithmic win is real, but **does not convert into time** | ❌ [measured] **confirms §5.11**: the connect-loop is too cheap (L2-resident, issue-bound, 13-instr) — per-node dedup-bookkeeping is more expensive than the iterations saved. H2/H3 — the same overhead profile |
| 50 | **H4/H5 first-char/hot-prefix trie sidecar** — measured + reason-closed | probe: **avg 7.216 trie-steps/start** (GSD scalar); first-char ≈3 steps (~42% of step-count), but these are **the hottest** (root, L1) steps, and the walk latency is already hidden by K=4-prefetch (#117) | ~0.5–0.8% best-case − bookkeeping → H1-redux | ❌ [reason+measured] the H1 lesson: it hits the cheap (hot) part — the per-start table-lookup dominates; the **sidecar** is on the byte-indexed floor (§6, 4 rounds). This closes the sidecar, NOT the corpus-frequency prefetch-relayout of the double array (idea #117) — that one is not prototyped, assessed only by literature (N7/N8), and remains open |
| 51 | **H6/H7 WordInfo materialization** — two probes (WI-cache + no-WI) | **WI-cache** (clone resolved) **~0%** (mode A −1.0% / C +1.1%); **no-WI** (skip parse+resolve, no clone) **+17.8% / +15.9%** | **H6 alone refuted (~0%)** — clone ≈ parse cost; **H7 ceiling CONFIRMED ~16–18%** | ⚠️ [measured] **the largest byte-identical lever**: parse+resolve = ~17% do_tokenize, but recoverable ONLY borrowed/shared, not with a deep-clone cache |
| 52 | **Shipped to prod — `WordInfo` = `MaybeShared<WordInfoData>` + `MaybeShared<StringsCache>` (Owned or Arc-Shared) + bounded tokenizer-local cache** | sharing both the resolved data and the string decode: **+27% on default output, +44% on full `-a`** byte-identical [vs pre-cache base; the prototype estimate was +20–21%]; 2-way set-associative (~1 MB, lazily allocated), exact key=(word_id,subset), tokenizer-local (no lock); OOV bypasses | **✅✅ THE LARGEST byte-identical win** (≈ #2 + #39-decode + materialization at once) | ✅ [measured] byte-identical A/B/C × 2 corpora, diff=0. Sharing is opt-in (`MaybeShared`) so non-cached paths stay alloc-free; the subset-key is mandatory (otherwise A/B broke). Committed to PR #348 |
| 53 | **H8 deterministic islands / H9 teacher-student** — characterized | H8-lossy and H9 (learned) change the output → outside Java-compat (like the paradigm #48); H8-exact (certifying bounds) is byte-identical, but attacks the cheap lattice (the H1 lesson) + is complex | low-priority/separate | ⚠️ H8-lossy/H9 — a separate tool (not sudachi.rs); H9 — "almost a paper", the seed = the fused prototype §5.12; H8-exact — unlikely (the lattice is cheap) |

(Breakdown by subsystem — in §5.1–5.4 below. Additional engines on the golden benchmark — §5.3. Port #39 in detail — §5.6.)

### 5.1 Speed e2e (full dict, kyoto-leads)

| approach | ns/char | sent/s | Δ | output | format | provenance |
|---|---:|---:|---|---|---|---|
| baseline 0.7 (scalar) | 457.6 | 75,959 | — | identical | — | [measured] |
| **+ prefetch K=4 (PR #348)** | 391.9 | 88,682 | **+5…17%** (median doc +9%) | identical | unchanged | [measured/doc] |
| **+ varint decoder** | — | — | **+2.7%** | identical | unchanged | [doc] |
| daac (charwise AC) | ~387 | ~90,034 | ~0% | identical (260,114) | +41% size | [measured] |
| **multithread ×16** | — | **802,669** | **~8.9×** | identical | unchanged | [doc] |
| **Vaporetto seg** | 41.6 | 835,768 | **9.4×** vs prefetch | different | diff. engine | [measured] |
| **Vaporetto seg+POS** | 54.9 | 632,813 | **7.1×** vs prefetch | different | diff. engine | [measured] |

### 5.2 Isolated lookup (real 2.59M keys, exact matches) [measured]

| structure | vs yada (ns/start) | memory vs yada | output |
|---|---:|---:|---|
| yada (current) | 1.00× | 1.00× | base |
| **yada + prefetch K=4** | **1.20–1.25×** | 1.00× | shipped |
| yada + prefetch K=8/12/16 | ≤1.0× | 1.00× | worse than K=4 |
| **crawdad trie** | **1.37–1.39×** | **0.76×** | lever |
| crawdad MP-trie | 1.28–1.37× | **0.67×** | memory↓, latency≈ |
| daachorse charwise | 1.29–1.39× | 1.86× | fast, memory↑ |
| daachorse bytewise | 0.84–0.89× | 2.98× | worse |
| MARISA (LOUDS succinct) | **0.12×** (8× slower) | **0.16×** | succinct: memory↓, latency↓↓ |
| FST (Burntsushi) | **0.03×** (35× slower) | 0.37× | unusable |
| prefetch hint L1-keep vs L2-keep | **1.245 vs 1.201** | — | L1 is correct |
| AC scan ns/char (char daac / byte / crate) | 17.3 / 35.1 / 58.6 | — | the Vaporetto engine |

### 5.3 Quality (UD_Japanese-GSD, span-F1, matched SUW) [measured]

| engine | SEG-F1 | POS-top-F1 | speed | scheme |
|---|---:|---:|---:|---|
| **Sudachi mode A** | **97.99** | **97.46** | 1× | SUW, lattice |
| Sudachi mode B | 95.06 | 94.58 | 1× | medium units |
| Sudachi mode C | 92.35 | 91.88 | 1× | NE units |
| **Vaporetto-SUW** | **97.07** (−0.92) | **93.50** (−3.96) | **7–9×** | SUW, pointwise |
| **MeCab+UniDic** (fugashi) | **99.11** (+1.12) | **97.57** (+0.11) | CRF-lattice (~MeCab) | SUW/UniDic reference |
| **Vibrato-UniDic** (Rust lattice) | **97.64** (−0.35) | **97.03** (−0.43) | **112 ns/char · 226k sent/s (~3.1× Sudachi)** | SUW/UniDic |

Conclusion on scheme-matched comparison: MeCab+UniDic is the accuracy ceiling on GSD (the UniDic reference, =Kudo'04
~99% news). Sudachi-A −1.12 SEG = the measured cost of the UniDic revisions in SudachiDict (POS almost
at parity). Vaporetto is the fastest, but also the lowest in accuracy (especially POS).
JUMAN engines (Jagger/Juman++) are boundary-level only (a different segmentation standard), POS is incomparable.
Empirically: Jagger (KWDLC/JUMAN) on GSD-SUW gives SEG-F1 **81.37** (11,848 tokens vs 13,034) —
but this is a measure of SCHEME DIVERGENCE (JUMAN≠SUW), not of quality; it confirms the scheme-matching principle
with a number. Jagger's value is speed (>1M sent/s [literature]), not measurable as quality on the SUW gold.
Likewise esupar (neural BERT, default ja = LUW model): SEG-F1 **59.17** on GSD-SUW = LUW≠SUW
granularity-mismatch, not quality (8803 tokens vs 13,034). The clean contribution of neural is **CPU speed:
30.6 sent/s ([measured]) = ~2400× slower than Sudachi, ~30000× slower than Vaporetto** — a point on the spectrum
of "transformer: high accuracy at the cost of catastrophic CPU speed". UPOS≠the XPOS metric, not compared.

**Matched-corpus speed (GSD, both Rust/native, 543 sentences, [measured]):** Sudachi ~348 ns/char
(mode C, full analysis, 73k sent/s) vs Vaporetto seg 27 ns/char (940k sent/s) = **~12.9×** on GSD
(seg+POS narrows it to ~10×); this confirms the kyoto-leads 9.4× on a second corpus. Caveat: Sudachi-full
vs Vaporetto-seg (seg-only-vs-full), the classic pitfall — more honestly ~10× at equal volume.

A note on POS-top: part of the Vaporetto gap is an artifact (the model does not emit a tag for
punctuation, which is counted as POS-incorrect); even after correction the POS gap is real and larger than
the segmentation one. The Vaporetto tag format = UniDic POS (matches GSD XPOS, sometimes
finer: `動詞-一般` vs `動詞-一般-五段-サ行`) — the top-level POS[0] is directly comparable.

Reference ceiling for the news domain (for framing): Kudo et al. 2004 seg/top/all ≈
98.96/98.31/96.75 on the Kyoto Corpus [literature]; GSD is web text, so 97.99 is expectedly
lower. The 2.01% shortfall of Sudachi-A = the measured **residual mismatch of Sudachi-A vs GSD-SUW**.

### 5.4 ❌ Negative results — "do not go here" (with numbers and reasons)

| # | hypothesis | result | reason | provenance |
|---|---|---|---|---|
| N1 | SoA `connect_node` (lattice) | **−1…4%** (exact) | short predecessor lists → 2 streams worse than 1 padded | [measured] |
| N2 | conn-ID freq remap (Vibrato) | **0.99× + broke 92/16051** | the matrix is volume-bound; OOV take conn-ID from the runtime config | [doc] |
| N3 | matrix software prefetch | **181→202 ms** | the matrix row is L2-resident → prefetch overhead | [doc] |
| N4 | matrix row-hoist | no-op | LLVM already hoists the invariant | [doc] |
| N5 | int8 quantization of the matrix | lossy, not argmin-preserving | breaks the output (like N2) | [literature] |
| N6 | vEB / cache-oblivious trie relayout | theor. slower than BFS | index arithmetic > miss; DA is already cache-conscious | [literature] |
| N7 | build-time node relayout | **14× build / ≤1.10×** | does not pay off | [doc] |
| N8 | frequency-weighted layout (MinWEP) | closed | yada builds from sorted keys → cannot reorder; relayout = N7 | [literature] |
| N9 | path-compression MP-trie (Japanese) | **slower (11→21 ns)** | short dense keys, few non-branching tails | [literature] |
| N10 | k-byte stride DA | memory **256^k** | impossible for 3-byte kanji | [literature] |
| N11 | code-dividing / D2FA | **+14…47% latency** | the inverse axis — more loads | [literature] |
| N12 | prefetch hint L2-keep | 1.201 < L1 1.245 | L1-keep is correct | [measured] |
| N13 | prefetch lanes K≥8 | worse than K=4 | low-instruction walk, the MLP ceiling (ART 1.6–1.7×) | [measured/literature] |
| N14 | Cuckoo Trie / MLP front-end | = our prefetch, worse on CJK | deep shared kanji prefixes | [literature] |
| N15 | SIMD/ART node-decode | N/A | DA computes the child via XOR — no candidate scan | [measured/literature] |
| N16 | FST / MARISA backend | 35× / 8× slower | not for common-prefix-from-boundary | [measured] |

### 5.5 Why Vibrato is ~3× faster than Sudachi (measured decomposition) [measured]

Vibrato 112 ns/char vs Sudachi mode-A `do_tokenize`-only 382 (full) / 301 (without the input+path plugins).
- **`collect_results` ≈ free** (full 369 ≈ nocollect 380) → WordInfo materialization is NOT the bottleneck; lazy-WordInfo "on output" will not help.
- **Mode A ≈ C** → the split modes are not to blame.
- **`sample` profile of `do_tokenize`:** dominated by **allocations** (malloc/free/memmove/realloc — #1 self-time) and the **WordInfo decode** (`parse_u32_array`, `String::from_utf16`, `get_word_info`); the latter is pulled by the **path-rewrite plugins** (JoinNumeric/JoinKatakanaOov fetch POS/normalized) even in mode A.
- The plugin layer = **~21%**, broken down [measured]: path-rewrite (JoinNumeric/JoinKatakanaOov, fetch WordInfo→from_utf16) **~14%** (382→330), input-norm **~8%** (330→301). The core without plugins = 301 ns/char — still ~2.7× Vibrato (allocations dominate).
- The remainder 301 vs 112 = **~2.7×**. The core profile (without plugins) [measured]: #1 **`Lattice::insert`** (connect_node min-plus over the matrix + push into 3 parallel Vecs); #2 **eager-fetch of WordInfo in `do_tokenize`** (`get_word_info_subset`→`parse_u32/i32_array` — called EVEN without plugins and without collect; the lattice needs only word-params for Viterbi, not the full WordInfo → **potentially deferrable**, Vibrato does not fetch it at tokenize); #3 MeCabOov `provide_oov`. Vibrato: feature=lazy &str-slice, a tighter connector.

**Formula:** 3.4× ≈ ~1.27× (plugins) × ~1.39× (bytewise→charwise trie) × ~1.9× (impl: allocations/OOV/varint/lattice). Vibrato: feature = a `&str` slice (UTF-8, without allocation and UTF-16 decode), no plugin layer, reuse buffers.

**Port targets (format-preserving, by profile, ranked):** ① reduce allocations (arena/reuse for WordInfo strings and per-word Vec) — #1 by self-time; ② avoid `String::from_utf16` on every lookup (a UTF-8 cache/interning or a narrow `InfoSubset`) — **implemented in §5.6 (port #39)**; ③ lighten the eager fetch of WordInfo in path-rewrite (~21%) — **partially closed by #39**; ④ charwise trie (×1.39 lookup, Amdahl-cap). None gives 3× alone — the gap is cumulative (Vibrato is tuned end-to-end).

### 5.6 Port #39 — gating the `normalized_form` decode in `JoinNumericPlugin` [measured]

**Finding (§5.5):** `JoinNumericPlugin::rewrite_gen` decoded `normalized_form` (`String::from_utf16` + malloc) for **every** node on the path (`join_numeric.rs:97`), even though the string `s` affects the branch only when the node is numeric **or** its normal form is a single `","`/`"."` (UTF-16-length 1). On non-nominally-numeric text this is hundreds of extra decodes per sentence.

**Port (byte-identical gate):**
- A new O(1) accessor `WordInfo::normalized_form_len` → `StringsCache::normalized_form_len` returns the UTF-16 length **without** a decode: for a self-referencing normal form (the common case) — `headword_strptr().length`; otherwise a single `get_word_info_subset(HEADWORD)` (parse the headword, no `from_utf16`); for OOV — the length of the already-materialized string.
- The gate in `rewrite_gen`: decode `s` only if `ctypes ∈ {NUMERIC, KANJINUMERIC}` **or** `normalized_form_len == 1`; otherwise `s = ""`.
- **Correctness:** `StringPointer.length` is in UTF-16 codepoints (`strings.rs:41`), so `length != 1` ⟹ `s` cannot be either `","`, or `"."`, or `len()==1` → all branches that read `s` proceed identically when `s == ""`. Provably output-preserving.

**Empirical check of byte-identity** (CLI `-a`, full plugins, `JoinNumeric enableNormalize:true`):

| corpus | mode A | mode B | mode C |
|---|---|---|---|
| UD-GSD (543 sentences, 13–13.6k tokens) | ✅ identical | ✅ identical | ✅ identical |
| num-stress (commas/dots/full-width/phone numbers/%) | ✅ identical | ✅ identical | ✅ identical |

**Speed** (`do_tokenize`, NOCOLLECT, GSD, 31 trials × 3 repeats, M4 Max, min — the most stable):

| path / mode | clean (ns/char) | gated (ns/char) | Δ |
|---|---:|---:|---|
| scalar, mode A (median) | ~383 | ~354 | **−7.6%** |
| scalar, mode A (min) | ~360 | ~341 | **−5.3%** |
| scalar, mode C (min) | ~338 | ~321 | **−5.0%** |
| pipelined+prefetch, mode A (min) | ~784 *(noise rep1 cv 12.8%)* | ~739 | **−5.7%** |

The port is **orthogonal** to the trie-lookup (it lives in path-rewrite) → scalar and pipelined win equally. This is the **first positive format-preserving port** from the vibrato decomposition §5.5: it confirms that the plugin layer (~14% path-rewrite) is a real and removable source, and without changing the output. Files: `strings_cache.rs`, `word_info/data.rs`, `plugin/path_rewrite/join_numeric.rs`.

### 5.7 Port #40 + the subset methodology fix + a map of the path to 3× [measured]

**Methodology fix (important for the whole study):** `tokenize_pipeline_bench` did not call `set_subset`, i.e. it ran under `InfoSubset::all()` (the default of `StatefulTokenizer::new`). Production does not do this: the CLI `output.rs::subset()` requests `POS_ID|NORMALIZED_FORM` (without `-a`) or `+DICT|READING|SYNONYM` (with `-a`); Python passes its own `fields`. Under `all()` every best-path node redundantly `parse_u32_array`s all the split/word_structure/synonym arrays. The measured artifact on GSD: `all()`→`min` ≈ **−6% / −21 ns/char** (mode C, best-min). Added `SUDACHI_BENCH_SUBSET` (`all`/`min`/`pos`/`all_cli`); the realistic baseline = `min`.

**Port #40 (byte-identical):** in `WordInfoParser::parse` — an early exit when `flds` does not intersect with `{SPLIT_A,B,C, WORD_STRUCTURE, SYNONYM_GROUP_IDS, USER_DATA}`: we skip parsing/skipping the entire variable section (the tail of the record is no longer read; boundaries are validated at load). Fires for the CLI default and mode C. Byte-identical checked on 12 combinations (±`-a` × A/B/C × GSD/num-stress).

**Cumulative session trajectory (do_tokenize, GSD, best-min, realistic subset):**

| stage | mode C `min` | mode C `pos` | Δ vs base |
|---|---:|---:|---|
| base (this session, realistic subset) | 320.2 | 310.4 | — |
| +#39 (gate normalized_form) | 301.0 | 302.0 | −6.0% / −2.7% |
| +#39+#40 (early-out var-section) | **294.0** | **288.8** | **−8.2% / −6.9%** |

Two byte-identical ports = **~8–9%** on the realistic path. The current frontier ≈ **290 ns/char** against Vibrato 112 → **~2.6×** remaining.

**Map of the remaining gap (`do_tokenize` profile after #39+#40, mode C `min`, [measured]):**

| component | share | nature / reachability |
|---|---:|---|
| `Lattice::insert` (Viterbi min-plus + connection matrix) | **~25%** | matrix-volume-bound; #22–26 already showed negative/lossy → a byte-identical win is unlikely |
| `resolve_best_path` WordInfo materialization (owned per node, `resolve`/`memmove`/`free`) | **~20%** | Vibrato keeps nodes light (borrowed `&str`); requires a refactor of `ResultNode`/plugins |
| input-text plugins (Default/IgnoreYomigana/ProlongedSound) | **~10%** | rewrite the input; output-sensitive |
| path-rewrite (JoinNumeric+JoinKatakanaOov), ~55% — `from_utf16` | **~9%** | #39 cut the content words; single-character punctuation still decodes |
| OOV `provide_oov` (MeCabOov) | **~5%** | allocations |

**Honest conclusion on 3×:** no remaining **byte-identical drop-in** gives >~5% e2e. The gap to Vibrato is structural and cumulative: (i) charwise-trie (×1.39 *isolated*, but the trie is ~17% e2e per Amdahl → ~5–6% e2e; **format change**); (ii) light/borrowed lattice nodes + dropping the per-node owned WordInfo (**architecture refactor**, the largest chunk — ~20% + allocations); (iii) `Lattice::insert`/matrix (~25%, historically impenetrable without accuracy loss). 3× in the Sudachi architecture = the compound of all three (as in Vibrato), not a single port. The paradigm shift (Vaporetto, #6–7) gives 7–13× immediately, but changes the output (see §5.3 for the accuracy cost).

### 5.8 Path A (chosen): Vaporetto as a Sudachi replacement — matched benchmark [measured]

The decision at the §7.5 fork is the paradigm shift. Here is an honest **matched** comparison on GSD against the **already-optimized** Sudachi (#39+#40, realistic `min` subset), both emitting surface+POS.

**Speed (GSD, 21328 chars, M4 Max, best-min):**

| engine | ns/char | sent/s | vs Sudachi |
|---|---:|---:|---:|
| Vaporetto seg-only | **27.3** | 932k | **10.7×** |
| Vaporetto seg+POS (`fill_tags`) | **36.4** | 699k | **8.1×** |
| Sudachi e2e mode C (collect, min subset) | 293.5 | — | 1× |
| Sudachi e2e mode A | 331.0 | — | 0.9× |

**Conclusion:** even after the byte-identical ports brought Sudachi down to the realistic floor of ~293 ns/char, the paradigm is **still ~8× faster**. 3× is not just reached — it is exceeded threefold. This confirms §5.7: the gap is structural, and the fastest way to close it is not lattice optimization but abandoning the lattice.

**⚠️ Key correction — feature-recovery was measured [measured], not projected.** I expected +30–50 ns/char; the reality is **substantially more expensive**. The `feature_lookup_bench` benchmark (exact-lookup + `normalized_form`-fetch on 13258 Vaporetto segments, the Sudachi dictionary, subset `NORMALIZED_FORM`, after #40):

| metric | value |
|---|---|
| feature-recovery | **89.3 ns/char** (median; min ~83), **143 ns/token** |
| dict-hit rate | **99.1%** (13140/13258 found, 118 OOV) |

Decomposition [measured, best-min] — what exactly is expensive:

| stage | ns/char | adds |
|---|---:|---|
| exact-lookup only (trie common-prefix) | **12.95** | — |
| + fetch POS (`get_word_info_subset`) | 45.64 | +33 (parse fixed-data) |
| + decode `normalized_form` (full) | 88.82 | +43 (`from_utf16`+resolve ref) |

**Decomposition conclusion:** the trie-lookup is cheap (~13); expensive is the **WordInfo-fetch + `from_utf16`-decode (~76)** — this is **the same decode that Sudachi also pays**, unavoidable for `normalized_form` parity with ANY segmenter. So a "better exact-match API" will not save it (≤13 saved) — the floor is set by the decode. This also explains why the `normalized_form` decode surfaced in #39: a process-wide decode cache (Arc<str> per WordId, deferred plan #2) would help BOTH paths (both Sudachi and the Vaporetto-replacement) on repeated words.

**Result — Vaporetto AS A REPLACEMENT for Sudachi (full output parity seg+POS+normalized):**

| output mode | path | ns/char | vs Sudachi 293 |
|---|---|---:|---:|
| seg only | model | 27.3 | 10.7× |
| seg + POS | model | 36.4 | 8.1× |
| **seg + POS + `normalized_form`** (parity) | model + dict-lookup | **36 + 89 ≈ 125** | **~2.3×** |

**Conclusion (honest):** the paradigmatic 8× exists **only if normal forms are not needed**. As soon as we recover the features that Sudachi gives through the lattice, the dict-lookup costs ~89 ns/char, and the real speedup of the replacement falls to **~2.3×** — *plus* POS −3.96 and the loss of A/B/C. That is, the cost of the lattice partly **buys** these features; "pointwise is 8× faster" — for the task of segmentation+tagging, not for full morphological analysis.

**Other costs of the paradigm:**
1. **POS accuracy** [measured #21]: POS-top **93.50 vs 97.46** (−3.96). SEG-F1 97.07 vs 97.99 (−0.92).
2. **Multi-granularity:** pointwise gives ONE segmentation — no A/B/C from a single pass (a hard limitation).
3. **Integration:** the model is tied to the dictionary/language, needs retraining; ~58 MB.
4. (For full parity, add another input normalization ~30 ns/char, not counted in 125 → the real replacement is closer to ~1.9× at equal input.)

**What would remain (path A, deeper):** the dict-lookup can be sped up with an exact-match API (without common-prefix scanning) — potentially <89; evaluate the POS gap on a fine-tuned model. Harness: `/tmp/vapo64` (`v64`), `feature_lookup_bench`, scorer `/tmp/gold/score.py`.

### 5.9 Prototype #2 — process-wide decode-cache on both paths [measured]

§5.8 showed: the feature-recovery floor is the **`from_utf16` decode of `normalized_form` (~43 ns/char)**, and BOTH paths pay it when emitting normal forms. The planned #2 (a process-wide decode cache) hits exactly that.

**Prototype:** a per-thread `WordId → &'static str` cache in `WordInfo::normalized_form` (env `SUDACHI_NF_CACHE`, one binary measures off/on). The per-instance `StringsCache` does not hit between nodes (a fresh `WordInfo` per node/lookup); the per-thread cache deduplicates the decode of repeated words (function words, frequent kanji). The strings leak (bounded by the number of unique WordIds; the prod version would own them in the dictionary). Content-identical: the totals matched off/on.

**Result on both paths (GSD, best-min):**

| path (emits `normalized_form`) | cache OFF | cache ON | Δ |
|---|---:|---:|---|
| Vaporetto feature-recovery (§5.8) | 89.7 ns/char | **49.8** | **−44%** |
| Sudachi full-analysis e2e + NF-output | 326.8 ns/char | **285.1** | −13% |

Both save ~40 ns/char (the very same decode). The relative win is larger for Vaporetto (a smaller base).

**Consequence — #2 takes the full replacement THROUGH 3×:**

| output seg+POS+`normalized_form` | cache OFF | cache ON |
|---|---:|---:|
| Sudachi e2e + NF | 326.8 | 285.1 |
| Vaporetto+dict (36.4 + recovery) | 126.1 | **86.2** |
| **ratio** | 2.6× | **3.3×** |

**Conclusions [measured]:**
1. **#2 speeds up BOTH paths** by the decode amount (~40 ns/char) — this is the first lever common to both the lattice and pointwise.
2. For **seg+POS-only** Sudachi, #2 is useless (there is no decode — `normalized_form` is lazy and not touched); it helps only when emitting normal forms (search/indexing, CLI `-a`).
3. **With #2 the full Vaporetto+dict replacement reaches 3.3×** (vs 2.6× without the cache) — finally breaking 3× for output parity (but the cost is unchanged: POS −3.96, no A/B/C).

**Level 2 — word_id cache on the Vaporetto path [measured]:** a `word_id → normalized_form` cache, checked BEFORE `get_word_info_subset` → on a hit the whole fetch+decode is skipped (not just the decode). Decomposition of feature-recovery (best-min ns/char):

| mode | ns/char | what it does |
|---|---:|---|
| lookup only (trie) | 12.5 | exact-match |
| + POS-fetch | 44.6 | + parse fixed |
| + decode (full, no cache) | 90.3 | base §5.8 |
| **word_id cache** | **18.1** | skip fetch+decode on a hit (>trie by ~5.6) |

**Full Vaporetto+dict replacement with the word_id cache [measured]:**

| output seg+POS+`normalized_form` | ns/char | vs Sudachi |
|---|---:|---:|
| Sudachi e2e+NF, decode-cached (floor) | 285.1 | 1× |
| Sudachi e2e+NF, no cache | 326.8 | — |
| **Vaporetto+dict (36.4 + 18.1 word_id-cached)** | **54.5** | **5.2× vs cached / 6.0× vs uncached** |

**~5× confirmed (measured).** The asymmetry is fair: the lookups of the Vaporetto path are *pure* feature recovery → the cache removes them on a repeat; Sudachi cannot skip WordInfo materialization (the lattice needs it), its floor stays ~285.

**Level 3 — surface→normalized cache [measured]:** the key = the surface string itself → on a hit EVEN the trie-lookup is skipped (only the string hash). The most aggressive — natural for a pointwise pipeline that already has the segment text on hand.

**Full caching ladder of Vaporetto feature-recovery [measured, best-min]:**

| recovery cache | recovery ns/char | Vaporetto-full (36.4+rec) | vs Sudachi (285 cached / 327 uncached) |
|---|---:|---:|---:|
| none | 90 | 126.6 | 2.3× / 2.6× |
| #2 decode | 50 | 86.2 | 3.3× / 3.8× |
| word_id | 22 | 58.9 | 4.8× / 5.5× |
| **surface** | **7.6** | **44.0** | **6.5× / 7.4×** |
| asymptote (recovery→0) | →0 | 36.4 | 7.8× / 9.0× |

**Synthesis:** the more aggressive the feature cache, the closer the full-parity Vaporetto replacement gets to the **pure 8× of the segmenter** (= the floor = Vaporetto seg+POS itself, 36.4). The surface cache (6.5–7.4×) is almost at the asymptote — it is bare seg+POS plus a string hash. **The ~6.8× from the projection is confirmed** (taken in the band 6.5–7.4× depending on whether Sudachi also caches). The Sudachi floor ~285 is unavoidable (the lattice + WordInfo materialization is the byte-identical wall §5.7), no matter how much the features are cached.

4. **(B) The prod version without leaks + thread-safe — the win survives [measured].** The prototypes leak (`Box::leak`) and are not Sync (`RefCell`). The production variant of the surface cache (owned `Arc<str>` without leak + a `Mutex` for Sync):

| surface cache | recovery ns/char | Vaporetto-full | vs Sudachi 285 |
|---|---:|---:|---:|
| no cache | 100.6 | 137.0 | 2.1× |
| leaky prototype (`&'static`+RefCell) | 7.0 | 43.4 | 6.6× |
| **prod (`Arc<str>`+`Mutex`, without leak)** | **10.0** | **46.4** | **6.1×** |

The machinery (lock + Arc-clone + owned key) costs **+3 ns/char**; the win is preserved (**6.1×** vs 6.6×, single-thread). Leak-as-intern-pool is also valid for a long-lived process (bounded by the dictionary, ~a few MB).

**Multithreaded scaling [measured]** (M4 Max 12 P-cores, T threads run the corpus ×25, aggregate throughput Mchar/s, `cache_mt_bench`):

| strategy | T=1 | T=4 | T=12 | scaling |
|---|---:|---:|---:|---|
| **global `Mutex`** | ~55 | ~17 | ~24 | **0.3–0.4× — COLLAPSE below 1 thread** |
| **`DashMap`** (sharded) | ~72 | ~96 | ~95 | 1.2–1.4× (safe, modest) |
| **`thread_local`** (per-thread) | ~92 | ~323 | ~833 | **8.6–9.5× — almost linear** |

**Verdict:** the global `Mutex` is a **lock convoy**: at T≥4 the throughput *drops below single-thread* (the critical section is tiny — hash+Arc-clone — all the time goes into the lock). `DashMap` is safe but gives only ~1.3× (the per-item work is too small relative to the shard-lock+Arc-refcount). **`thread_local` scales almost linearly** (8.6–9.5× on 12 cores) at the cost of N× memory (bounded by the dictionary) + the absence of cross-thread reuse (each thread cold-fills its own cache, amortized over the lifetime). **Production recommendation: a per-thread cache** (`thread_local`), not a shared lock and not DashMap. This is consistent with the fact that multithreading (#5, ~8.9×) is the largest lever: the cache must not interfere with it.
5. **Important caveat:** the cache win is proportional to the **repetition of surfaces**. GSD (543 sentences) — high repetition (function/frequent words) → a warm cache hits almost everything → 6.5×. On heterogeneous/short input, recovery lies between 7.6 (all hits) and 90 (all misses) ns/char; a "cold" cache = the base 2.3×. The ladder numbers are steady-state on a repetitive corpus (typical for batch processing/indexing).

### 5.10 (A) Diagnosis of the POS-gap — it is not fundamental [measured]

Question: is −3.96 POS (#21/#42) the paradigm, an artifact of the tagsets, or the POS source? Analysis:

1. **The tagsets are aligned [measured].** The Vaporetto tag (`名詞-普通名詞-形状詞可能`) is format-IDENTICAL to the gold XPOS; the scorer takes `split("-")[0]` of both → the comparison is fair, not an artifact. So −3.96 is real for MODEL-POS.
2. **Root cause:** Sudachi does not predict POS but **takes it from the dictionary** (the gold UD-GSD is annotated with the same UniDic → almost ideal, conditional 99.5%); Vaporetto **predicts** POS with a model (conditional 96.3%).
3. **Key:** the full Vaporetto+dict replacement does the dict-lookup anyway (for `normalized_form`) → it can take **dict-POS instead of model-POS**. Measured on the Vaporetto segmentation:

| POS source | POS-top F | gap vs Sudachi 97.46 |
|---|---:|---:|
| MODEL tag (prediction) | 93.50 | −3.96 |
| DICT in-context (95.1% of spans matched Sudachi) | 95.07 | −2.39 |
| **HYBRID (dict where the span matched, otherwise model)** | **96.27** | **−1.19** |

**Conclusion (A): the POS gap is not fundamental.** Most of −3.96 is from MODEL-POS; by taking dict-POS (which the pipeline computes anyway), the gap shrinks to **−1.19**, and the remainder is mostly segmentation propagation (a wrong span → a wrong POS). On correctly segmented tokens, dict-POS gives conditional ≈99.2% ≈ Sudachi 99.5%. **Retraining is not needed** — the dictionary (already on hand for `normalized_form`) gives POS almost at the Sudachi level; the model POS is needed only for disambiguating homographs (its 93.5% is enough to pick among the dict candidates).

**Result of the honest cost of the Vaporetto+dict replacement (with dict-POS):** SEG **−0.92**, POS-top **−1.19** (not −3.96) — an order of magnitude smaller. What remains hard: **no A/B/C** (one segmentation) + retraining the model for the language/dictionary. Script: `/tmp/gold/score_vapo_dictpos.py`.

### 5.11 Microarchitectural analysis of `connect_node` — the matrix is NOT DRAM-bound (premise refutation) [measured]

A fresh angle "below the assembler": an 18-agent analysis of the hot kernel `Lattice::insert`/`connect_node` (Viterbi min-plus, ~22–25% do_tokenize). The hypothesis was that the 71MB matrix is DRAM-latency-bound, and that batching connect_node by the boundary's right-nodes would deepen the MLP. **The hypothesis is refuted, the study's premise is corrected.**

**Disasm (release ARM64): the codegen is optimal.** The invariant `R.left_id*num_left` + data-ptr are hoisted out of the loop; `ldrsh` without a bounds-check (`get_unchecked`); min+argmin is fully branchless (3× `csel`); the matrix load is address-independent between iterations → **already MLP-capable**. The assembler is not a lever.

**Empirics (GSD, instrumented connect_node):** 173,803 calls, 1,506,919 matrix lookups, **avg 8.67 left-nodes/call** (max 64); 27% of calls ≤4 nodes. **BOS-skip = 0%** on a fully reachable corpus → the `is_connected_to_bos` branch is always not-taken (dead weight, but ~free).

**Three independent pillars of refutation:**
1. **LRU simulation** on the exact trace of 1,506,919 lookups: a total of **30,916 unique 128B lines = 3.77 MiB working set**, miss 2.05% **at both 4MB and 16MB L2, ZERO capacity-misses** (all compulsory) → 97.95% of loads are L2-hits. Skew: only **1474/5981** conn-id touched; top-16 rows = 47%, top-256 = 92.5%. Conn-id are derived from POS (a bounded Zipf dictionary) → the working set is L2-resident for natural text.
2. **nomat probe** (the matrix load replaced by ALU, the loop shape/VNode reads/min-argmin preserved — verified by the morpheme-count shift 12375→12572): baseline 6.67ms vs nomat 6.19ms = **1.0775×** → the whole 71MB matrix = **≤7.2% do_tokenize**. Removing the load entirely is the theoretical ceiling of any latency-hiding (batch/prefetch only reorder, they do not remove).
3. **Cycle arithmetic:** 5.42 cyc/load (within ~1.7× of the issue-floor 3.25); serial DRAM at depth 8.67 would give 150.7ms ≫ the whole pass of 6.67ms (physically impossible). The L2-hit model (4ns × 8.67) → 0.70ms ≈ the observed delta 0.5–0.8ms.

**Conclusion: the kernel is on the byte-identical floor.** The matrix is L2-resident and **throughput/issue-bound, not latency-bound**. The ceiling of ANY matrix-locality/MLP/prefetch optimization = ~7.2%, in reality **~0%** (batching hits a non-problem; expectedly net-negative — 8–44 spilled accumulators + recomputing the row-base break the dense 13-instr loop, exactly like the #24 failure −12%). **Premise correction:** the earlier "the matrix is 22.8%, volume/DRAM-bound" (§6) is the connect_node-LOOP (1.5M iterations × cheap work), while the matrix LOAD inside is only ≤7.2% and L2-resident; the conclusion "not improvable byte-identical" stands, but the mechanism was the wrong one.

**Redirect:** the remaining ~93% do_tokenize is outside this kernel and outside the matrix/MLP lens: VNode parallel-array pushes (`insert` L131-133), trie/lexicon lookup, OOV, path-rewrite (the last already cut by #39/#40). None gives a big lever in the matrix/MLP lens — but **later the WordInfo cache (#52) found a large one precisely in WordInfo materialization** (parse+resolve ~17%, recovered with a shared `Arc` cache): **+27% on default output and +44% on full `-a`** [vs the pre-cache base; the prototype estimate was +20–21%], raising the byte-identical ceiling to ~1.4–1.6×. **A diagnostic technique** (for a paper/regressions): the nomat probe + LRU-sim is a reproducible way to prove "kernel at floor" (the artifacts are reproducible; see the synthesis of the `lattice-microarch` workflow).

### 5.12 Real fused-prototype Vaporetto→Sudachi-dict — authoritative numbers + a correction [measured]

§5.8–5.9 gave the "full replacement" as the arithmetic `v64(36) + feature-recovery(7.6) = 44 → 6.5×`. **This was a projection that mixed profiles.** A REAL fused binary was assembled (`/tmp/vapo-sud`: vaporetto 0.6.5 + sudachi path-dep in one crate, LTO, Sentence-reuse): Vaporetto segments+`fill_tags`, and per token does an exact dict-lookup (`normalized_form` + dict-POS), emitting output in the Sudachi form. One binary, internally consistent.

**Correction (what the real build revealed):** the `v64` number 36 ns/char was `predict()` **without `fill_tags` and without materializing tokens** — an undercount. The real seg+POS (with tags extracted) = **~71 ns/char**.

**Speed (GSD, best-min, fused binary):**

| variant | ns/char | note |
|---|---:|---|
| vaporetto-only (seg+POS, materialized) | **~71** | was mistakenly 36 |
| fused, no cache | ~182 | the dict-lookup per token is expensive (+110) |
| **fused + surface-cache (full output parity)** | **~78** | dict-recovery cached **+7–9** (validates feature_lookup_bench 7.6) |

→ **full replacement ≈ 78 ns/char vs Sudachi ~290 = ~3.7×** (NOT 6.5×).

**Quality (real fused output, scored vs gold):**

| POS source | GSD-test | GSD-dev | meaning |
|---|---:|---:|---|
| SEG-F1 | 97.07 | 96.16 | −0.92 vs Sudachi |
| POS model (prediction) | 93.50 | 92.31 | −3.96 |
| POS **dict-first** (without disambiguation) | **69.76** | **70.61** | ⚠️ homograph trap — you CANNOT take the first entry |
| POS **hybrid** (dict-candidates, disambiguated by the model) | **95.72** | 94.40 | −1.74; the realistic fast path |
| normalized produced | 13258/13258 | 12592/12592 | 100% |

**Two engineering discoveries from the real artifact:**
1. **The full replacement is ~3.7×, not 6.5×** — the projection was optimistic (v64 did not materialize the tags). The component numbers (seg 27 / recovery 7.6) are valid separately, but their SUM was wrong as a "full replacement".
2. **Dict-POS requires disambiguation by the model.** Taking the first exact entry = POS 70 (homographs). Hybrid (dict-candidates ∩ model-POS) = 95.7. That is, the fused pipeline must use BOTH: Vaporetto-POS (to pick among candidates) + the dictionary (normalized + POS-candidates).

**Robustness (GSD-test/dev + kyoto-leads news 461k):** speed **71–113 ns/char** (shorter sentences → higher per-sentence overhead), ~3.5–4× vs Sudachi; SEG **96–97**, hybrid-POS **94–96**; the dict-first trap (~70) is consistent. The numbers hold, they do not collapse. Artifact: `/tmp/vapo-sud` (runnable), scorer `/tmp/gold/score_fused.py`.

**Result of the honest cost of replacement (corrected):** speed **~3.7×** (not 6.5×), SEG **−0.92**, POS-top **−1.74** (the fast hybrid; −1.19 with in-context disambiguation §5.10), hard constraints — no A/B/C + retraining. >3× is still true, but the order is 3–4×, not 6–8×.

## 6. Discussion

**Convergence.** Four independent rounds of literature research (data
structures; SIMD/asm; build-time layout; load-count) + an assembly-level analysis of the walk +
empirical prototypes gave the same conclusion: **the byte-indexed darts-clone walk is on the
practical floor** — 1 dependent load/byte, optimal codegen, latency-bound;
the format is already cache-conscious (XOR puts all children in a 256-unit window); the only
unclosed axis is the **number of loads**, cuttable only by charwise indexing (the format).

**What worked.** prefetch K=4 (the original L1 idea #117, measured +1.20–1.25× iso /
+5–17% e2e); varint (+2.7%); both shipped and exact. This study added two
new byte-identical ports from the vibrato decomposition (§5.5): **#39** (gating the
`normalized_form` decode in JoinNumeric) + **#40** (early-out for the variable section of
WordInfoParser) = **~9%** on a realistic subset (§5.6–5.7). **multithread ~8.9×** —
the largest format-preserving lever. **Vaporetto** reproduces the literature 7–9×.

**Quality (golden benchmark).** For the first time, Sudachi's accuracy was measured: mode A
**SEG-F1 97.99 / POS-top 97.46**. The paradigmatic trade-off was honestly quantified and
**refined** (§5.10): the previously apparent POS −3.96 is an artifact of MODEL-predicted POS;
with **dict-POS** (the dictionary is on hand anyway for `normalized_form`) the gap = **−1.19**.
Result of the cost of the Vaporetto+dict replacement: SEG −0.92, POS −1.19, hard constraint — no A/B/C.

**Vaporetto as a replacement — the ladder (§5.8–5.9).** Bare seg+POS **8.1×**; the full replacement
(output parity) **2.3× without cache → 3.3× decode-cache (#2) → 6.5× surface-cache**,
asymptote → 8× (= the segmenter itself). The decode cache (#2, ~40 ns/char) — **the first lever
common to both the lattice and pointwise**; the prod version without leaks + thread-safe survives (6.1×).
The cache win ∝ the repetition of surfaces (steady-state on a batch corpus).

**Main negative lessons.** The `connect_node` loop is the #1 hotspot (~22–25%), but
**the mechanism is corrected (§5.11):** it is not DRAM/volume-bound but L2-resident and
throughput/issue-bound — the live working set is only **3.77 MiB** (0 capacity-misses, 97.95%
L2-hits; 1474/5981 conn-id touched), and the matrix load itself = **≤7.2% do_tokenize**
(nomat probe); the kernel is on the byte-identical floor, the codegen is optimal. Therefore matrix
prefetch/quant/relayout/MLP-batching hit a non-problem (this explains #24 −12%).
The layout tricks tested on the trie are either dead (vEB), or inverse (code-dividing),
or backfire for Japanese (path-compression). Caveat: a corpus-frequency
prefetch-friendly relayout of the double array (the original idea #117) **is not prototyped
empirically** — it is closed only by literature (N7/N8: yada builds from sorted keys, relayout
≈ ≤1.10× lookup at ~14× build); as a direction it remains open, especially if the
patterns turn out to be more accessible to the hardware prefetch (potentially also for the Java version).

## 7. Conclusions and recommendations

1. **Ship as-is** prefetch K=4 + varint (PR #348) — the bulk of the
   available format-preserving single-threaded win, exact.
2. **For throughput** — multithreading (~8.9×), which composes with everything.
3. **The only trie lever** — charwise crawdad (+1.37× iso / ~+6–10% e2e [proj],
   a format change); the prototype integration was assessed but not brought to an e2e number.
4. **The pointwise paradigm (Vaporetto)** — measured (§5.8–5.10, #41–45):
   **8.1× for seg+POS**; as a full replacement (with `normalized_form`) — **2.3× without cache,
   3.3× with a decode cache (#2), up to 6.5× with a surface cache** (§5.9, depends on
   repetition). The quality cost is **not −3.96 but SEG −0.92 / POS −1.19** with dict-POS
   (§5.10); what remains hard is only the loss of A/B/C + retraining. The "multipliers" = "doing less".
5. **The main synthesis (corrected #48/#52):** 3× while preserving the FULL Sudachi
   output is unreachable byte-identical (ceiling **~1.4–1.6×**: #39/#40 + the **WordInfo cache** #52,
   +27% default / +44% full `-a`; the lattice kernel is on the floor §5.11, but WordInfo materialization is not). The paradigm (the real
   fused) = **~3.7×**, changes the output (SEG −0.92, POS −1.74, no A/B/C) = task reduction.
6. **The binding constraint — Java compatibility (decisive).** sudachi.rs
   contractually reproduces the output of Java-Sudachi (A/B/C, exact POS/boundaries). The paradigm
   breaks this → **not an option for sudachi.rs**, only a separate tool. The real
   path for the library: byte-identical wins (#39/#40) + multithreading (~8.9×);
   do not touch the matrix/lattice (§5.11). Details — `paradigm-proposal.md`.
7. **Do not repeat** N1–N16 (§5.4).

### 7.5 Roadmap: the chosen path and the deferred directions

After #39+#40 (~9% byte-identical, the frontier ~290 ns/char do_tokenize, a realistic subset) there is a fork to 3× (Vibrato 112 ns/char). **Decision (2026-06-13): we go to the Vaporetto paradigm shift** — the only path with a proven ≥3× (in reality 7–13×). The other three are **deferred but viable** — return here if the paradigm does not fit by accuracy/output:

| # | direction | expected win | byte-id? | scope/risk | where to start |
|---|---|---|---|---|---|
| **A (chosen)** | **Vaporetto pointwise** | **7–13× (#6–7)** | ❌ changes the output | the model is ready (`/tmp/vapo/model.raw`); harness `/tmp/vapo64` | §5.7 + current work: an honest matched full-pipeline bench |
| B (deferred) | borrowed lattice nodes (dropping owned WordInfo per node; `&str` slices like Vibrato) | ~1.3–1.6× (attacks ~20% `resolve_best_path` + allocations) | ✅ possible | a **large refactor** of `ResultNode`/plugins/`MorphemeList`; risk to identity | profile §5.7; goal — remove `memmove`/`free`/`resolve` per-node |
| C (deferred) | charwise-trie (crawdad) | ×1.39 iso / ~5–6% e2e | ✅ (but a dictionary format change) | medium; needs a re-encode of the dictionary + a trie swap | §5.0 #11; integration assessed, not brought to e2e |
| D (dead end) | `Lattice::insert`/connection-matrix (~25%) | — | — | historically negative/lossy (#22–26); the matrix is volume-bound | DO NOT go here without changing the output |

**Prototype state (for the return):** the byte-identical ports #39+#40 live in the worktree `/tmp/sud-vib` (branch `vib-cmp`, off `feat/117-runtime-prefetch`), 4 files: `dic/strings_cache.rs`, `dic/word_info/data.rs`, `dic/word_info/parse.rs`, `plugin/path_rewrite/join_numeric.rs` + the bench `examples/tokenize_pipeline_bench.rs` (env `SUDACHI_BENCH_SUBSET`). Not committed, no PR made (by request — for now only the benchmark). 3× in byte-identical mode is recognized as unreachable (the ceiling = matrix ~25% + the architecture of owned nodes); the compound B+C+allocations would give Vibrato's ~3×, but that is months of refactoring.

## 8. References (peer-reviewed sources)

- M. Farrar. *Striped Smith-Waterman speeds database searches.* Bioinformatics 23(2):156–161, 2007.
- S. Marco-Sola et al. *Fast gap-affine pairwise alignment using the wavefront algorithm (WFA).* Bioinformatics 37(4):456–463, 2021.
- Ferreira/Roma/Russo. *Inter-task SIMD Viterbi (COPS/HMMER).* BMC Bioinformatics 15:165, 2014.
- Chen et al. *Exact Lattice Generation on GPU (Kaldi WFST).* Interspeech 2018.
- André, Kermarrec, Le Scouarnec. *Cache locality / Quick(er) ADC (PQ Fast Scan).* VLDB 2016 / ICMR 2017 / IEEE TPAMI 2019.
- Wang et al. *Hyperscan (FDR/Teddy).* USENIX NSDI 2019.
- D. Lemire, N. Kurz, C. Rupp. *Stream VByte.* Information Processing Letters 130, 2018.
- V. Leis, A. Kemper, T. Neumann. *The Adaptive Radix Tree (ART).* IEEE ICDE 2013.
- Barratt & Zhang. *Cache-Friendly Search Trees.* arXiv:1907.01631, 2019.
- Khuong & Morin. *Array Layouts for Comparison-Based Searching.* ACM JEA / SEA 2017.
- Brodal, Fagerberg, Jacob. *Cache Oblivious Search Trees via Binary Trees of Small Height.* SODA 2002.
- Bender, Demaine, Farach-Colton. *Cache-Oblivious B-Trees.* FOCS 2000 / SICOMP 2005.
- Bender et al. *The Cost of Cache-Oblivious Searching.* Algorithmica 61(2), 2011.
- Lindstrom & Rajan. *Optimal Hierarchical Layouts for Cache-Oblivious Search Trees (MinWEP).* IEEE ICDE 2014.
- Rao & Ross. *Cache Conscious Indexing (CSS) / Cache-Sensitive B+-Trees (CSB+).* VLDB 1999 / SIGMOD 2000.
- Yoshinaga & Kitsuregawa. *A Self-adaptive Classifier... (cedar, XOR double-array).* COLING 2014.
- Kanda, Morita, Fuketa. *Compressed double-array tries (XCDA).* Knowledge and Information Systems 51:1023–1042, 2017.
- Yata et al. *A compact static double-array / minimal-prefix double array.* IP&M 43(1) / SPE 37(5), 2007.
- Zeitak & Morrison. *Cuckoo Trie: Exploiting Memory-Level Parallelism.* SOSP 2021.
- Liu et al. *Compression Methods... Code Dividing for Chinese DA-trie.* IJCNLP 2011.
- *StriD2FA / stride-k DFA.* IEEE ICC 2011 / JNCA 2015.
- Kudo, Yamamoto, Matsumoto. *Applying CRFs to Japanese Morphological Analysis (MeCab).* EMNLP 2004.
- Neubig, Nakata, Mori. *Pointwise Prediction for... Japanese MA (KyTea).* ACL 2011.
- Takaoka et al. *Sudachi: a Japanese Tokenizer...* LREC 2018.
- Akabe, Kanda, Oda, Mori. *Vaporetto: Efficient Japanese Tokenization...* 2024 (arXiv:2406.17185).
- N. Yoshinaga. *Back to Patterns: Efficient Japanese MA with Feature-Sequence Trie (Jagger).* ACL 2023.
- Kanda, Akabe, Oda. *Engineering faster double-array Aho-Corasick (daachorse).* SPE 53(6):1332–1361, 2023.
- *UD_Japanese-GSD treebank* (Universal Dependencies); NINJAL *BCCWJ* SUW/LUW scheme.

## Appendix A — reproduction

```bash
# Quality (golden benchmark)
wget https://github.com/UniversalDependencies/UD_Japanese-GSD/raw/master/ja_gsd-ud-test.conllu
python score.py            # Sudachi A/B/C SEG/POS-F1 vs GSD; writes gsd_text.txt
VAPO_MODEL=model.raw VAPO_INPUTS=gsd_text.txt VAPO_DUMP=1 v64 > vapo_out.txt
python score.py            # + Vaporetto SEG-F1

# Speed e2e
SUDACHI_BENCH_DICT=full/system_full.dic SUDACHI_BENCH_TRIALS=31 \
  cargo run -p sudachi --release --example tokenize_pipeline_bench

# Isolated lookup (cross-method)
SUDACHI_TRIE_BENCH_SURFACE_LEXICONS=small.csv:core.csv:notcore.csv \
  cargo run -p sudachi --release --example dictionary_matcher_report --features matcher-comparison

# do_tokenize over a realistic subset + #39/#40 + decode-cache (#2)
SUDACHI_BENCH_SUBSET=min SUDACHI_BENCH_MODE=C SUDACHI_BENCH_NOCOLLECT=1 \
  SUDACHI_NF_CACHE=1 SUDACHI_BENCH_ACCESS_NF=1 \
  cargo run -p sudachi --release --example tokenize_pipeline_bench
# subset: all|min|pos|all_cli ; SUDACHI_NF_CACHE — decode-cache; ACCESS_NF — emit norm-forms

# Vaporetto path A: feature-recovery + the cache ladder (full/pos/lookup/cache/scache/scache_prod)
VAPO_MODEL=model.raw VAPO_INPUTS=gsd_text.txt VAPO_DUMP=1 v64 | tr '\t' '\n' | grep -v '^$' > vapo_surfaces.txt
SUDACHI_BENCH_INPUTS=vapo_surfaces.txt SUDACHI_BENCH_FEAT=scache \
  cargo run -p sudachi --release --example feature_lookup_bench

# POS-gap diagnosis (A): model-POS vs dict-POS vs hybrid on the Vaporetto segmentation
python score_vapo_dictpos.py

# Cache multithreading: Mutex vs DashMap vs thread_local (T=1..12)
SUDACHI_BENCH_INPUTS=vapo_surfaces.txt SUDACHI_BENCH_THREADS=1,2,4,8,12 SUDACHI_BENCH_REPEAT=25 \
  cargo run -p sudachi --release --example cache_mt_bench   # requires dashmap (dev-dep)
```

**Artifacts:** `bench/quality/{test.conllu,score.py,gsd_text.txt}`; results —
`/tmp/gold/` (+`score_vapo_dictpos.py`, `vapo_surfaces.txt`), the Vaporetto harness
`/tmp/vapo64` (`v64`), the worktree `/tmp/sud-vib` (`vib-cmp`: #39/#40/#2 +
`feature_lookup_bench` with the cache modes). The prototypes are not committed (no PR made —
the task was to measure). The provenance of each number — the tags [measured]/[doc]/[literature]/[proj] in the header.
