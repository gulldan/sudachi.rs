#!/usr/bin/env bash
#
# Issue #117 — precise perf profiling on Linux (the part that needs hardware perf).
#
# eiennohito's point: naive `perf annotate` misattributes DRAM stalls under OoO
# execution (the stall skids to a neighbouring instruction — often WordInfo
# parsing next to a connection-matrix load). The discipline that "guides perf to
# correct conclusions":
#   (A) function-level call-stack profile — robust to skid (attributes by frame);
#   (B) PRECISE event sampling (PEBS on Intel / IBS on AMD) — tags the exact PC;
#   (C) memory-access sampling — exact load addresses + which cache level;
#   then cross-validate against the causal experiments (the ground truth).
#
# Usage: SUDACHI_BENCH_DICT=... SUDACHI_BENCH_INPUTS=... ./bench/issue117-linux/perf.sh

set -uo pipefail
cd "$(dirname "$0")/../.."
: "${SUDACHI_BENCH_DICT:?set SUDACHI_BENCH_DICT to a full current-format .dic}"
: "${SUDACHI_BENCH_INPUTS:?set SUDACHI_BENCH_INPUTS to a corpus file}"
export SUDACHI_BENCH_DICT SUDACHI_BENCH_INPUTS
export SUDACHI_BENCH_CONFIG="${SUDACHI_BENCH_CONFIG:-resources/sudachi.json}"
export SUDACHI_LOOP_SECS="${SUDACHI_LOOP_SECS:-30}"

cargo build -p sudachi --release --example dotok_loop >/dev/null
BIN=./target/release/examples/dotok_loop
KEYS='connect_node|batch_impl|do_tokenize|Lattice.*insert|get_word_info|resolve|from_utf16|parse_u32|parse_i32|provide_oov|connect'

echo "##### [A] function-level profile (call-stack, robust to skid) #####"
perf record -g -F 999 -o /tmp/p117.data -- "$BIN" >/dev/null 2>&1 \
  && perf report -i /tmp/p117.data --stdio -g none 2>/dev/null \
       | grep -E "$KEYS" | head -20 \
  || echo "perf record failed (need 'perf' + kernel.perf_event_paranoid<=1)"
echo

echo "##### [B] precise cycles (PEBS/IBS) — tags the exact instruction #####"
if perf record -e cycles:pp -g -F 999 -o /tmp/pp117.data -- "$BIN" >/dev/null 2>&1; then
  perf report -i /tmp/pp117.data --stdio -g none 2>/dev/null | grep -E "$KEYS" | head -20
else
  echo "cycles:pp unavailable (no PEBS/IBS here) — falling back to plain cycles is NOT precise."
fi
echo

echo "##### [C] memory-access sampling — load addresses + cache level #####"
if perf mem record -o /tmp/mem117.data -- "$BIN" >/dev/null 2>&1; then
  echo "# by symbol + memory level (look at connect_node's loads):"
  perf mem report -i /tmp/mem117.data --stdio --sort=mem,sym 2>/dev/null | head -30
else
  echo "perf mem unavailable on this CPU/kernel."
fi
echo

echo "##### [D] stall / miss counters over the whole run #####"
perf stat -e cycles,instructions,cache-references,cache-misses,LLC-load-misses "$BIN" 2>&1 | tail -12 || true
echo

cat <<'EOF'
##### cross-validation — "guide perf to correct conclusions" #####
Expected if perf is read correctly (must match our causal results from M4):
  [A] connect_node is #1 (~25-30%), trie batch_impl ~9-11%.
      (M4 `sample`: connect_node 29.9%, batch_impl 10.6%, WordInfo ~10%.)
  [C] connect_node's matrix loads: on a >=16MB-L2 box they should be L2 hits
      (our causal: matrix working set = 8.8 MiB, L2-resident). On a SMALL-L2 box
      they may show DRAM -> that is the spill case where a prefetch-friendly trie
      layout would finally pay (the open #117 cell).
  SKID CHECK: if `perf annotate` charges a DRAM stall to a WordInfo-parse
      instruction sitting next to a matrix load, that is the OoO misattribution
      eiennohito flagged. Do NOT conclude "WordInfo is the bottleneck" from it —
      the causal block-class experiment (dense matrix -> L1-resident class block
      = +17.6% do_tokenize) is the ground truth: the matrix is the lever.
EOF
