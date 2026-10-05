//! Command-line tool: prints one JSON line of results per audio file.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

use bpm_key::{analyze_bytes, analyze_file, Analysis, Config, Features, KeyProfile, ALGORITHM_VERSION};
use serde::Serialize;
use serde_json::json;

const USAGE: &str = "\
Usage: bpm-key [OPTIONS] FILE...

Prints one JSON object per line for each FILE, in the order given. Use - to read one file from stdin.
Formats: MP3, WAV, FLAC, Ogg Vorbis, AAC/M4A.

Options:
  --threads N            analyse N files at once (default: available cores)
  --prior CENTER,OCTAVES tempo prior centre in BPM and width in octaves (default 140,1.3)
  --prior none           flat tempo prior
  --human-tempo          the human-tapping preset (prior 100,1.3); overrides --prior
  --profile NAME         key profile: krumhansl, temperley, edma, bgate (default), shaath
  --only LIST            run only these analyses, comma-separated: tempo, key, loudness, timbre
  -h, --help             show this help
  -V, --version          show the version

Exit status is 1 if any file failed, 2 on bad arguments.";

struct Args {
    files: Vec<String>,
    threads: usize,
    cfg: Config,
}

fn parse_args(argv: impl Iterator<Item = String>) -> Result<Option<Args>, String> {
    let mut files = Vec::new();
    let mut threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let mut builder = Config::builder();
    let mut human = false;
    let mut argv = argv.peekable();
    let mut only_files = false;
    while let Some(a) = argv.next() {
        if only_files || a == "-" || !a.starts_with('-') {
            files.push(a);
            continue;
        }
        let (name, inline) = match a.split_once('=') {
            Some((n, v)) => (n.to_string(), Some(v.to_string())),
            None => (a.clone(), None),
        };
        let mut value = || inline.clone().or_else(|| argv.next()).ok_or_else(|| format!("{name} needs a value"));
        match name.as_str() {
            "--" => only_files = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(None);
            }
            "-V" | "--version" => {
                println!("bpm-key {}", env!("CARGO_PKG_VERSION"));
                return Ok(None);
            }
            "--threads" => {
                threads = value()?.parse().ok().filter(|&n| n > 0).ok_or("--threads needs a positive integer")?;
            }
            "--prior" => {
                let v = value()?;
                builder = if v.eq_ignore_ascii_case("none") {
                    builder.no_tempo_prior()
                } else {
                    let (c, o) = v.split_once(',').ok_or("--prior needs CENTER,OCTAVES or none")?;
                    let parse =
                        |s: &str| s.trim().parse::<f32>().map_err(|_| format!("--prior: {s:?} is not a number"));
                    builder.tempo_prior(parse(c)?, parse(o)?)
                };
            }
            "--human-tempo" => human = true,
            "--only" => {
                let only = value()?
                    .split(',')
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| s.parse::<Analysis>().map_err(|e| e.to_string()))
                    .collect::<Result<Vec<_>, _>>()?;
                builder = builder.only(only);
            }
            "--profile" => builder = builder.key_profile(value()?.parse::<KeyProfile>().map_err(|e| e.to_string())?),
            _ => return Err(format!("unknown option {a}")),
        }
    }
    if files.is_empty() {
        return Err("no input files".into());
    }
    if files.iter().filter(|f| *f == "-").count() > 1 {
        return Err("stdin (-) can be given only once".into());
    }
    if human {
        if let Some((center, octaves)) = Config::human_tempo().tempo_prior() {
            builder = builder.tempo_prior(center, octaves);
        }
    }
    let cfg = builder.build().map_err(|e| e.to_string())?;
    Ok(Some(Args { files, threads, cfg }))
}

#[derive(Serialize)]
struct Line<'a> {
    file: &'a str,
    algorithm_version: u32,
    #[serde(flatten)]
    features: &'a Features,
}

fn record(file: &str, result: &Result<Features, bpm_key::Error>) -> String {
    match result {
        Ok(features) => serde_json::to_string(&Line { file, algorithm_version: ALGORITHM_VERSION, features }),
        Err(e) => serde_json::to_string(&json!({ "file": file, "error": e.to_string() })),
    }
    .expect("results serialize to JSON")
}

fn analyse(file: &str, cfg: &Config) -> Result<Features, bpm_key::Error> {
    if file == "-" {
        let mut bytes = Vec::new();
        std::io::stdin().read_to_end(&mut bytes)?;
        analyze_bytes(bytes, cfg)
    } else {
        analyze_file(file, cfg)
    }
}

fn main() -> ExitCode {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(Some(a)) => a,
        Ok(None) => return ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("bpm-key: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let next = AtomicUsize::new(0);
    let (tx, rx) = mpsc::channel::<(usize, bool, String)>();
    let mut failed = false;
    std::thread::scope(|s| {
        for _ in 0..args.threads.min(args.files.len()) {
            let tx = tx.clone();
            let (next, args) = (&next, &args);
            s.spawn(move || loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(file) = args.files.get(i) else { break };
                let result = analyse(file, &args.cfg);
                if tx.send((i, result.is_ok(), record(file, &result))).is_err() {
                    break;
                }
            });
        }
        drop(tx);
        // Results arrive out of order; print them in input order as soon as each is due.
        let mut pending = BTreeMap::new();
        let mut due = 0;
        let stdout = std::io::stdout();
        let mut out = stdout.lock();
        for (i, ok, line) in rx {
            pending.insert(i, (ok, line));
            while let Some((ok, line)) = pending.remove(&due) {
                failed |= !ok;
                let _ = writeln!(out, "{line}");
                let _ = out.flush();
                due += 1;
            }
        }
    });
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
