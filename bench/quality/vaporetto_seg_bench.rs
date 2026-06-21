use std::fs::File;
use std::io::BufReader;
use std::time::Instant;

use vaporetto::{CharacterBoundary, Model, Predictor, Sentence};

fn stats(mut s: Vec<f64>) -> (f64, f64, f64) {
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let min = s[0];
    let med = s[s.len() / 2];
    let mean = s.iter().sum::<f64>() / s.len() as f64;
    let var = s.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / s.len() as f64;
    (min, med, var.sqrt() / mean * 100.0)
}

fn main() {
    let model_path = std::env::var("VAPO_MODEL").expect("set VAPO_MODEL");
    let inputs = std::env::var("VAPO_INPUTS").expect("set VAPO_INPUTS");
    let trials: usize = std::env::var("VAPO_TRIALS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(15);
    let predict_tags = matches!(std::env::var("VAPO_TAGS").ok().as_deref(), Some("1"));

    let f = File::open(&model_path).expect("open model");
    let model = Model::read(BufReader::new(f)).expect("read model");
    let predictor = Predictor::new(model, predict_tags).expect("build predictor");

    let text = std::fs::read_to_string(&inputs).expect("read inputs");
    let lines: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
    let chars: usize = lines.iter().map(|l| l.chars().count()).sum();

    // DUMP mode: emit one tokenization per input line (surfaces tab-joined),
    // 1:1 with the input file, reconstructing tokens from word boundaries.
    if std::env::var("VAPO_DUMP").is_ok() {
        use std::io::Write as _;
        let mut out = std::io::BufWriter::new(std::io::stdout());
        for line in text.lines() {
            let mut s = match Sentence::from_raw(line) {
                Ok(s) => s,
                Err(_) => {
                    writeln!(out).unwrap();
                    continue;
                }
            };
            predictor.predict(&mut s);
            let cs: Vec<char> = line.chars().collect();
            let b = s.boundaries();
            let mut start = 0usize;
            let mut first = true;
            for i in 0..cs.len() {
                let split = i + 1 == cs.len() || matches!(b[i], CharacterBoundary::WordBoundary);
                if split {
                    if !first {
                        write!(out, "\t").unwrap();
                    }
                    first = false;
                    let tk: String = cs[start..=i].iter().collect();
                    write!(out, "{tk}").unwrap();
                    start = i + 1;
                }
            }
            writeln!(out).unwrap();
        }
        return;
    }

    let run = || -> usize {
        let mut toks = 0usize;
        for line in &lines {
            let mut s = match Sentence::from_raw(*line) {
                Ok(s) => s,
                Err(_) => continue,
            };
            predictor.predict(&mut s);
            let wb = s
                .boundaries()
                .iter()
                .filter(|b| matches!(b, CharacterBoundary::WordBoundary))
                .count();
            toks += wb + 1;
        }
        toks
    };

    let warm = run();
    let mut times = Vec::with_capacity(trials);
    for _ in 0..trials {
        let t = Instant::now();
        let _ = run();
        times.push(t.elapsed().as_secs_f64() * 1e3);
    }
    let (min, med, cv) = stats(times);
    println!("# model: {model_path}");
    println!(
        "# inputs: {} lines  {} chars  predict_tags={predict_tags}",
        lines.len(),
        chars
    );
    println!("# tokens(seg): {warm}");
    println!(
        "vaporetto  min {:.2}  median {:.2} ms  cv {:.1}%  |  {:.2} ns/char  |  {:.0} sent/s",
        min,
        med,
        cv,
        med * 1e6 / chars as f64,
        lines.len() as f64 / (med / 1e3)
    );
}
