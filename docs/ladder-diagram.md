# Sudachi speedup ladder (figure)

*UD_Japanese-GSD, Apple M4 Max. Full output = seg + POS + normalized. All [meas] on real binaries
(lattice: `tokenize_pipeline_bench`; paradigm: the fused `/tmp/vapo-sud`, §5.12). Bar length ∝ speedup ×.
The binding question is not "how fast" but "does it keep Java-Sudachi output compatibility".*

```
 approach                            ×       ns/char  compat  speedup (bar ∝ ×) ▸
 ──────────────────────────────────────────────────────────────────────────────
 JAVA-COMPATIBLE (shippable as sudachi.rs)
 Sudachi lattice (pf+varint+#39/40)  1.0×    ~414     ✅      ████   (full -a output)
   └ + WordInfo cache (#52)          1.44×   ~270     ✅      ██████  byte-identical, full -a
   └ ceiling now ~1.4–1.6× single-thread: cache is +27% default / +44% full -a [meas, real dict],
     atop pf+varint+#39/40 (~+9%). Lattice kernel at its floor — but WordInfo materialization wasn't.
 + multithreading (orthogonal)       ~8.9×   —        ✅      ████████████████████████████████████
 ──────────────────────────────────────────────────────────────────────────────
 NOT JAVA-COMPATIBLE (a different tool, not sudachi.rs)
 Vaporetto+dict, cold cache          1.6×    ~182     ❌      ██████
 Vaporetto+dict, warm cache          3.7×    ~78      ❌      ███████████████        ← real full-parity
 bare seg+POS (no normalized)        4.1×    ~71      ❌      ████████████████
 segmentation only (no features)    10.7×    ~27      ❌      ███████████████████████████████████████████
 ──────────────────────────────────────────────────────────────────────────────
 The ❌ rows change the output (no A/B/C, SEG −0.92, POS −1.74) → outside sudachi.rs's
 compatibility contract. The shippable wins are the ✅ rows. See paradigm-proposal.md.
```

*Reading: the paradigm's multipliers are real and measured (~3.7× full-parity, not the earlier
projected 6.5×), but every paradigm row breaks Java-Sudachi compatibility. Within the compatibility
contract that defines sudachi.rs, the lattice kernel is at its floor, but WordInfo materialization was
not — sharing it (#52) is +27% on default output and +44% on full `-a` output, byte-identical [meas,
real SudachiDict, GSD]. The byte-identical single-thread ceiling is therefore ~1.4–1.6× (cache atop
#39/#40+pf+varint), higher than the earlier +20–21% prototype estimate; the win scales with how much
per-morpheme output is materialized (decoded strings are shared too). The largest remaining lever is
still multithreading (~8.9×, orthogonal). 3× "as sudachi.rs" remains off the table for full output;
3× "as a different tokenizer" is real via the paradigm rows.*
