# Cross-runtime accessor benchmark

This harness compares Rust, Python, and Java Sudachi implementations on the
same dictionary and corpus.

It writes a TSV row per morpheme with the common accessor/split fields:

```text
line_idx, morph_idx, begin, end, surface, word_id, dictionary_id, pos_id,
dictionary_form, normalized_form, split_a, split_b
```

`tools/cross_runtime_bench.py` reports:

- byte-for-byte SHA-256 equality against the first successful implementation
- morpheme rows
- elapsed time
- rows/second
- sampled peak RSS

Example:

```bash
python3 tools/cross_runtime_bench.py \
  --input /path/to/wiki_titles.txt \
  --config /path/to/sudachi.json \
  --resource-dir /path/to/resources \
  --dict /path/to/system.dic \
  --runs 3 \
  --java-classpath '/path/to/Sudachi/build/libs/*:/path/to/Sudachi/build/install/executable/lib/*'
```

Java is optional. Use a Sudachi build from `WorksApplications/Sudachi`
`develop-v0.8` if the probe must be compiled against
`dictionaryFormMorpheme()` / `normalizedFormMorpheme()` era APIs. The strict
cross-runtime TSV intentionally uses only fields that are currently exposed by
all three runtimes in this repository.

## Rust phase/profile matrix

For Rust-only target selection, use `profile_pipeline` directly or the matrix
runner:

```bash
python3 tools/rust_profile_matrix.py \
  --dict /path/to/system.dic \
  --iterations 5 \
  --repeat 1000
```

Useful filters:

```bash
python3 tools/rust_profile_matrix.py --scenario core
python3 tools/rust_profile_matrix.py --scenario accessors
python3 tools/rust_profile_matrix.py --scenario wordinfo
python3 tools/rust_profile_matrix.py --scenario oov
python3 tools/rust_profile_matrix.py --scenario rewrite
python3 tools/rust_profile_matrix.py --scenario candidate
```

The matrix reports tokenizer phase timings and sampled peak RSS for tokenizer
core paths, accessor-specific paths, WordInfo subset/decode paths, OOV-heavy
corpora, rewrite-heavy corpora, and candidate-heavy corpora.

For release validation against a full dictionary and external corpus, keep
timing and attribution runs separate. Use non-profile release binaries for
wall-time numbers:

```bash
cargo build -p sudachi --release --example accessor_probe

target/release/examples/accessor_probe \
  --dict /tmp/sudachi-current-full-20260428/system_full.dic \
  --input /tmp/titles_100k_java_ok.txt \
  --output /tmp/sudachi_current.tsv \
  --mode C

shasum -a 256 /tmp/sudachi_current.tsv /tmp/sudachi_baseline.tsv
wc -l /tmp/sudachi_current.tsv /tmp/sudachi_baseline.tsv
cmp -s /tmp/sudachi_current.tsv /tmp/sudachi_baseline.tsv
```

Then run the Rust matrix without profile counters for timing and peak RSS:

```bash
python3 tools/rust_profile_matrix.py \
  --dict /tmp/sudachi-current-full-20260428/system_full.dic \
  --input /tmp/titles_100k_java_ok.txt \
  --iterations 5 \
  --repeat 1 \
  --skip-errors
```

Use counter mode for deeper target selection:

```bash
python3 tools/rust_profile_matrix.py \
  --profile-counters \
  --scenario wordinfo \
  --scenario oov \
  --iterations 5
```

Counter mode builds `sudachi` with the `profile` feature and reports WordInfo
decode counts, approximate owned `String`/`Vec` materializations, OOV
candidates, lattice node counts, connection checks, and vector growths.
Counter-mode numbers are for attribution, not for release timing comparisons.
For the current optimization slice, see `PERFORMANCE_NOTES.md` for the
wall-time tables, parity hashes, and rejected-experiment matrix.

Focused Rust run:

```bash
cargo run -p sudachi --release --example profile_pipeline -- \
  --corpus oov-mixed \
  --accessors none \
  --subset all \
  --iterations 10
```
