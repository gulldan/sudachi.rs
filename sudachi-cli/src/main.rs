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

mod analysis;
mod build;
mod output;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::path::PathBuf;
use std::rc::Rc;
use std::str::FromStr;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use clap::Parser;

use crate::analysis::{Analysis, AnalyzeNonSplitted, AnalyzeSplitted, SplitSentencesOnly};
use crate::build::{build_main, is_build_mode, BuildCli};
use sudachi::analysis::stateful_tokenizer::TokenizerOptimization;
use sudachi::config::Config;
use sudachi::dic::dictionary::JapaneseDictionary;
use sudachi::prelude::*;

#[cfg(feature = "bake_dictionary")]
const BAKED_DICTIONARY_BYTES: &[u8] = include_bytes!(env!("SUDACHI_DICT_PATH"));

#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub enum SentenceSplitMode {
    /// Do both sentence splitting and analysis
    #[default]
    Default,
    /// Do only sentence splitting and not analysis
    Only,
    /// Do only analysis without sentence splitting
    None,
}

impl FromStr for SentenceSplitMode {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "yes" | "default" => Ok(SentenceSplitMode::Default),
            "no" | "none" => Ok(SentenceSplitMode::None),
            "only" => Ok(SentenceSplitMode::Only),
            _ => Err("invalid sentence split mode: allowed values - yes, default, no, none, only"),
        }
    }
}

/// A Japanese tokenizer
///
/// If you are looking for options for the dictionary building, try sudachi build/ubuild --help.
#[derive(Parser)]
#[command(
    name = "sudachi",
    version,
    next_line_help = true,
    propagate_version = true
)]
struct Cli {
    /// Input text file: If not present, read from STDIN
    file: Option<PathBuf>,

    /// Path to the setting file in JSON format
    #[arg(short = 'r', long = "config-file")]
    config_file: Option<PathBuf>,

    /// Path to the root directory of resources
    #[arg(short = 'p', long = "resource_dir")]
    resource_dir: Option<PathBuf>,

    /// Split unit: "A" (short), "B" (middle), or "C" (Named Entity)
    #[arg(short = 'm', long = "mode", default_value = "C")]
    mode: Mode,

    /// Output text file: If not present, use stdout
    #[arg(short = 'o', long = "output")]
    output_file: Option<PathBuf>,

    /// Prints all fields
    #[arg(short = 'a', long = "all")]
    print_all: bool,

    /// Outputs only surface form
    #[arg(short = 'w', long = "wakati")]
    wakati: bool,

    /// Debug mode: Print the debug information
    #[arg(short = 'd', long = "debug")]
    enable_debug: bool,

    /// Path to sudachi dictionary.
    /// If None, it refer config and then baked dictionary
    #[arg(short = 'l', long = "dict")]
    dictionary_path: Option<PathBuf>,

    /// How to split sentences.
    ///
    /// "yes", "default" means split sentences,
    /// "no", "none" means don't split sentences,
    /// "only" means split sentences, do not perform analysis
    #[arg(long = "split-sentences", default_value = "yes")]
    split_sentences: SentenceSplitMode,

    /// Tokenize input lines in parallel, preserving output order.
    #[arg(long = "threads", default_value_t = 1)]
    threads: usize,

    /// Enable exact-safe right-id dominance pruning. Intended for benchmarking.
    #[arg(long = "exact-prune", hide = true)]
    exact_prune: bool,

    /// Experimental approximate mode: keep at most N lattice nodes per boundary.
    #[arg(long = "beam-width")]
    beam_width: Option<usize>,

    /// Experimental approximate mode: drop boundary nodes whose accumulated cost is more than COST above the best node.
    #[arg(long = "beam-margin")]
    beam_margin: Option<i32>,

    /// Experimental approximate mode: insert at most N OOV candidates from each OOV provider call.
    #[arg(long = "fast-oov-limit")]
    fast_oov_limit: Option<usize>,

    #[command(subcommand)]
    command: Option<BuildCli>,
}

const PARALLEL_BATCH_LINES: usize = 256;
const PARALLEL_IN_FLIGHT_MULTIPLIER: usize = 2;

// want to instantiate a different type for different output format
// this takes a f as a function which will be created with a different actual type
macro_rules! with_output {
    ($cli: expr, $f: expr) => {
        if $cli.wakati {
            Box::new($f(output::Wakachi::default()))
        } else {
            Box::new($f(output::Simple::new($cli.print_all)))
        }
    };
}

fn main() {
    let args: Cli = Cli::parse();
    validate_cli(&args);

    if is_build_mode(&args.command) {
        build_main(args.command.unwrap());
        return;
    }

    let inner_reader: Box<dyn Read> = match args.file.as_ref() {
        Some(input_path) => Box::new(
            File::open(input_path)
                .unwrap_or_else(|_| panic!("Failed to open input file {:?}", &input_path)),
        ),
        None => Box::new(io::stdin()),
    };

    // input: stdin or file
    let mut reader = BufReader::new(inner_reader);

    // output: stdout or file
    let inner_writer: Box<dyn Write> = match &args.output_file {
        Some(output_path) => Box::new(
            File::create(output_path)
                .unwrap_or_else(|_| panic!("Failed to open output file {:?}", &output_path)),
        ),
        None => Box::new(io::stdout()),
    };
    let mut writer = BufWriter::new(inner_writer);

    // load config file
    let config = Config::new(
        args.config_file.clone(),
        args.resource_dir.clone(),
        args.dictionary_path.clone(),
    )
    .expect("Failed to load config file");

    let dict = JapaneseDictionary::from_cfg(&config)
        .unwrap_or_else(|e| panic!("Failed to create dictionary: {:?}", e));

    let mut data = String::with_capacity(4 * 1024);
    let is_stdout = args.output_file.is_none();

    if args.threads <= 1 {
        let mut analyzer = create_analysis(&args, &dict);

        // tokenize and output results
        while reader.read_line(&mut data).expect("readline failed") > 0 {
            let no_eol = strip_eol(&data);
            analyzer.analyze(no_eol, &mut writer);
            if is_stdout {
                // for stdout we want to flush every result
                writer.flush().expect("flush failed");
            }
            data.clear();
        }
    } else {
        analyze_parallel(&args, &dict, &mut reader, &mut writer);
    }

    // it is recommended to call write before dropping BufWriter
    writer.flush().expect("flush failed");
}

fn validate_cli(args: &Cli) {
    if matches!(args.beam_width, Some(0)) {
        eprintln!("--beam-width must be greater than zero");
        std::process::exit(2);
    }
    if matches!(args.fast_oov_limit, Some(0)) {
        eprintln!("--fast-oov-limit must be greater than zero");
        std::process::exit(2);
    }
    if matches!(args.beam_margin, Some(value) if value < 0) {
        eprintln!("--beam-margin must be zero or greater");
        std::process::exit(2);
    }
}

fn create_analysis<'a>(args: &Cli, dict: &'a JapaneseDictionary) -> Box<dyn Analysis + 'a> {
    let optimization = tokenizer_optimization(args);
    let default_optimization = optimization == TokenizerOptimization::default();
    match args.split_sentences {
        SentenceSplitMode::Only => Box::new(SplitSentencesOnly::new(dict)),
        SentenceSplitMode::Default if default_optimization => with_output!(args, |o| {
            AnalyzeSplitted::new(o, dict, args.mode, args.enable_debug)
        }),
        SentenceSplitMode::Default => with_output!(args, |o| {
            AnalyzeSplitted::new_with_optimization(
                o,
                dict,
                args.mode,
                args.enable_debug,
                optimization,
            )
        }),
        SentenceSplitMode::None if default_optimization => with_output!(args, |o| {
            AnalyzeNonSplitted::new(o, dict, args.mode, args.enable_debug)
        }),
        SentenceSplitMode::None => with_output!(args, |o| {
            AnalyzeNonSplitted::new_with_optimization(
                o,
                dict,
                args.mode,
                args.enable_debug,
                optimization,
            )
        }),
    }
}

fn tokenizer_optimization(args: &Cli) -> TokenizerOptimization {
    TokenizerOptimization {
        exact_right_id_pruning: args.exact_prune,
        beam_width: args.beam_width,
        beam_margin: args.beam_margin,
        oov_limit: args.fast_oov_limit,
    }
}

fn analyze_parallel(
    args: &Cli,
    dict: &JapaneseDictionary,
    reader: &mut impl BufRead,
    writer: &mut output::Writer,
) {
    thread::scope(|scope| {
        let threads = args.threads;
        let (job_tx, job_rx) = mpsc::channel::<ParallelJob>();
        let (result_tx, result_rx) = mpsc::channel::<ParallelResult>();
        let job_rx = Arc::new(Mutex::new(job_rx));

        for _ in 0..threads {
            let job_rx = Arc::clone(&job_rx);
            let result_tx = result_tx.clone();
            scope.spawn(move || {
                let mut analyzer = create_analysis(args, dict);

                loop {
                    let job = {
                        let rx = job_rx.lock().expect("job receiver poisoned");
                        rx.recv()
                    };
                    let Ok(job) = job else {
                        break;
                    };

                    let (mut writer, buffer) = memory_writer();
                    for line in &job.lines {
                        analyzer.analyze(line, &mut writer);
                    }

                    writer.flush().expect("write failed");
                    drop(writer);
                    let output = Rc::try_unwrap(buffer)
                        .expect("buffer still borrowed")
                        .into_inner();
                    result_tx
                        .send(ParallelResult {
                            seq: job.seq,
                            output,
                        })
                        .expect("send result failed");
                }
            });
        }
        drop(result_tx);

        let max_in_flight = threads * PARALLEL_IN_FLIGHT_MULTIPLIER;
        let mut data = String::with_capacity(4 * 1024);
        let mut eof = false;
        let mut in_flight = 0usize;
        let mut next_job = 0usize;
        let mut next_write = 0usize;
        let mut pending = BTreeMap::new();

        loop {
            while !eof && in_flight < max_in_flight {
                let lines = read_parallel_batch(reader, &mut data, &mut eof);
                if lines.is_empty() {
                    break;
                }
                job_tx
                    .send(ParallelJob {
                        seq: next_job,
                        lines,
                    })
                    .expect("send job failed");
                next_job += 1;
                in_flight += 1;
            }

            if in_flight == 0 {
                break;
            }

            let result = result_rx.recv().expect("worker result missing");
            in_flight -= 1;
            pending.insert(result.seq, result.output);
            while let Some(output) = pending.remove(&next_write) {
                writer.write_all(&output).expect("write failed");
                next_write += 1;
            }
        }
        drop(job_tx);
    });
}

fn read_parallel_batch(
    reader: &mut impl BufRead,
    data: &mut String,
    eof: &mut bool,
) -> Vec<String> {
    let mut lines = Vec::with_capacity(PARALLEL_BATCH_LINES);
    while lines.len() < PARALLEL_BATCH_LINES {
        data.clear();
        let read = reader.read_line(data).expect("readline failed");
        if read == 0 {
            *eof = true;
            break;
        }
        lines.push(strip_eol(data).to_owned());
    }
    lines
}

struct ParallelJob {
    seq: usize,
    lines: Vec<String>,
}

struct ParallelResult {
    seq: usize,
    output: Vec<u8>,
}

struct SharedBuffer(Rc<RefCell<Vec<u8>>>);

impl Write for SharedBuffer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.borrow_mut().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn memory_writer() -> (output::Writer, Rc<RefCell<Vec<u8>>>) {
    let buffer = Rc::new(RefCell::new(Vec::new()));
    let writer = BufWriter::new(Box::new(SharedBuffer(buffer.clone())) as Box<dyn Write>);
    (writer, buffer)
}

/// strip (\r?\n)? pattern at the end of string
fn strip_eol(data: &str) -> &str {
    let mut bytes = data.as_bytes();
    let mut len = bytes.len();
    if len > 1 && bytes[len - 1] == b'\n' {
        len -= 1;
        bytes = &bytes[..len];
        if len > 1 && bytes[len - 1] == b'\r' {
            len -= 1;
            bytes = &bytes[..len];
        }
    }

    // Safety: str was correct and we only removed full characters
    unsafe { std::str::from_utf8_unchecked(bytes) }
}
#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::Cli;

    /// Verify that the CLI definition is valid.
    #[test]
    fn verify_cli() {
        Cli::command().debug_assert()
    }
}
