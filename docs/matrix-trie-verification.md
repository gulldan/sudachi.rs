# PR #348 — reviewer-hypothesis audit, verified

Factual, reproducible check of `@eiennohito`'s review of
<https://github.com/WorksApplications/sudachi.rs/pull/348>, plus the claims in
`docs/optimization-study.md`.

**Method & integrity (bounds every number below):**
- Every result is a **causal experiment** (change one thing, measure the effect).
  **No profiler attribution is used anywhere** — this is a direct response to the
  review note that `perf annotate` / `sample` mis-attribute DRAM stalls on OoO cores.
- Every claim was then **independently re-verified by a separate adversarial reviewer
  in a clean context** (given only the probe code + the claim, asked to break it).
  Where that pass changed a number or framing, it is recorded in §1.
- Timing A/B is always **in-process, per-trial interleaved, paired sign test** (kills
  cross-process thermal drift). Probe atomics inflate *absolute* ns vs a pristine
  build → only **relative/paired** numbers are claimed. Deterministic outputs (GSD F1,
  working-set sizes, parity hashes, hit counts) are not timing-sensitive.
- Several methodology errors were caught and fixed along the way — e.g. cross-process
  drift (faked −5%) and a sequential "packed" pointer-chase (faked 31×); §1 records the
  corrections the independent verification pass forced.
- Environment: Apple **M4 Max** (L1d 128 KiB, **L2 16 MiB**, 128 B lines, 48 GB),
  `rustc 1.96.0 --release`. Dict: SudachiDict full (matrix **5981²×i16 ≈ 71 MB**).
  Speed corpus `kyoto-leads` (16,051 sents / 461,815 chars). Quality
  `UD_Japanese-GSD` test (543 sents, mode-A SEG-F1). Cross-arch (Zen4) numbers are
  predictions, not measured.

---

## 0. Bottom line

The reviewer's **diagnosis is right and his worst-case framing is wrong.** There is a
real, profiler-hidden matrix cost — but it is **L2 latency, not a DRAM-bound 71 MB
bottleneck.** His architectural fix (separate node creation from the trie walk) is
correct and is in fact where PR #348's speedup comes from — **not** the prefetch the PR
is named after. His "relearn/simplify the matrix" is **partially supported** (the
trained matrix carries large removable redundancy) but a faithful retrain is **not
possible in this repo** and any speed win is small. Build-time trie relayout (issue
#117) is **feasible and output-safe but yields ~0 end-to-end.** "MOST of the analysis
is incorrect" is overstated — the load-bearing measurements hold.

Per-comment verdicts: §2. Evidence + numbers: §3. What independent verification
corrected vs the prior single-context analysis: §1.

---

## 1. Corrections forced by independent verification (read first)

**1.1 The L1 result does NOT "overturn" the block/low-rank result.** They are
*different operators* and are **logically consistent**: block/low-rank = uniform
distortion (smears the large discriminative deviations → signal lost); L1 soft-
threshold = magnitude-selective (keeps base + large deviations, drops only small ones
→ near-lossless). Both correct; neither overturns the other.

**1.2 Pure-additive floor is F1 94.54, not 95.88.** In the L1 sweep λ=6400 is ~99.97%
zeroed (95.88); true 100% (pure `a[l]+b[r]`) is ~λ=24000 at **94.54**.

**1.3 Phase-separation breakdown was mis-apportioned.** Pure separation (lanes=1,
prefetch=0) captures ~the **entire** ~18% win; MLP and prefetch add **~0** (not
"+13% / +1% / +1%"). Conclusion right, breakdown wrong.

**1.4 Trie-layout headroom is ~1.85×, not 2.3×** (chase ratio range 1.64–2.19×;
working-set sizes were exact). And "packed = best any layout" is an upper bound.

**1.5 L1 "no loss" is post-hoc and single-metric** — "residual cells the Viterbi
argmax already ignores on GSD mode-A are droppable", weaker than "a trainer should
never learn them". POS / modes B,C / other corpora untested.

---

## 2. Verdict table — mapped to the review comments

Legend: ✅ reviewer right · ❌ refuted · 🟡 partial. All verdicts were independently
re-verified in a clean context (§ Method). Numbers live in §3 — cells give the verdict
+ reason + pointer.

| # | @eiennohito said | Verdict | Why (→ detail) |
|---|---|---|---|
| C1 | PR is "halfway there" (not full #117) | ✅ | #117 = build-time layout; PR = runtime. Both agree. |
| C2 | node creation **after** trie walk, not interleaved (L1 contention) | ✅ | the PR's win is this separation, not the prefetch (§3.3). |
| C3 | relayout the trie itself; even Java free wins | 🟡 | real corpus-freq build is byte-identical but end-to-end ~0 (§3.4). |
| C4 | "free perf gains welcome" | ✅ | PR code uncontested. |
| C5 | too much unfiltered Claude output | 🟡 | partly: the study's `nomat` probe is unreliable and its working-set figure was ~2× low (§3.1). |
| C6 | "MOST of your analysis is incorrect" | ❌ | overstated — load-bearing measurements hold; only the matrix interpretation + `nomat` were wrong. |
| C7 | misattributed **DRAM** latency, "trashes all cache" | 🟡 | not DRAM (refuted), but a real hidden **L2** cost exists (§3.1). |
| C8 | matrix overlaps WordInfo (OoO); perf misattributes | 🟡 | OoO-hiding confirmed; "with WordInfo" imprecise — the cost persists in WordInfo-free do_tokenize (§3.1). |
| C9 | correct fix = relearn from scratch / simplify | 🟡 / open | post-hoc shows ~94% residual is removable (his intuition); faithful retrain impossible in-repo (§3.2, §3.7). |
| C10 | matrix = i16 CRF weights | ✅ | confirmed in code; Kudo 2004 + MeCab (§3.2). |
| C11 | L1 regularization skips noisy features | ✅ post-hoc / open | post-hoc L1 sparsification is lossless on GSD (§3.2); training-time untested. |

---

## 3. Evidence by area

### 3.1 Connection-matrix memory (E1/E2/E3)
Env-controlled index mask folds every matrix access into a small footprint with the
**identical instruction stream and load count**. Independent audit proved the load
count and lattice topology are built from trie hits + OOV only — **never from
connection costs** — so masking cannot change the workload; the measured Δ is pure
cache latency.
```
71MB → 4MB  : ~0%      (21/31)   → matrix is L2-resident, NOT DRAM-bound  [E1]
71MB → 16KB : −7…−9%   (50/51)   → real, profiler-hidden L1-recoverable cost
nomat       : +3% SLOWER (5/51)  → the study's probe is unreliable        [E2]
working set : 342,689 cells = 6.39 MiB (64B) / 8.80 MiB (128B)            [E3]
```
The 7–9% is an upper bound (real working set lives in L2 on M4); on Zen4 (1 MB L2) it
spills to L3 → predicted larger (unmeasured).

### 3.2 Matrix simplification / "relearn" (E4/E5/L1)
Three operators on the same matrix, scored on GSD gold (baseline F1 **97.995** = the
published Sudachi mode-A figure → validates the scorer):
```
E4 low-rank : 68.2% additive a[l]+b[r]; residual HIGH-rank (rank16→85.6%, rank48→97.3%)
              rank-16 reconstruction = 94.8% tokens / 63.5% sentences identical
E5 block    : K=256(L1-fit) F1 96.72 (−1.27), regressions:fixes ≈ 22:1; K=1024 97.54 (−0.45)
L1 thresh   : zero |M−(a+b)|<λ  → λ=1600: 93.6% zeroed, F1 98.00 (no loss)
              floor (100% zeroed = pure additive): F1 94.54
```
Reconciliation (§1.1): the discriminative signal is a **sparse heavy tail of large
residuals**; L1 keeps it (lossless), block/low-rank smear it (lossy). So the matrix
*does* carry ~94% removable structure — the reviewer's intuition — but this is
post-hoc, not proof a retrain reproduces quality.

**CRF basis (reviewer correct):** Kudo, Yamamoto, Matsumoto 2004 formulate JA
morphological analysis as a lattice CRF (Viterbi maximizes a linear sum of learned
feature weights) and describe L1-CRFs with sparse zero weights; MeCab sums learned
`alpha` into word/path costs; Sudachi stores costs as i16 and `connect_node` sums
`left.total_cost + matrix.cost(left,right) + node.cost`. The error would be to infer
that *post-hoc* compression of an already-trained matrix is therefore safe.

### 3.3 Phase separation (E6)
```
scalar (interleaved walk+insert) → pipelined (walks first, then nodes): ~1.18× do_tokenize
  pure separation (lanes=1, prefetch=0) : captures ~the entire win
  + MLP (lanes=4) , + explicit prefetch : ~0 each (within noise)
  lanes 8/16: plateau  → confirms study's "K>4 doesn't help"
```
The PR is named after prefetch, but the verified win is the **separation** (the
reviewer's suggestion). Consistent with E1 (data L2-resident → little raw latency for
a prefetch to hide). *Fragility:* the knob no-ops if the dict ships a DAAC index
(verified absent here).

### 3.4 Trie layout — issue #117 (LAYOUT/E7)
A real corpus-frequency double-array builder (`weighted_trie.rs`, wired into
`build_trie`) was used to build a full system dict:
```
parity      : exact (741,360 hits) + BYTE-IDENTICAL tokenization
              (the two .dic differ across 186 MB → a genuine relayout, not a fallback)
isolated    : lookup −8…−10%, 128B footprint 11.72→9.89 MiB (−15.6%)
end-to-end  : do_tokenize −0.5% (within noise), full pipeline ~0  → does NOT survive
E7 headroom : trie working set 7.84/11.83 MiB; scattered/packed chase ~1.85× (upper bound)
```
So #117 is feasible and output-safe, but the end-to-end payoff is ~0 — the runtime
pipelining (3.3) already hides the trie latency. Isolated lookup overstates the
end-to-end win ~16×.

### 3.5 External builder audit (issue #117 production base)
Can a corpus-frequency layout reuse an existing builder? Tested builders give a
**compatible base** but none implements corpus-aware ordering:
```
yada   72,608,768 B  build 21.5s  baseline           128B footprint 11.72 MiB
tried  72,608,768 B  build  2.7s  byte-equal to yada  (build-speed only)
darts  72,592,384 B  build  2.6s  parity 741,360      11.78 MiB (compatible, no layout win)
```
→ a patched/custom weighted builder is needed (= `weighted_trie.rs`, §3.4); the format
and reader are not the blocker.

### 3.6 Runtime trie replacement audit (separate from #117)
Full raw lexicon, lookup-only; all reproduce the full hit stream (741,360); heap sizes
are exact, absolute ns are thermal-dependent so only the ordering is claimed:
```
yada        72.6 MB serial   baseline
crawdad     55.7 MB heap     ~2× faster than yada  → promising, NOT drop-in (format/API)
crawdad MP  49.0 MB heap     promising, smaller
char daac  136.3 MB heap     competitive but large
byte daac  217.6 MB heap     slower
lexime     289.6 MB heap     slower
```
→ runtime replacement is possible but not a drop-in #117 fix; only `crawdad` merits an
adapter spike.

### 3.7 Repo training gap (REPO)
Independent grep + read: **no trainer/optimizer/feature-templates/labeled-corpus**
anywhere (the only "regularization/CRF" hits are PROBE comments). `dic/build/conn.rs`
**reads `matrix.def` and writes it verbatim** (`write_all(&self.matrix)`) — costs are
given, never learned. The build accepts **any** `matrix.def` (`sudachi build -m`), so
an externally-trained sparse/L1 matrix would integrate with **no code change — but
only if the left/right connection-ID inventory is held fixed** (else the lexicon's IDs
must be regenerated too). Hence a faithful retrain is impossible *in this repo*; the
build side is ready, the training side is absent (external/WAP-internal).

---

## 4. Open / not closeable here
1. **Faithful from-scratch L1-CRF retrain** — needs an external trainer + feature
   templates + a labeled corpus; this repo can only pack the resulting matrix.def.
2. **Sparse runtime matrix** — turning the L1 redundancy (94% of residual) into an
   actual cache/speed win (additive in L1 + sparse residual) is unbuilt; bounded above
   by E1's ~7–9%.
3. **Cross-arch** — all timing is M4 Max; Zen4 (`perf mem`/IBS can settle attribution)
   untested.
4. **Quality scope** — L1/layout "no loss" verified only on GSD mode-A segmentation.

---

## 5. Reproduction
```bash
# all probes are env-gated PROBE code (marked "not for upstream") in
# dic/connect.rs, dic/lexicon/trie.rs, dic/lexicon.rs, dic/build/{index,weighted_trie}.rs
cargo build -p sudachi --release \
  --example matrix_probe_ab --example matrix_lowrank --example gsd_quality \
  --example matrix_l1_gsd --example trie_layout --example corpus_trie_layout_probe \
  --example dict_ab --example tokenize_pipeline_bench
cargo build --release -p sudachi-cli
D=target/bench-lookup/full-v1/system_full.dic; C=resources/sudachi.json
I=target/issue-117-corpora/kyoto-leads.txt; G=bench/quality/ja_gsd-ud-test.conllu
e="SUDACHI_BENCH_CONFIG=$C SUDACHI_BENCH_DICT=$D SUDACHI_BENCH_INPUTS=$I"

env $e SUDACHI_AB_FOOTPRINTS=2097152,8192,0 SUDACHI_AB_TRIALS=41 ./target/release/examples/matrix_probe_ab   # E1/E2
env $e SUDACHI_AB_WORKINGSET=1 ./target/release/examples/matrix_probe_ab                                     # E3
env $e SUDACHI_LOWRANK_L=64 SUDACHI_LOWRANK_R=16 ./target/release/examples/matrix_lowrank                    # E4
for K in 256 1024; do env $e SUDACHI_GSD=$G SUDACHI_GSD_MODE=A SUDACHI_APPROX_K=$K ./target/release/examples/gsd_quality; done   # E5
for L in 0 1600 3200 6400 24000; do env $e SUDACHI_GSD=$G SUDACHI_L1_THRESH=$L ./target/release/examples/matrix_l1_gsd; done     # L1
for cfg in "1 0" "4 0" "4 1"; do set -- $cfg; env $e SUDACHI_BENCH_TRIALS=31 SUDACHI_BENCH_NOCOLLECT=1 SUDACHI_TRIE_LANES=$1 SUDACHI_TRIE_PREFETCH=$2 ./target/release/examples/tokenize_pipeline_bench | grep speedup; done   # E6
env $e ./target/release/examples/trie_layout                                                                 # E7
SUDACHI_BENCH_INPUTS=$I ./target/release/examples/corpus_trie_layout_probe                                    # LAYOUT isolated + 3.5 footprint
# Build the two layout dicts (same matrix/lexicon; only trie layout differs), then A/B + parity:
M=target/bench-lookup/raw/unzipped/matrix/matrix.def
S=target/bench-lookup/raw/unzipped/small/small_lex.csv; CO=target/bench-lookup/raw/unzipped/core/core_lex.csv; N=target/bench-lookup/raw/unzipped/notcore/notcore_lex.csv
./target/release/sudachi build -o target/bench-lookup/layout/baseline.dic -m $M $S $CO $N
SUDACHI_LAYOUT_CORPUS=$I ./target/release/sudachi build -o target/bench-lookup/layout/weighted.dic -m $M $S $CO $N
env SUDACHI_BENCH_CONFIG=$C SUDACHI_DICT_A=target/bench-lookup/layout/baseline.dic SUDACHI_DICT_B=target/bench-lookup/layout/weighted.dic SUDACHI_BENCH_INPUTS=$I SUDACHI_AB_TRIALS=41 SUDACHI_BENCH_NOCOLLECT=1 ./target/release/examples/dict_ab   # LAYOUT end-to-end + parity
# external builder / runtime replacement audits (separate workspaces):
SUDACHI_BENCH_INPUTS=$(pwd)/$I cargo run --release --offline --manifest-path bench/external-builder-probe/Cargo.toml      # 3.5
SUDACHI_BENCH_INPUTS=$(pwd)/$I cargo run --release --offline --manifest-path bench/runtime-replacement-probe/Cargo.toml   # 3.6
```

## 6. Artifacts
- Probes (env-gated, in-tree): `dic/connect.rs` (PROBE_MASK/NOMAT/RECORD/APPROX/REPLACE),
  `dic/lexicon/trie.rs` (lane/prefetch knob, trie recorder), `dic/lexicon.rs`,
  `dic/build/{index.rs hook, weighted_trie.rs}`.
- Examples: `matrix_probe_ab`, `matrix_analyze`, `matrix_approx_diff`, `matrix_lowrank`,
  `gsd_quality`, `matrix_l1_gsd`, `trie_layout`, `corpus_trie_layout_probe`, `dict_ab`,
  `tokenize_pipeline_bench`.
- Sub-projects: `bench/external-builder-probe`, `bench/runtime-replacement-probe`.
- Gold: `bench/quality/ja_gsd-ud-test.conllu` (UD_Japanese-GSD).
