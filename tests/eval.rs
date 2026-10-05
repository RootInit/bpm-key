//! Accuracy regression test against a labelled evaluation set. The audio is not shipped, so the test only
//! runs when `BPM_KEY_EVAL` names a directory holding `clips/*.mp3`, `labels.json`, `key_reference.json`
//! and `synth/`. Run it in release:
//!
//! ```sh
//! BPM_KEY_EVAL=/path/to/eval cargo test --release --test eval -- --nocapture
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rayon::prelude::*;
use serde_json::Value;

use bpm_key::{analyze_bytes, Config, Features};

const PC: [&str; 12] = ["C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"];

fn pitch_class(name: &str) -> u8 {
    let alias = [("Db", "C#"), ("D#", "Eb"), ("Gb", "F#"), ("G#", "Ab"), ("A#", "Bb")];
    let name = alias.iter().find(|(a, _)| *a == name).map_or(name, |(_, b)| b);
    PC.iter().position(|p| *p == name).unwrap_or_else(|| panic!("pitch {name}")) as u8
}

/// MIREX weighted key score.
fn key_score(reference: (u8, bool), est: Option<(u8, bool)>) -> f64 {
    let Some((et, em)) = est else { return 0.0 };
    let (rt, rm) = reference;
    let up = |a: u8, b: u8| (b + 12 - a) % 12;
    if rt == et && rm == em {
        1.0
    } else if rm == em && up(rt, et) == 7 {
        0.5
    } else if !rm && em && up(rt, et) == 9 || rm && !em && up(rt, et) == 3 {
        0.3
    } else if rt == et {
        0.2
    } else {
        0.0
    }
}

fn tempo_acc(reference: f64, est: Option<f64>) -> (bool, bool) {
    let Some(est) = est.filter(|e| *e > 0.0) else { return (false, false) };
    let near = |e: f64| (e - reference).abs() <= 0.04 * reference;
    (near(est), [1.0, 2.0, 0.5, 3.0, 1.0 / 3.0].iter().any(|f| near(est * f)))
}

fn analyse_dir(dir: &Path) -> HashMap<String, Features> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "mp3"))
        .collect();
    files.sort();
    let pool = rayon::ThreadPoolBuilder::new().num_threads(4).build().unwrap();
    pool.install(|| {
        files
            .par_iter()
            .map(|p| {
                let id = p.file_stem().unwrap().to_str().unwrap().to_string();
                let f = analyze_bytes(std::fs::read(p).unwrap(), &Config::default()).unwrap_or_default();
                (id, f)
            })
            .collect()
    })
}

struct Scores {
    acc1: f64,
    acc2: f64,
    mirex: f64,
    n_key: usize,
}

fn score(root: &Path, est: &HashMap<String, Features>) -> Scores {
    let labels: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(root.join("labels.json")).unwrap()).unwrap();
    let mut keyref: HashMap<String, (u8, bool)> = HashMap::new();
    if let Ok(s) = std::fs::read_to_string(root.join("key_reference.json")) {
        let kr: HashMap<String, Value> = serde_json::from_str(&s).unwrap();
        for (id, r) in kr {
            if let (Some(k), Some(m)) = (r["key"].as_str(), r["mode"].as_str()) {
                if r["votes"].as_u64().unwrap_or(3) >= 2 {
                    keyref.insert(id, (pitch_class(k), m == "minor"));
                }
            }
        }
    }
    let (mut a1, mut a2, mut ks, mut nk) = (0usize, 0usize, 0.0, 0usize);
    for l in &labels {
        let id = l["id"].as_str().unwrap();
        if let (Some(k), Some(m)) = (l["key"].as_str(), l["mode"].as_str()) {
            keyref.insert(id.to_string(), (pitch_class(k), m == "minor"));
        }
        let f = est.get(id);
        let (x1, x2) = tempo_acc(l["bpm"].as_f64().unwrap(), f.and_then(|f| f.tempo.as_ref()).map(|t| t.bpm as f64));
        a1 += x1 as usize;
        a2 += x2 as usize;
        if let Some(&r) = keyref.get(id) {
            nk += 1;
            ks += key_score(r, f.and_then(|f| f.key).map(|k| (k.key.tonic.index(), k.key.is_minor())));
        }
    }
    let n = labels.len() as f64;
    Scores { acc1: a1 as f64 / n, acc2: a2 as f64 / n, mirex: ks / nk as f64, n_key: nk }
}

#[test]
fn scores_match_the_recorded_reference() {
    let Ok(root) = std::env::var("BPM_KEY_EVAL") else {
        eprintln!("BPM_KEY_EVAL unset; skipping");
        return;
    };
    let root = PathBuf::from(root);

    let real = analyse_dir(&root.join("clips"));
    let s = score(&root, &real);
    eprintln!(
        "real: {} clips, Acc1 {:.3} Acc2 {:.3}, MIREX {:.3} over {}",
        real.len(),
        s.acc1,
        s.acc2,
        s.mirex,
        s.n_key
    );
    // Recorded with ALGORITHM_VERSION 1 and the default config: 1,498 clips, 1,420 of them with a key.
    assert!((s.acc2 - 0.907).abs() <= 0.01, "Acc2 {}", s.acc2);
    assert!((s.acc1 - 0.685).abs() <= 0.01, "Acc1 {}", s.acc1);
    assert!((s.mirex - 0.813).abs() <= 0.01, "MIREX {}", s.mirex);

    let synth = analyse_dir(&root.join("synth/clips"));
    let t = score(&root.join("synth"), &synth);
    eprintln!("synth: {} clips, Acc1 {:.3} Acc2 {:.3}, key MIREX {:.3}", synth.len(), t.acc1, t.acc2, t.mirex);
    assert!(t.mirex >= 0.97, "synth key MIREX {}", t.mirex);
}
