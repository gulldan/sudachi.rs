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
from pathlib import Path

from sudachipy import Dictionary, SplitMode


FIELDS = {
    "surface",
    "pos",
    "normalized_form",
    "dictionary_form",
    "split_a",
    "split_b",
}


def main() -> None:
    args = parse_args()
    dictionary = Dictionary(
        config_path=str(args.config),
        resource_dir=str(args.resource_dir) if args.resource_dir else None,
        dict=str(args.dict) if args.dict else None,
    )
    tokenizer = dictionary.create(mode=SplitMode(args.mode), fields=FIELDS)

    out = open(args.output, "w", encoding="utf-8", newline="\n") if args.output else None
    try:
        writer = out if out is not None else None
        with open(args.input, "r", encoding="utf-8") as reader:
            for line_idx, line in enumerate(reader):
                text = line.removesuffix("\n").removesuffix("\r")
                morphemes = tokenizer.tokenize(text)
                for morph_idx, morpheme in enumerate(morphemes):
                    split_a = morpheme.split(SplitMode.A, add_single=False)
                    split_b = morpheme.split(SplitMode.B, add_single=False)
                    row = [
                        str(line_idx),
                        str(morph_idx),
                        str(morpheme.begin()),
                        str(morpheme.end()),
                        escaped(morpheme.raw_surface()),
                        str(morpheme.word_id()),
                        str(morpheme.dictionary_id()),
                        str(morpheme.part_of_speech_id()),
                        escaped(morpheme.dictionary_form()),
                        escaped(morpheme.normalized_form()),
                        format_split(split_a),
                        format_split(split_b),
                    ]
                    line_out = "\t".join(row) + "\n"
                    if writer is None:
                        print(line_out, end="")
                    else:
                        writer.write(line_out)
    finally:
        if out is not None:
            out.close()


def format_split(morphemes) -> str:
    return "|".join(f"{escaped(m.raw_surface())}:{m.word_id()}" for m in morphemes)


def escaped(text: str) -> str:
    return (
        text.replace("\\", "\\\\")
        .replace("\t", "\\t")
        .replace("\n", "\\n")
        .replace("\r", "\\r")
    )


def parse_args() -> argparse.Namespace:
    root = Path(__file__).resolve().parents[2]
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", default=root / "resources" / "sudachi.json", type=Path)
    parser.add_argument("--resource-dir", default=root / "resources", type=Path)
    parser.add_argument("--dict", default=None, type=Path)
    parser.add_argument("--input", required=True, type=Path)
    parser.add_argument("--output", default=None, type=Path)
    parser.add_argument("--mode", default="C", choices=["A", "B", "C", "a", "b", "c"])
    return parser.parse_args()


if __name__ == "__main__":
    main()
