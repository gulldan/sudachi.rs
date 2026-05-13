/*
 * Copyright (c) 2021-2024 Works Applications Co., Ltd.
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use sudachi::analysis::stateful_tokenizer::StatefulTokenizer;
use sudachi::analysis::Mode;
use sudachi::config::Config;
use sudachi::dic::dictionary::JapaneseDictionary;
use sudachi::prelude::MorphemeList;

struct Args {
    config_file: PathBuf,
    resource_dir: Option<PathBuf>,
    dictionary_path: Option<PathBuf>,
    input: PathBuf,
    output: Option<PathBuf>,
    mode: Mode,
}

fn main() {
    let args = parse_args().unwrap_or_else(|e| {
        eprintln!("{e}");
        usage_and_exit();
    });

    let config = Config::new(
        Some(args.config_file),
        args.resource_dir,
        args.dictionary_path,
    )
    .expect("failed to load config");
    let dict = JapaneseDictionary::from_cfg(&config).expect("failed to load dictionary");
    let mut tokenizer = StatefulTokenizer::create(&dict, false, args.mode);
    let mut morphemes = MorphemeList::empty(&dict);
    let mut split_a = MorphemeList::empty(&dict);
    let mut split_b = MorphemeList::empty(&dict);

    let input = File::open(&args.input)
        .unwrap_or_else(|e| panic!("failed to open input {}: {e}", args.input.display()));
    let mut reader = BufReader::new(input);
    let writer: Box<dyn Write> = match args.output {
        Some(path) => {
            Box::new(BufWriter::new(File::create(&path).unwrap_or_else(|e| {
                panic!("failed to create output {}: {e}", path.display())
            })))
        }
        None => Box::new(BufWriter::new(std::io::stdout())),
    };
    let mut writer = writer;

    let mut line = String::new();
    let mut line_idx = 0usize;
    while reader.read_line(&mut line).expect("failed to read input") > 0 {
        let text = strip_eol(&line);
        tokenizer.reset().push_str(text);
        tokenizer.do_tokenize().expect("tokenization failed");
        morphemes
            .collect_results(&mut tokenizer)
            .expect("result collection failed");

        for (morph_idx, morpheme) in morphemes.iter().enumerate() {
            split_a.clear();
            let has_split_a = morpheme
                .split_into(Mode::A, &mut split_a)
                .expect("A split failed");
            split_b.clear();
            let has_split_b = morpheme
                .split_into(Mode::B, &mut split_b)
                .expect("B split failed");

            write!(
                writer,
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                line_idx,
                morph_idx,
                morpheme.begin_c(),
                morpheme.end_c(),
                escaped(&morpheme.surface()),
                morpheme.word_id().as_raw(),
                morpheme.dictionary_id(),
                morpheme.part_of_speech_id(),
                escaped(morpheme.dictionary_form()),
                escaped(morpheme.normalized_form()),
            )
            .expect("failed to write output");
            write_split(&mut writer, has_split_a.then_some(&split_a)).expect("write failed");
            write_split(&mut writer, has_split_b.then_some(&split_b)).expect("write failed");
            writeln!(writer).expect("failed to write output");
        }

        line.clear();
        line_idx += 1;
    }
}

fn write_split(
    writer: &mut dyn Write,
    split: Option<&MorphemeList<&JapaneseDictionary>>,
) -> std::io::Result<()> {
    write!(writer, "\t")?;
    let Some(split) = split else {
        return Ok(());
    };

    for (idx, morpheme) in split.iter().enumerate() {
        if idx > 0 {
            write!(writer, "|")?;
        }
        write!(
            writer,
            "{}:{}",
            escaped(&morpheme.surface()),
            morpheme.word_id().as_raw()
        )?;
    }
    Ok(())
}

fn escaped(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => result.push_str("\\\\"),
            '\t' => result.push_str("\\t"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            _ => result.push(c),
        }
    }
    result
}

fn strip_eol(data: &str) -> &str {
    data.strip_suffix("\r\n")
        .or_else(|| data.strip_suffix('\n'))
        .or_else(|| data.strip_suffix('\r'))
        .unwrap_or(data)
}

fn parse_args() -> Result<Args, String> {
    let root = workspace_root();
    let mut config_file = root.join("resources/sudachi.json");
    let mut resource_dir = Some(root.join("resources"));
    let mut dictionary_path = None;
    let mut input = None;
    let mut output = None;
    let mut mode = Mode::C;

    let mut iter = env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--config" | "--config-file" => config_file = value_path(&mut iter, &arg)?,
            "--resource-dir" => resource_dir = Some(value_path(&mut iter, &arg)?),
            "--dict" => dictionary_path = Some(value_path(&mut iter, &arg)?),
            "--input" => input = Some(value_path(&mut iter, &arg)?),
            "--output" => output = Some(value_path(&mut iter, &arg)?),
            "--mode" => {
                let value = value_string(&mut iter, &arg)?;
                mode = Mode::from_str(&value).map_err(|e| e.to_owned())?;
            }
            "--help" | "-h" => usage_and_exit(),
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }

    Ok(Args {
        config_file,
        resource_dir,
        dictionary_path,
        input: input.ok_or_else(|| "--input is required".to_owned())?,
        output,
        mode,
    })
}

fn value_path(iter: &mut impl Iterator<Item = String>, arg: &str) -> Result<PathBuf, String> {
    value_string(iter, arg).map(PathBuf::from)
}

fn value_string(iter: &mut impl Iterator<Item = String>, arg: &str) -> Result<String, String> {
    iter.next().ok_or_else(|| format!("{arg} requires a value"))
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("sudachi crate must be inside workspace")
        .to_path_buf()
}

fn usage_and_exit() -> ! {
    eprintln!(
        "Usage: cargo run -p sudachi --release --example accessor_probe -- \\
         --input PATH [--output PATH] [--config PATH] [--resource-dir PATH] [--dict PATH] \\
         [--mode A|B|C]"
    );
    std::process::exit(2);
}
