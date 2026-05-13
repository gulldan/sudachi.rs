# Performance notes for the tokenizer optimization slice

These numbers are local benchmark notes for the current performance PR. They
are intended to make the PR reviewable: the kept changes have wall-time or
counter evidence, and the rejected experiments are listed with the metric that
made them lose.

Counter-mode runs are for attribution only. Release timing comparisons use
non-profile release binaries.

## Benchmark setup

| Item | Value |
|---|---|
| Dictionary | SudachiDict 20260428 full dictionary, rebuilt as a compatible dictionary |
| Full-corpus input | 99,998 filtered Japanese Wikipedia title lines |
| Full-corpus chars / bytes | 1,301,348 chars / 2,465,051 bytes |
| Mode | C |
| Timing binary | `profile_pipeline` or `accessor_probe`, release, no `profile` feature |
| Counter binary | `profile_pipeline`, release, `profile` feature |
| Parity probe | `accessor_probe` TSV, Rust optimized output against no-cache Rust baseline |

## Final release sanity rerun on current tree

This table was rerun after the final test/documentation cleanup. It compares
the original local baseline numbers used for the optimization work with the
current tree on the same corpus or stress preset.

| Group | Scenario | Corpus | Baseline total ms | Current total ms | Delta | Current tokenize ms | Current accessor/split ms | Current morph/s | Current RSS MiB |
|---|---|---|---:|---:|---:|---:|---:|---:|---:|
| core | tokenize-only | 99,998 titles | 398.503 | 367.628 | +7.7% | 346.105 | 0.000 | 1,195,779 | 302.2 |
| core | split-tokenize | 99,998 titles | 469.143 | 423.828 | +9.7% | 341.272 | split 58.122 | 1,037,260 | 302.1 |
| accessors | all | 99,998 titles | 441.118 | 400.225 | +9.3% | 347.770 | accessor 29.258 | 1,098,387 | 304.3 |
| oov | mixed | synthetic stress | 25.173 | 22.806 | +9.4% | 21.474 | 0.000 | 1,446,988 | 179.2 |
| oov | long-katakana | synthetic stress | 40.582 | 28.525 | +29.7% | 27.912 | 0.000 | 315,513 | 178.7 |
| candidate | candidate-heavy | synthetic stress | 10.358 | 10.445 | -0.8% | 9.498 | 0.000 | 1,053,135 | 178.3 |

Earlier optimization-run samples on the same baseline showed the final slice
at `352.879 ms` tokenize-only, `401.826 ms` split-tokenize, `387.274 ms`
accessors/all, `28.113 ms` long-katakana, and `10.028 ms` candidate-heavy.
The table above is intentionally the later sanity rerun, so the PR does not
depend on the best observed local run.

## Parity gates

| Probe | Rows | SHA-256 | Result |
|---|---:|---|---|
| Current Rust vs no-cache Rust baseline, full 99,998-title accessor TSV | 439,602 | `2251cc824683cf1bb0145f28430d6a96aa344e4e03a0c393036f6b52adeabedc` | byte-for-byte identical |
| Java / Rust / Python accessor probe, 2,000 Wikipedia title lines | 10,573 | `2f058e52dc4d066fea45606c9a9f6a36881b59c53b6f45770488e7fba2f577bc` | byte-for-byte identical |

The Java probe used `WorksApplications/Sudachi` `develop-v0.8`, because the
Maven `0.7.5` release did not expose
`dictionaryFormMorpheme()` / `normalizedFormMorpheme()`.

## Kept changes

| Area | Change | Evidence | Decision |
|---|---|---|---|
| Sentence detector | Replaced regex boundary detection with a manual scanner | split-only + checker: `~129-132 ms` to `~8-9 ms` (`~14-16x`); split+tokenize: `~566-579 ms` to `~424-438 ms` (`~23-25%`); tokenize-only unchanged at `~416-432 ms` vs `~416-428 ms` | keep |
| WordInfo | Avoid secondary dictionary-form decode when `DIC_FORM_WORD_ID` is not requested | surface-only counters after fix: `20,000` WordInfo requests, `20,000` decodes, `20,000` string allocations, `0` Vec allocations, `0` split decodes | keep |
| Lattice best predecessor | Cache best predecessor per boundary by `left_id` | full-corpus checks/node: `9.32` to `5.54`; estimated saved transition checks: `~40.5%`; long-katakana checks/node: `~12.91` to `~5.93` | keep |
| Lattice scan threshold | Direct scan for small predecessor sets | thresholds `6`, `8`, and `16` were tested; threshold `4` kept the OOV profile safer and avoided extra checks on wider cases | keep threshold `4` |
| Connection matrix | Add row-slice accessor for connection matrix hot loop | Included in current release sanity rerun: tokenize-only `398.503 ms` to `367.628 ms`; long-katakana `40.582 ms` to `28.525 ms` | keep |
| Lattice reset | Active-range reset instead of historical-capacity reset | visited inner vectors on full 100k: `~40.5M` to `~5.6M`, about `7.2x` fewer; checks/node unchanged at `5.54` | keep |
| BestPrev layout | Remove redundant begin from `BestPrev` | `BestPrev` size: `12 B` to `8 B`; fewer bytes in cache slots and return/copy path | keep |
| OOV plugin context | `needs_oov_buffer_context()` for built-ins that do not need the buffered context | Keeps external/custom OOV behavior while allowing built-ins to avoid unnecessary context handling | keep |
| Profiling | Add release/profile harnesses and counters | Matrix reports timings/RSS; profile mode reports WordInfo, lattice, OOV, range, storage, and dominance counters | keep |

## Rejected experiments

| Area | Experiment | Measured result | Decision |
|---|---|---|---|
| Lattice | `right_id` compaction of previous boundary | Reduced check counters, but wall-time regressed from compaction/stamp/bookkeeping overhead | drop |
| Lattice | Dense stamped cache by `left_id` | Extra cache machinery lost wall-time on low-benefit boundaries | drop |
| Lattice | transient single `best_prev` cache per begin | Wall-time regression / no stable positive result | drop |
| Lattice | `BoundaryHot` parallel scan array | Additional write/memory traffic outweighed removed indirection | drop |
| Lattice | `FullNode { node, prev }` / removing separate `indices` | Tests passed, but wall-time regressed; separate `indices` preserved better access pattern | drop |
| Lattice | connected-node reachability scan | Nodes remained `9,301,612`, OOV candidates remained `5,224,487`, checks/node remained `5.54`; only added overhead | drop |
| Lattice reset | touched-boundary reset | Fewer theoretical reset operations, but per-insert bookkeeping lost wall-time | drop |
| OOV | batch insertion for OOV ranges | Real/full and long-katakana max OOV range was `18`; threshold `24` activated on `0.0%` of ranges; lower thresholds did not justify the extra path | drop |
| OOV | streaming/sink insertion | More elegant path, but wall-time regressed | drop |
| OOV | duplicate removal | duplicates/equal-cost ties: `2,690 / 5,224,487`, about `0.05%`; too small for HashSet/sort overhead | drop |
| OOV | strict dominance suppression | strict dominated candidates: `0` on full 100k, `0` on oov/mixed, `0` on long-katakana, `0` on candidate-heavy | drop |
| OOV | direct category tables / array lookup | Wall-time regression versus existing category path | drop |
| OOV | category-run cache prototype | Existing `mod_cat_continuity` / continuous length path already covers the useful case; prototype did not improve wall-time | drop |
| Misc hot loop | `inline(always)` / `add_word_len` cleanup | No stable wall-time win; removed to avoid noise | drop |

## Diagnostic counters that drove the priorities

| Slice | Metric | Value | Interpretation |
|---|---|---:|---|
| Full-dict core baseline | OOV candidates | 5,224,487 | OOV pressure is real, not only synthetic |
| Full-dict core baseline | Lattice nodes | 9,301,612 | Main CPU work is lattice transition volume |
| Full-dict core baseline | checks/node before lattice cache | 9.32 | Hot predecessor scan was too expensive |
| Full-dict current | checks/node after lattice cache | 5.54 | Cache removes repeated predecessor scans |
| Full-dict current | saved transition checks | ~40.5% | The cache is doing real work |
| Full-dict current | max nodes/boundary | 171 | Wide boundaries explain OOV-heavy wins |
| Full-dict reset | reset visits before active range | ~40.5M | Historical-capacity reset was too broad |
| Full-dict reset | reset visits after active range | ~5.6M | Active-range reset removes dead reset work |
| OOV dominance | strict dominated OOV candidates | 0 | Exact candidate deletion was not available |
| OOV dedup | equal-cost/duplicate-like ties | 2,690 | Only ~0.05%; not worth production dedup |

## WordInfo and accessor diagnostics

| Scenario | Total ms | Tokenize ms | Accessor ms | Counter signal |
|---|---:|---:|---:|---|
| wordinfo/surface | 338.543 | not used for target selection | 2.046 | surface accessor is cheap |
| wordinfo/splits | not recorded in final table | not recorded in final table | 18.570 | split accessors are a separate API-heavy target |
| wordinfo/all | 421.612 | not used for target selection | 28.312 | accessor-heavy path is still worth future work |
| core/all | 386.365 | not recorded in final table | 0.000 | baseline full WordInfo subset |
| core/rewrite-min | 383.749 | not recorded in final table | 0.000 | only `0.68%` below all, so WordInfo was not the next general CPU target |
| core/none | 312.856 | not recorded in final table | 0.000 | theoretical lower bound; not API-safe |

`core/all` RSS was about `302.2 MiB`; `core/none` was about `237.2 MiB`.
The `~65 MiB` gap is useful for future memory/API-materialization work, but it
was not the best immediate CPU target compared with lattice.

## Next project, not this PR

The next exact-safe optimization layer should start with counters only:

| Target | First measurement |
|---|---|
| hot/cold candidate representation | cold fields written but never read |
| output/backtrace materialization | nodes/output morpheme and cold reads by phase |
| `ends_full` / `indices` storage | writes, reads, reset work, and output-only use |

Do not mix this with the current PR. The current PR is the hot-path/reset slice;
hot/cold candidate representation is a larger separate design.
