#!/usr/bin/env python3
#
# Copyright (c) 2021-2024 Works Applications Co., Ltd.
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

from __future__ import annotations

import argparse
import shutil
import subprocess
import time
from dataclasses import dataclass, field
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


@dataclass(frozen=True)
class Scenario:
    group: str
    name: str
    corpus: str
    split: bool
    accessors: str
    subset: str


@dataclass
class MatrixResult:
    scenario: Scenario
    status: str
    corpus_label: str = ""
    peak_rss_kib: int = 0
    lines: int = 0
    input_bytes: int = 0
    input_chars: int = 0
    sentences: int = 0
    morphemes: int = 0
    errors: int = 0
    total_ms: float = 0.0
    split_ms: float = 0.0
    reset_push_ms: float = 0.0
    tokenize_ms: float = 0.0
    collect_ms: float = 0.0
    accessors_ms: float = 0.0
    message: str = ""
    counters: dict[str, int] = field(default_factory=dict)

    @property
    def peak_rss_mib(self) -> float:
        return self.peak_rss_kib / 1024.0

    @property
    def morphemes_per_sec(self) -> float:
        if self.total_ms <= 0:
            return 0.0
        return self.morphemes / (self.total_ms / 1000.0)

    @property
    def chars_per_sec(self) -> float:
        if self.total_ms <= 0:
            return 0.0
        return self.input_chars / (self.total_ms / 1000.0)

    @property
    def bytes_per_sec(self) -> float:
        if self.total_ms <= 0:
            return 0.0
        return self.input_bytes / (self.total_ms / 1000.0)

    @property
    def morphemes_per_char(self) -> float:
        if self.input_chars <= 0:
            return 0.0
        return self.morphemes / self.input_chars


SCENARIOS: tuple[Scenario, ...] = (
    Scenario("core", "tokenize-only", "base", False, "none", "all"),
    Scenario("core", "tokenize-rewrite-min", "base", False, "none", "rewrite-min"),
    Scenario("core", "tokenize-empty-subset", "base", False, "none", "none"),
    Scenario("core", "split-tokenize", "base", True, "none", "all"),
    Scenario("accessors", "surface", "base", False, "surface", "all"),
    Scenario("accessors", "pos", "base", False, "pos", "all"),
    Scenario("accessors", "normalized", "base", False, "normalized", "all"),
    Scenario("accessors", "dictionary", "base", False, "dictionary", "all"),
    Scenario("accessors", "splits", "base", False, "splits", "all"),
    Scenario("accessors", "all", "base", False, "all", "all"),
    Scenario("wordinfo", "surface-subset", "candidate-heavy", False, "surface", "surface"),
    Scenario("wordinfo", "pos-subset", "candidate-heavy", False, "pos", "pos"),
    Scenario(
        "wordinfo",
        "normalized-subset",
        "candidate-heavy",
        False,
        "normalized",
        "normalized",
    ),
    Scenario(
        "wordinfo",
        "dictionary-subset",
        "candidate-heavy",
        False,
        "dictionary",
        "dictionary",
    ),
    Scenario("wordinfo", "splits-subset", "candidate-heavy", False, "splits", "splits"),
    Scenario("wordinfo", "all-subset", "candidate-heavy", False, "all", "all"),
    Scenario("oov", "ascii", "oov-ascii", False, "none", "all"),
    Scenario("oov", "mixed", "oov-mixed", False, "none", "all"),
    Scenario("oov", "symbols", "oov-symbol", False, "none", "all"),
    Scenario("oov", "long-katakana", "long-katakana", False, "none", "all"),
    Scenario("rewrite", "numeric", "rewrite-numeric", False, "none", "all"),
    Scenario("rewrite", "normalized", "rewrite-normalized", False, "none", "all"),
    Scenario("candidate", "candidate-heavy", "candidate-heavy", False, "none", "all"),
)


def main() -> None:
    args = parse_args()
    selected = select_scenarios(args.scenario)
    binary = args.binary or ROOT / "target" / "release" / "examples" / "profile_pipeline"
    if not args.no_build and args.binary is None:
        build_cmd = [
            "cargo",
            "build",
            "-p",
            "sudachi",
            "--release",
            "--example",
            "profile_pipeline",
        ]
        if args.profile_counters:
            build_cmd.extend(["--features", "profile"])
        build = subprocess.run(build_cmd, cwd=ROOT)
        if build.returncode != 0:
            raise SystemExit(build.returncode)

    if not binary.exists():
        raise SystemExit(f"missing profile binary: {binary}")

    results = [run_scenario(args, binary, scenario) for scenario in selected]
    print_table(results)
    if args.profile_counters:
        print_counters_table(results)
        print_lattice_shape_table(results)
        print_lattice_storage_table(results)
        print_oov_materialization_table(results)
        print_oov_semantic_table(results)
        print_oov_dominance_table(results)
        print_oov_range_table(results)
    if any(r.status != "OK" for r in results):
        raise SystemExit(1)


def select_scenarios(filters: list[str] | None) -> list[Scenario]:
    if not filters:
        return list(SCENARIOS)

    selected = []
    wanted = set(filters)
    for scenario in SCENARIOS:
        keys = {
            scenario.group,
            scenario.name,
            f"{scenario.group}/{scenario.name}",
        }
        if keys & wanted:
            selected.append(scenario)

    missing = wanted - {
        key
        for scenario in selected
        for key in (scenario.group, scenario.name, f"{scenario.group}/{scenario.name}")
    }
    if missing:
        raise SystemExit(f"unknown --scenario filter(s): {', '.join(sorted(missing))}")
    return selected


def run_scenario(args: argparse.Namespace, binary: Path, scenario: Scenario) -> MatrixResult:
    cmd = [
        str(binary),
        "--config",
        str(args.config),
        "--resource-dir",
        str(args.resource_dir),
        "--mode",
        args.mode,
        "--iterations",
        str(args.iterations),
        "--repeat",
        str(args.repeat),
        "--split-sentences",
        "yes" if scenario.split else "no",
        "--accessors",
        scenario.accessors,
        "--subset",
        scenario.subset,
    ]
    if args.input:
        cmd.extend(["--input", str(args.input)])
    else:
        cmd.extend(["--corpus", scenario.corpus])
    if args.dict:
        cmd.extend(["--dict", str(args.dict)])
    if args.skip_errors:
        cmd.append("--skip-errors")

    with subprocess.Popen(
        cmd,
        cwd=ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    ) as proc:
        peak = sample_peak_rss(proc, args.sample_interval)
        stdout, stderr = proc.communicate()

    if proc.returncode != 0:
        return MatrixResult(
            scenario,
            "FAIL",
            peak_rss_kib=peak,
            message=(stderr.strip() or stdout.strip()).splitlines()[-1][:120],
        )

    result = parse_profile_output(stdout, scenario)
    result.peak_rss_kib = peak
    return result


def parse_profile_output(output: str, scenario: Scenario) -> MatrixResult:
    result = MatrixResult(scenario, "OK")
    for line in output.splitlines():
        if not line.strip() or "\t" not in line:
            continue
        key, value, *_rest = line.split("\t")
        match key:
            case "lines":
                result.lines = int(value)
            case "corpus":
                result.corpus_label = value
            case "input_bytes":
                result.input_bytes = int(value)
            case "input_chars":
                result.input_chars = int(value)
            case "sentences_per_iter":
                result.sentences = int(value)
            case "morphemes_per_iter":
                result.morphemes = int(value)
            case "errors_per_iter":
                result.errors = int(value)
            case "pipeline_total_avg":
                result.total_ms = parse_ms(value)
            case "split_next_avg":
                result.split_ms = parse_ms(value)
            case "reset_push_avg":
                result.reset_push_ms = parse_ms(value)
            case "do_tokenize_avg":
                result.tokenize_ms = parse_ms(value)
            case "collect_results_avg":
                result.collect_ms = parse_ms(value)
            case "accessors_splits_avg":
                result.accessors_ms = parse_ms(value)
            case _:
                if key.startswith(("word_info_", "lattice_", "oov_", "type_size_")) or key in {
                    "pos_decodes",
                    "normalized_form_decodes",
                    "dictionary_form_decodes",
                    "reading_form_decodes",
                    "split_a_decodes",
                    "split_b_decodes",
                    "split_c_decodes",
                    "owned_string_allocations",
                    "vec_allocations",
                }:
                    result.counters[key] = int(value)
    return result


def parse_ms(value: str) -> float:
    return float(value.removesuffix(" ms").strip())


def sample_peak_rss(proc: subprocess.Popen, interval: float) -> int:
    peak = 0
    while proc.poll() is None:
        peak = max(peak, rss_kib(proc.pid))
        time.sleep(interval)
    return max(peak, rss_kib(proc.pid))


def rss_kib(pid: int) -> int:
    try:
        out = subprocess.check_output(["ps", "-o", "rss=", "-p", str(pid)], text=True)
    except (subprocess.CalledProcessError, FileNotFoundError):
        return 0
    out = out.strip()
    return int(out) if out else 0


def print_table(results: list[MatrixResult]) -> None:
    print(
        "| Group | Scenario | Corpus | Lines | Chars | Bytes | Split | Accessors | Subset | Total ms | Tokenize ms | Split ms | Accessor ms | Morph/s | Chars/s | Bytes/s | Morph/char | Peak RSS MiB | Errors |"
    )
    print(
        "|---|---|---|---:|---:|---:|---:|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|"
    )
    for r in results:
        s = r.scenario
        note = f" {r.message}" if r.message else ""
        corpus = r.corpus_label or s.corpus
        print(
            f"| {s.group} | {s.name}{note} | {corpus} | {r.lines} | "
            f"{r.input_chars} | {r.input_bytes} | {str(s.split).lower()} | "
            f"{s.accessors} | {s.subset} | {r.total_ms:.3f} | {r.tokenize_ms:.3f} | "
            f"{r.split_ms:.3f} | {r.accessors_ms:.3f} | {r.morphemes_per_sec:.0f} | "
            f"{r.chars_per_sec:.0f} | {r.bytes_per_sec:.0f} | {r.morphemes_per_char:.3f} | "
            f"{r.peak_rss_mib:.1f} | {r.errors} |"
        )


def print_counters_table(results: list[MatrixResult]) -> None:
    print()
    print(
        "| Group | Scenario | WI req | WI dec | String alloc | Vec alloc | Split dec | OOV cand | OOV dup | Nodes | Checks/node | Max nodes/bound | Vec growths | Rejected cand |"
    )
    print("|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|")
    for r in results:
        c = r.counters
        nodes = c.get("lattice_inserted_nodes", 0)
        checks = c.get("lattice_left_connection_checks", 0)
        checks_per_node = checks / nodes if nodes else 0.0
        split_decodes = (
            c.get("split_a_decodes", 0)
            + c.get("split_b_decodes", 0)
            + c.get("split_c_decodes", 0)
        )
        vec_growths = c.get("lattice_node_vec_growths", 0) + c.get(
            "lattice_edge_vec_growths", 0
        )
        print(
            f"| {r.scenario.group} | {r.scenario.name} | "
            f"{c.get('word_info_requests', 0)} | {c.get('word_info_decodes', 0)} | "
            f"{c.get('owned_string_allocations', 0)} | {c.get('vec_allocations', 0)} | "
            f"{split_decodes} | {c.get('oov_candidates_provided', 0)} | "
            f"{c.get('oov_duplicate_candidates', 0)} | {nodes} | "
            f"{checks_per_node:.2f} | {c.get('lattice_max_nodes_per_boundary', 0)} | "
            f"{vec_growths} | {c.get('lattice_rejected_candidates', 0)} |"
        )


def print_lattice_shape_table(results: list[MatrixResult]) -> None:
    print()
    print(
        "| Group | Scenario | Prev=1 % | Prev 2-4 % | Prev 5-16 % | Prev 17+ % | Left-id hit % | Est saved checks % | Unreachable % | Nodes/unique left-id | Avg unique left-id/bound | Max unique left-id/bound | Fast single % | Direct small % |"
    )
    print(
        "|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|"
    )
    for r in results:
        c = r.counters
        probes = c.get("lattice_left_id_cache_probes", 0)
        hits = c.get("lattice_left_id_cache_hits", 0)
        misses = c.get("lattice_left_id_cache_misses", 0)
        saved_checks = c.get("lattice_left_id_cache_saved_checks", 0)
        checks = c.get("lattice_left_connection_checks", 0)
        baseline_checks = saved_checks + checks
        left_boundaries = c.get("lattice_left_id_boundaries", 0)
        unique_left_ids = c.get("lattice_left_id_total_unique", 0)
        best_prev_calls = c.get("lattice_best_prev_calls", 0)
        fast_single = c.get("lattice_best_prev_fast_single", 0)
        direct_small = c.get("lattice_best_prev_direct_small", 0)
        positions_total = c.get("lattice_positions_total", 0)
        positions_unreachable = c.get("lattice_positions_unreachable_skipped", 0)
        prev_total = (
            c.get("lattice_prev_nodes_0", 0)
            + c.get("lattice_prev_nodes_1", 0)
            + c.get("lattice_prev_nodes_2_4", 0)
            + c.get("lattice_prev_nodes_5_16", 0)
            + c.get("lattice_prev_nodes_17_plus", 0)
        )
        print(
            f"| {r.scenario.group} | {r.scenario.name} | "
            f"{pct(c.get('lattice_prev_nodes_1', 0), prev_total):.1f} | "
            f"{pct(c.get('lattice_prev_nodes_2_4', 0), prev_total):.1f} | "
            f"{pct(c.get('lattice_prev_nodes_5_16', 0), prev_total):.1f} | "
            f"{pct(c.get('lattice_prev_nodes_17_plus', 0), prev_total):.1f} | "
            f"{pct(hits, probes):.1f} | "
            f"{pct(saved_checks, baseline_checks):.1f} | "
            f"{pct(positions_unreachable, positions_total):.1f} | "
            f"{ratio(probes, misses):.2f} | "
            f"{ratio(unique_left_ids, left_boundaries):.2f} | "
            f"{c.get('lattice_left_id_max_unique_per_boundary', 0)} | "
            f"{pct(fast_single, best_prev_calls):.1f} | "
            f"{pct(direct_small, best_prev_calls):.1f} |"
        )


def print_lattice_storage_table(results: list[MatrixResult]) -> None:
    print()
    print(
        "| Group | Scenario | Reset calls | Vecs visited/call | Items cleared | New boundaries | Empty bound % | Avg width | Width 1 % | Width 2-4 % | Width 5-16 % | Width 17+ % | Cap/node | Cache pushes | Cache cap/unique |"
    )
    print(
        "|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|"
    )
    for r in results:
        c = r.counters
        reset_calls = c.get("lattice_reset_calls", 0)
        touched = c.get("lattice_boundaries_touched", 0)
        empty = c.get("lattice_boundaries_empty", 0)
        nodes = c.get("lattice_total_nodes_per_boundary", 0)
        width_1 = c.get("lattice_boundary_width_1", 0)
        width_2_4 = c.get("lattice_boundary_width_2_4", 0)
        width_5_16 = c.get("lattice_boundary_width_5_8", 0) + c.get(
            "lattice_boundary_width_9_16", 0
        )
        width_17_plus = (
            c.get("lattice_boundary_width_17_32", 0)
            + c.get("lattice_boundary_width_33_64", 0)
            + c.get("lattice_boundary_width_65_128", 0)
            + c.get("lattice_boundary_width_129_plus", 0)
        )
        capacity_total = (
            c.get("lattice_ends_capacity_total", 0)
            + c.get("lattice_ends_full_capacity_total", 0)
            + c.get("lattice_indices_capacity_total", 0)
        )
        unique_left_ids = c.get("lattice_left_id_total_unique", 0)
        print(
            f"| {r.scenario.group} | {r.scenario.name} | "
            f"{reset_calls} | "
            f"{ratio(c.get('lattice_reset_vecs_visited', 0), reset_calls):.1f} | "
            f"{c.get('lattice_reset_items_cleared', 0)} | "
            f"{c.get('lattice_reset_new_boundaries', 0)} | "
            f"{pct(empty, empty + touched):.1f} | "
            f"{ratio(nodes, touched):.2f} | "
            f"{pct(width_1, touched):.1f} | "
            f"{pct(width_2_4, touched):.1f} | "
            f"{pct(width_5_16, touched):.1f} | "
            f"{pct(width_17_plus, touched):.1f} | "
            f"{ratio(capacity_total, nodes):.2f} | "
            f"{c.get('lattice_best_prev_cache_pushes', 0)} | "
            f"{ratio(c.get('lattice_best_prev_cache_capacity_total', 0), unique_left_ids):.2f} |"
        )


def print_oov_materialization_table(results: list[MatrixResult]) -> None:
    print()
    print(
        "| Group | Scenario | Buffered OOV cand | Buffered calls | Temp buf growths | Temp buf max len | Node bytes | VNode bytes | BestPrev bytes |"
    )
    print("|---|---|---:|---:|---:|---:|---:|---:|---:|")
    for r in results:
        c = r.counters
        buffered = c.get("oov_buffered_candidates", 0)
        print(
            f"| {r.scenario.group} | {r.scenario.name} | "
            f"{buffered} | "
            f"{c.get('oov_buffered_provider_calls', 0)} | "
            f"{c.get('oov_temp_buffer_growths', 0)} | "
            f"{c.get('oov_temp_buffer_max_len', 0)} | "
            f"{c.get('type_size_node', 0)} | "
            f"{c.get('type_size_vnode', 0)} | "
            f"{c.get('type_size_best_prev', 0)} |"
        )


def print_oov_semantic_table(results: list[MatrixResult]) -> None:
    print()
    print(
        "| Group | Scenario | MeCab cand % | Simple cand % | Regex cand % | Grouped % | Single % | Best-path survival % | Supp has-word | Supp invoke | Supp group | Top category | Top category % | Top len bucket | Top len % |"
    )
    print(
        "|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|---:|---|---:|"
    )
    category_keys = [
        ("DEFAULT", "oov_category_default"),
        ("SPACE", "oov_category_space"),
        ("KANJI", "oov_category_kanji"),
        ("SYMBOL", "oov_category_symbol"),
        ("NUMERIC", "oov_category_numeric"),
        ("ALPHA", "oov_category_alpha"),
        ("HIRAGANA", "oov_category_hiragana"),
        ("KATAKANA", "oov_category_katakana"),
        ("KANJINUMERIC", "oov_category_kanjinumeric"),
        ("GREEK", "oov_category_greek"),
        ("CYRILLIC", "oov_category_cyrillic"),
        ("USER1", "oov_category_user1"),
        ("USER2", "oov_category_user2"),
        ("USER3", "oov_category_user3"),
        ("USER4", "oov_category_user4"),
        ("OTHER", "oov_category_other"),
    ]
    length_keys = [
        ("1", "oov_length_1"),
        ("2", "oov_length_2"),
        ("3", "oov_length_3"),
        ("4", "oov_length_4"),
        ("5-8", "oov_length_5_8"),
        ("9-16", "oov_length_9_16"),
        ("17-32", "oov_length_17_32"),
        ("33+", "oov_length_33_plus"),
    ]
    for r in results:
        c = r.counters
        total = c.get("oov_candidates_provided", 0)
        mecab = c.get("oov_mecab_candidates", 0)
        simple = c.get("oov_simple_candidates", 0)
        regex = c.get("oov_regex_candidates", 0)
        grouped = c.get("oov_grouped_candidates", 0)
        single = c.get("oov_single_candidates", 0)
        best_path = c.get("oov_best_path_nodes", 0)
        top_category, top_category_count = max(
            ((name, c.get(key, 0)) for name, key in category_keys),
            key=lambda item: item[1],
        )
        top_len, top_len_count = max(
            ((name, c.get(key, 0)) for name, key in length_keys),
            key=lambda item: item[1],
        )
        print(
            f"| {r.scenario.group} | {r.scenario.name} | "
            f"{pct(mecab, total):.1f} | {pct(simple, total):.1f} | {pct(regex, total):.1f} | "
            f"{pct(grouped, grouped + single):.1f} | {pct(single, grouped + single):.1f} | "
            f"{pct(best_path, total):.2f} | "
            f"{c.get('oov_suppressed_by_has_other_words', 0)} | "
            f"{c.get('oov_suppressed_by_invoke_false', 0)} | "
            f"{c.get('oov_suppressed_group_by_group_false', 0)} | "
            f"{top_category} | {pct(top_category_count, mecab):.1f} | "
            f"{top_len} | {pct(top_len_count, grouped + single):.1f} |"
        )


def print_oov_dominance_table(results: list[MatrixResult]) -> None:
    print()
    print(
        "| Group | Scenario | OOV cand | Dominance groups | Strict dominated | Dominated % | Equal-cost ties | Unique after dominance | Top dom category | Top dom category % | Top dom len | Top dom len % |"
    )
    print("|---|---|---:|---:|---:|---:|---:|---:|---|---:|---|---:|")
    dominated_category_keys = [
        ("DEFAULT", "oov_dominated_category_default"),
        ("SPACE", "oov_dominated_category_space"),
        ("KANJI", "oov_dominated_category_kanji"),
        ("SYMBOL", "oov_dominated_category_symbol"),
        ("NUMERIC", "oov_dominated_category_numeric"),
        ("ALPHA", "oov_dominated_category_alpha"),
        ("HIRAGANA", "oov_dominated_category_hiragana"),
        ("KATAKANA", "oov_dominated_category_katakana"),
        ("KANJINUMERIC", "oov_dominated_category_kanjinumeric"),
        ("GREEK", "oov_dominated_category_greek"),
        ("CYRILLIC", "oov_dominated_category_cyrillic"),
        ("USER1", "oov_dominated_category_user1"),
        ("USER2", "oov_dominated_category_user2"),
        ("USER3", "oov_dominated_category_user3"),
        ("USER4", "oov_dominated_category_user4"),
        ("OTHER", "oov_dominated_category_other"),
    ]
    dominated_length_keys = [
        ("1", "oov_dominated_length_1"),
        ("2", "oov_dominated_length_2"),
        ("3", "oov_dominated_length_3"),
        ("4", "oov_dominated_length_4"),
        ("5-8", "oov_dominated_length_5_8"),
        ("9-16", "oov_dominated_length_9_16"),
        ("17-32", "oov_dominated_length_17_32"),
        ("33+", "oov_dominated_length_33_plus"),
    ]
    for r in results:
        c = r.counters
        total = c.get("oov_candidates_provided", 0)
        dominated = c.get("oov_strict_dominated_candidates", 0)
        top_category, top_category_count = max(
            ((name, c.get(key, 0)) for name, key in dominated_category_keys),
            key=lambda item: item[1],
        )
        top_len, top_len_count = max(
            ((name, c.get(key, 0)) for name, key in dominated_length_keys),
            key=lambda item: item[1],
        )
        print(
            f"| {r.scenario.group} | {r.scenario.name} | "
            f"{total} | {c.get('oov_dominance_groups', 0)} | "
            f"{dominated} | {pct(dominated, total):.2f} | "
            f"{c.get('oov_equal_cost_ties', 0)} | "
            f"{c.get('oov_unique_after_dominance', 0)} | "
            f"{top_category} | {pct(top_category_count, dominated):.1f} | "
            f"{top_len} | {pct(top_len_count, dominated):.1f} |"
        )


def print_oov_range_table(results: list[MatrixResult]) -> None:
    print()
    print(
        "| Group | Scenario | OOV ranges | Avg cand/range | Max cand/range | Range 0 % | Range 1 % | Range 2-4 % | Range 5-8 % | Range 9-16 % | Range 17-23 % | Range 24+ % |"
    )
    print("|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|")
    for r in results:
        c = r.counters
        ranges = c.get("oov_ranges_total", 0)
        candidates = c.get("oov_range_candidates_total", 0)
        print(
            f"| {r.scenario.group} | {r.scenario.name} | "
            f"{ranges} | {ratio(candidates, ranges):.2f} | "
            f"{c.get('oov_range_max_candidates', 0)} | "
            f"{pct(c.get('oov_range_0', 0), ranges):.1f} | "
            f"{pct(c.get('oov_range_1', 0), ranges):.1f} | "
            f"{pct(c.get('oov_range_2_4', 0), ranges):.1f} | "
            f"{pct(c.get('oov_range_5_8', 0), ranges):.1f} | "
            f"{pct(c.get('oov_range_9_16', 0), ranges):.1f} | "
            f"{pct(c.get('oov_range_17_23', 0), ranges):.1f} | "
            f"{pct(c.get('oov_range_24_plus', 0), ranges):.1f} |"
        )


def pct(part: int, whole: int) -> float:
    return part / whole * 100.0 if whole else 0.0


def ratio(numerator: int, denominator: int) -> float:
    return numerator / denominator if denominator else 0.0


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", default=ROOT / "resources" / "sudachi.json", type=Path)
    parser.add_argument("--resource-dir", default=ROOT / "resources", type=Path)
    parser.add_argument("--dict", default=None, type=Path)
    parser.add_argument("--input", default=None, type=Path)
    parser.add_argument("--mode", default="C", choices=["A", "B", "C", "a", "b", "c"])
    parser.add_argument("--iterations", default=5, type=int)
    parser.add_argument("--repeat", default=1000, type=int)
    parser.add_argument("--sample-interval", default=0.02, type=float)
    parser.add_argument("--scenario", action="append")
    parser.add_argument("--binary", default=None, type=Path)
    parser.add_argument("--no-build", action="store_true")
    parser.add_argument("--skip-errors", action="store_true")
    parser.add_argument(
        "--profile-counters",
        action="store_true",
        help="Build/run profile_pipeline with sudachi/profile counters enabled.",
    )
    args = parser.parse_args()
    if args.iterations <= 0:
        parser.error("--iterations must be greater than zero")
    if args.repeat <= 0:
        parser.error("--repeat must be greater than zero")
    if shutil.which("ps") is None:
        parser.error("ps is required for RSS sampling")
    args.mode = args.mode.upper()
    return args


if __name__ == "__main__":
    main()
