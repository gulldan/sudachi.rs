# Issue #117 — Linux/x86 verification

Re-run the M4 causal probes on a machine with a **smaller L2** to settle the one
open cell: a prefetch-friendly trie layout is ~0 on M4 only because the ~7.76 MiB
trie working set is L2-resident there. On a core whose L2 < ~7.76 MiB the walk
goes to DRAM (58–93 ns/load vs 8 ns) and a frequency-packed layout should pay
("even Java free wins" — eiennohito).

## Run

```bash
# 1) latency calibration — NO dictionary needed (the key x86 proof):
./bench/issue117-linux/run.sh

# 2) the dict-dependent probes:
export SUDACHI_BENCH_DICT=/path/to/system_full.dic      # full, current format
export SUDACHI_BENCH_INPUTS=/path/to/kyoto-leads.txt
./bench/issue117-linux/run.sh

# 3) the layout A/B (build the two dicts first, see below):
export SUDACHI_DICT_A=/tmp/baseline.dic SUDACHI_DICT_B=/tmp/weighted.dic
./bench/issue117-linux/run.sh

# 4) precise perf profiling (needs hardware perf):
sudo sysctl kernel.perf_event_paranoid=1
./bench/issue117-linux/perf.sh
```

## Data setup

Either **copy from the Mac** (simplest):
```
target/bench-lookup/full-v1/system_full.dic   # ~315 MB, current format
target/issue-117-corpora/kyoto-leads.txt
```
…or **build SudachiDict on Linux** and the layout/char-wise dicts:
```bash
cargo build --release -p sudachi-cli
M=matrix.def; S=small_lex.csv; CO=core_lex.csv; N=notcore_lex.csv     # from SudachiDict
./target/release/sudachi build               -o /tmp/baseline.dic -m $M $S $CO $N
SUDACHI_LAYOUT_CORPUS=$SUDACHI_BENCH_INPUTS \
  ./target/release/sudachi build             -o /tmp/weighted.dic -m $M $S $CO $N   # freq layout
./target/release/sudachi build --experimental-daac-index -o /tmp/daac.dic -m $M $S $CO $N  # char-wise
```

## M4 baselines (compare your x86 numbers to these)

**Latency calibration** (`pointer_chase_latency`, M4 Max, 16 MB L2):
```
   8–128 KiB  0.9–1.1 ns   L1
   256 KiB    3.69 ns      L2          <- the trie walk measures 3.76 ns/load here
   8 MiB      8.05 ns      L2
   16 MiB     11.0 ns      L2
   64 MiB     57.8 ns      DRAM
   256 MiB    93.0 ns      DRAM
```
→ On x86, find the L2→DRAM jump. If it is **below 7.76 MiB**, the trie walk is
DRAM-bound on this box (it is L2-bound on M4) — the spill case.

**Trie working set** (`trie_workingset`): 7.42 loads/char, **distinct 64B lines
127,128 = 7.76 MiB**; Zipf head: 50% of loads in **137 lines (8.6 KiB, L1)**,
90% in 1 MiB, 99% in 5.8 MiB.

**Freq layout packs tighter** (`trie_freq_workingset`): 90%-of-loads set
1067 → **899 KiB** (−15.7%); 50% set 137 → 116 lines. On a ~1 MB-L2 core this
899-vs-1067 KiB difference crosses the L2-fit threshold.

**Prefetchability** (`trie_prefetchability`): fraction |Δline|≤1 — baseline
**9.6%**, freq 9.2, triplet-phase 8.0, parent-child 8.0, stride 9.3. NONE beats
baseline (byte-XOR scatter). Check whether this holds on x86.

**Char-wise** (`charwise_lookup_ab`): isolated yada 12.88 ms vs char-daac 6.96 ms
= **1.85×**; but e2e (real daac dict) **−1%** (+129 MB).

**Layout A/B** (`dict_ab`, baseline vs weighted, do_tokenize): **+2%** on M4.
Expect MORE on a small-L2 box if the working set spills.

**Profile** (`sample` on M4, `dotok_loop`, 15207 leaf samples):
connect_node (matrix) **29.9%**, trie batch_impl **10.6%**, WordInfo
materialization ~10%. These match the causal experiments (matrix block-class
+17.6%, trie ~9%, WordInfo cache +17%) — see `perf.sh` for the cross-validation.

## Causal ground truth (what perf must match — avoids the OoO skid)

- Matrix is the #1 cache lever: replacing the dense 71 MB matrix with an
  L1-resident class block = **+17.6% do_tokenize** (causal, not perf-annotate).
- Trie lookup = ~9% of do_tokenize (causal extra-pass probe).
- WordInfo materialization sharing (Arc) = **+17%, byte-identical**.

If `perf annotate` charges a DRAM stall to WordInfo-parse next to a matrix load,
that is the skid eiennohito flagged — trust the causal numbers above.
