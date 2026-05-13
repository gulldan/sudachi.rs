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
import difflib
import glob
import hashlib
import os
import shutil
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Iterable


ROOT = Path(__file__).resolve().parents[1]


@dataclass
class RunResult:
    impl: str
    status: str
    elapsed: list[float] = field(default_factory=list)
    peak_rss_kib: list[int] = field(default_factory=list)
    rows: int = 0
    sha256: str = ""
    output: Path | None = None
    stderr: Path | None = None
    message: str = ""

    @property
    def best_elapsed(self) -> float:
        return min(self.elapsed) if self.elapsed else 0.0

    @property
    def avg_elapsed(self) -> float:
        return sum(self.elapsed) / len(self.elapsed) if self.elapsed else 0.0

    @property
    def peak_rss_mib(self) -> float:
        return max(self.peak_rss_kib) / 1024.0 if self.peak_rss_kib else 0.0

    @property
    def rows_per_sec(self) -> float:
        return self.rows / self.best_elapsed if self.best_elapsed > 0 else 0.0


def main() -> None:
    args = parse_args()
    out_dir = args.out_dir
    out_dir.mkdir(parents=True, exist_ok=True)

    impls = args.impl or ["rust", "python", "java"]
    results: list[RunResult] = []

    if "rust" in impls:
        results.append(run_rust(args, out_dir))
    if "python" in impls:
        results.append(run_python(args, out_dir))
    if "java" in impls:
        results.append(run_java(args, out_dir))

    reference = next((r for r in results if r.status == "OK"), None)
    for result in results:
        if reference is None or result.status != "OK" or result is reference:
            continue
        if result.sha256 != reference.sha256:
            result.status = "DIFF"
            result.message = write_diff(reference, result, out_dir)

    print_table(results, reference.impl if reference else None)
    if any(r.status in {"FAIL", "DIFF"} for r in results):
        raise SystemExit(1)


def run_rust(args: argparse.Namespace, out_dir: Path) -> RunResult:
    binary = args.rust_binary or ROOT / "target" / "release" / "examples" / "accessor_probe"
    if not args.rust_binary and not args.no_build_rust:
        build = subprocess.run(
            ["cargo", "build", "-p", "sudachi", "--release", "--example", "accessor_probe"],
            cwd=ROOT,
        )
        if build.returncode != 0:
            return RunResult("rust", "FAIL", message="cargo build failed")
    if not binary.exists():
        return RunResult("rust", "SKIP", message=f"missing {binary}")

    cmd = [
        str(binary),
        "--config",
        str(runtime_config(args, "rust")),
        "--resource-dir",
        str(runtime_resource_dir(args, "rust")),
        "--input",
        str(args.input),
        "--mode",
        args.mode,
    ]
    if dictionary := runtime_dict(args, "rust"):
        cmd.extend(["--dict", str(dictionary)])
    return run_probe("rust", cmd, out_dir, args.runs, args.sample_interval)


def run_python(args: argparse.Namespace, out_dir: Path) -> RunResult:
    python = shutil.which(args.python)
    if python is None:
        return RunResult("python", "SKIP", message=f"{args.python} not found")

    script = ROOT / "tools" / "cross_runtime" / "python_probe.py"
    cmd = [
        python,
        str(script),
        "--config",
        str(runtime_config(args, "python")),
        "--resource-dir",
        str(runtime_resource_dir(args, "python")),
        "--input",
        str(args.input),
        "--mode",
        args.mode,
    ]
    if dictionary := runtime_dict(args, "python"):
        cmd.extend(["--dict", str(dictionary)])

    env = os.environ.copy()
    if args.python_path:
        python_path = os.pathsep.join(str(p) for p in args.python_path)
        env["PYTHONPATH"] = (
            python_path + os.pathsep + env["PYTHONPATH"] if env.get("PYTHONPATH") else python_path
        )
    if not can_import_sudachipy(python, env):
        return RunResult(
            "python",
            "SKIP",
            message="sudachipy is not importable; install/build it or pass --python-path",
        )
    return run_probe("python", cmd, out_dir, args.runs, args.sample_interval, env=env)


def run_java(args: argparse.Namespace, out_dir: Path) -> RunResult:
    if not args.java_classpath:
        return RunResult("java", "SKIP", message="pass --java-classpath for Sudachi develop-v0.8")

    javac = shutil.which(args.javac)
    java = shutil.which(args.java)
    if javac is None or java is None:
        return RunResult("java", "SKIP", message="javac/java not found")

    classpath = expand_classpath(args.java_classpath)
    classes = out_dir / "java-classes"
    classes.mkdir(parents=True, exist_ok=True)
    source = ROOT / "tools" / "cross_runtime" / "AccessorProbe.java"
    compile_cmd = [javac, "-encoding", "UTF-8", "-cp", classpath, "-d", str(classes), str(source)]
    compile_result = subprocess.run(compile_cmd, cwd=ROOT)
    if compile_result.returncode != 0:
        return RunResult("java", "FAIL", message="javac failed")

    runtime_cp = str(classes) + os.pathsep + classpath
    cmd = [
        java,
        "-cp",
        runtime_cp,
        "AccessorProbe",
        "--config",
        str(runtime_config(args, "java")),
        "--resource-dir",
        str(runtime_resource_dir(args, "java")),
        "--input",
        str(args.input),
        "--mode",
        args.mode,
    ]
    if dictionary := runtime_dict(args, "java"):
        cmd.extend(["--dict", str(dictionary)])
    return run_probe("java", cmd, out_dir, args.runs, args.sample_interval)


def run_probe(
    impl: str,
    base_cmd: list[str],
    out_dir: Path,
    runs: int,
    sample_interval: float,
    env: dict[str, str] | None = None,
) -> RunResult:
    result = RunResult(impl, "OK")
    for run_idx in range(runs):
        output = out_dir / f"{impl}.{run_idx}.tsv"
        stderr = out_dir / f"{impl}.{run_idx}.stderr"
        cmd = [*base_cmd, "--output", str(output)]
        with open(stderr, "wb") as err:
            started = time.perf_counter()
            with subprocess.Popen(cmd, cwd=ROOT, stderr=err, env=env) as proc:
                peak = sample_peak_rss(proc, sample_interval)
                code = proc.wait()
            elapsed = time.perf_counter() - started

        result.elapsed.append(elapsed)
        result.peak_rss_kib.append(peak)
        result.stderr = stderr
        if code != 0:
            result.status = "FAIL"
            result.output = output
            result.message = f"exit code {code}; stderr: {stderr}"
            return result

        if run_idx == 0:
            result.output = output
            result.sha256 = sha256_file(output)
            result.rows = count_lines(output)

    return result


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


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def count_lines(path: Path) -> int:
    with open(path, "rb") as f:
        return sum(1 for _ in f)


def write_diff(reference: RunResult, result: RunResult, out_dir: Path) -> str:
    assert reference.output is not None
    assert result.output is not None
    diff_path = out_dir / f"{result.impl}_vs_{reference.impl}.diff"
    with open(reference.output, encoding="utf-8") as a, open(result.output, encoding="utf-8") as b:
        diff = difflib.unified_diff(
            a.readlines(),
            b.readlines(),
            fromfile=str(reference.output),
            tofile=str(result.output),
        )
        with open(diff_path, "w", encoding="utf-8") as out:
            for idx, line in enumerate(diff):
                out.write(line)
                if idx > 2000:
                    out.write("...\n")
                    break
    return f"diff: {diff_path}"


def print_table(results: list[RunResult], reference_impl: str | None) -> None:
    print(f"Reference\t{reference_impl or 'none'}")
    print()
    print(
        "| Impl | Status | Avg elapsed (s) | Best elapsed (s) | Rows/s | Peak RSS (MiB) | Rows | SHA-256 | Note |"
    )
    print("|---|---:|---:|---:|---:|---:|---:|---|---|")
    for r in results:
        sha = r.sha256[:12] if r.sha256 else ""
        print(
            f"| {r.impl} | {r.status} | {r.avg_elapsed:.3f} | {r.best_elapsed:.3f} | "
            f"{r.rows_per_sec:.0f} | {r.peak_rss_mib:.1f} | {r.rows} | `{sha}` | {r.message} |"
        )


def expand_classpath(classpath: str) -> str:
    parts: list[str] = []
    for part in classpath.split(os.pathsep):
        matches = glob.glob(part)
        if matches:
            parts.extend(matches)
        else:
            parts.append(part)
    return os.pathsep.join(parts)


def can_import_sudachipy(python: str, env: dict[str, str]) -> bool:
    result = subprocess.run(
        [python, "-c", "import sudachipy"],
        cwd=ROOT,
        env=env,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    return result.returncode == 0


def runtime_config(args: argparse.Namespace, impl: str) -> Path:
    return getattr(args, f"{impl}_config") or args.config


def runtime_resource_dir(args: argparse.Namespace, impl: str) -> Path:
    return getattr(args, f"{impl}_resource_dir") or args.resource_dir


def runtime_dict(args: argparse.Namespace, impl: str) -> Path | None:
    return getattr(args, f"{impl}_dict") or args.dict


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--input", required=True, type=Path)
    parser.add_argument("--config", default=ROOT / "resources" / "sudachi.json", type=Path)
    parser.add_argument("--resource-dir", default=ROOT / "resources", type=Path)
    parser.add_argument("--dict", default=None, type=Path)
    parser.add_argument("--rust-config", default=None, type=Path)
    parser.add_argument("--rust-resource-dir", default=None, type=Path)
    parser.add_argument("--rust-dict", default=None, type=Path)
    parser.add_argument("--python-config", default=None, type=Path)
    parser.add_argument("--python-resource-dir", default=None, type=Path)
    parser.add_argument("--python-dict", default=None, type=Path)
    parser.add_argument("--java-config", default=None, type=Path)
    parser.add_argument("--java-resource-dir", default=None, type=Path)
    parser.add_argument("--java-dict", default=None, type=Path)
    parser.add_argument("--mode", default="C", choices=["A", "B", "C", "a", "b", "c"])
    parser.add_argument("--impl", action="append", choices=["rust", "python", "java"])
    parser.add_argument("--runs", default=1, type=int)
    parser.add_argument("--sample-interval", default=0.02, type=float)
    parser.add_argument("--out-dir", default=ROOT / "target" / "cross-runtime", type=Path)
    parser.add_argument("--no-build-rust", action="store_true")
    parser.add_argument("--rust-binary", default=None, type=Path)
    parser.add_argument("--python", default=sys.executable)
    parser.add_argument(
        "--python-path",
        action="append",
        type=Path,
        help="Extra PYTHONPATH entry, e.g. python/py_src for a local editable build.",
    )
    parser.add_argument("--java", default="java")
    parser.add_argument("--javac", default="javac")
    parser.add_argument(
        "--java-classpath",
        default=None,
        help="Classpath containing Sudachi develop-v0.8 classes/jar and runtime dependencies.",
    )
    args = parser.parse_args()
    if args.runs <= 0:
        parser.error("--runs must be greater than zero")
    args.mode = args.mode.upper()
    return args


if __name__ == "__main__":
    main()
