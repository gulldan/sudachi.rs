#!/usr/bin/env bash
#
# Issue #117 — Linux/x86 verification runbook.
#
# Re-runs the same causal probes we ran on Apple M4, so the cache numbers can be
# compared on a machine with a smaller L2 — the open question: does a
# prefetch-friendly trie layout pay once the ~7.76 MiB trie working set spills
# out of L2 (it doesn't on M4's 16 MB L2)?
#
# Usage:
#   ./bench/issue117-linux/run.sh
#       runs the dependent-load latency calibration (no dict needed) and, if
#       SUDACHI_BENCH_DICT + SUDACHI_BENCH_INPUTS are set, the dict-dependent probes.
#
# See bench/issue117-linux/README.md for data setup and the M4 baselines.

set -euo pipefail
cd "$(dirname "$0")/../.."   # repo root

echo "===================================================================="
echo " issue #117 — Linux/x86 run"
echo "===================================================================="
echo "--- CPU / cache hierarchy (the L2 size is what matters) ---"
(lscpu | grep -iE "model name|^CPU\(s\)|L1d|L2|L3|cache") 2>/dev/null || cat /proc/cpuinfo | grep -m1 "model name" || true
echo

echo "--- building probes (release) ---"
cargo build -p sudachi --release \
  --example pointer_chase_latency \
  --example trie_workingset \
  --example trie_freq_workingset \
  --example trie_prefetchability \
  --example charwise_lookup_ab \
  --example dict_ab \
  --example dotok_loop
EX=./target/release/examples
echo

echo "===================================================================="
echo " [1] dependent-load latency calibration  (NO dictionary needed)"
echo "     KEY x86 proof: find the L2->DRAM jump. Compare the ~8 MiB row to"
echo "     M4 (~8 ns). If this box's L2 < 7.76 MiB, the trie walk is DRAM-bound"
echo "     here -> a frequency-packed layout should give a real win (vs +2% on M4)."
echo "===================================================================="
"$EX/pointer_chase_latency"
echo

DICT="${SUDACHI_BENCH_DICT:-}"
INPUTS="${SUDACHI_BENCH_INPUTS:-}"
if [[ -z "$DICT" || -z "$INPUTS" ]]; then
  cat <<'EOF'
====================================================================
 dict-dependent probes SKIPPED.
 Set a full current-format dict + corpus and re-run, e.g.:
   export SUDACHI_BENCH_DICT=/path/to/system_full.dic
   export SUDACHI_BENCH_INPUTS=/path/to/kyoto-leads.txt
 (copy both from the Mac, or build SudachiDict — see README.md)
====================================================================
EOF
  exit 0
fi
export SUDACHI_BENCH_DICT SUDACHI_BENCH_INPUTS
export SUDACHI_BENCH_CONFIG="${SUDACHI_BENCH_CONFIG:-resources/sudachi.json}"

echo "===== [2] trie working set + Zipf head — does 7.76 MiB fit THIS L2? ====="
"$EX/trie_workingset"
echo
echo "===== [3] does the freq layout pack the Zipf head tighter here? ====="
"$EX/trie_freq_workingset"
echo
echo "===== [4] prefetchability of the layout family (is XOR-scatter the same on x86?) ====="
SUDACHI_LAYOUT_OFFSET_K=16 "$EX/trie_prefetchability"
echo
echo "===== [5] isolated char-wise vs byte-wise (yada) lookup ====="
"$EX/charwise_lookup_ab"
echo

A="${SUDACHI_DICT_A:-}"; B="${SUDACHI_DICT_B:-}"
if [[ -n "$A" && -n "$B" ]]; then
  echo "===== [6] layout A/B do_tokenize  (A=$A  B=$B) ====="
  SUDACHI_AB_TRIALS="${SUDACHI_AB_TRIALS:-41}" SUDACHI_BENCH_NOCOLLECT=1 \
    SUDACHI_DICT_A="$A" SUDACHI_DICT_B="$B" "$EX/dict_ab"
else
  echo "# [6] layout A/B skipped — set SUDACHI_DICT_A=baseline.dic SUDACHI_DICT_B=weighted.dic"
  echo "#     (build them per README.md, then re-run)"
fi

echo
echo "===================================================================="
echo " Compare against the M4 baselines in bench/issue117-linux/README.md"
echo " (esp. the latency curve and the trie working-set fit)."
echo "===================================================================="
