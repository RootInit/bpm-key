//! Smoke test of the `bpm-key` binary.

use std::f32::consts::PI;
use std::io::Write;
use std::process::{Command, Stdio};

use bpm_key::{analyze_file, Config, Features, ALGORITHM_VERSION};
use serde_json::Value;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_bpm-key"))
}

/// A mono 16-bit WAV of a C major triad over a 100 BPM click.
fn wav() -> Vec<u8> {
    let rate = 22_050u32;
    let n = 10 * rate as usize;
    let period = (0.6 * rate as f32) as usize;
    let samples: Vec<i16> = (0..n)
        .map(|i| {
            let t = i as f32 / rate as f32;
            let tone: f32 = [261.63f32, 329.63, 392.0, 130.81].iter().map(|f| (2.0 * PI * f * t).sin() * 0.12).sum();
            let j = i % period;
            let click = if j < 400 { (-(j as f32) / 60.0).exp() * (j as f32 * 1.7).sin() * 0.4 } else { 0.0 };
            ((tone + click).clamp(-1.0, 1.0) * 32767.0) as i16
        })
        .collect();
    let mut w = b"RIFF".to_vec();
    w.extend_from_slice(&(36 + 2 * n as u32).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    for v in [16u32, 1 | (1 << 16), rate, rate * 2, 2 | (16 << 16)] {
        w.extend_from_slice(&v.to_le_bytes());
    }
    w.extend_from_slice(b"data");
    w.extend_from_slice(&(2 * n as u32).to_le_bytes());
    samples.iter().for_each(|s| w.extend_from_slice(&s.to_le_bytes()));
    w
}

fn wav_file() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("bpm-key-cli-only-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("c.wav");
    std::fs::write(&path, wav()).unwrap();
    path
}

fn lines(out: &[u8]) -> Vec<Value> {
    String::from_utf8(out.to_vec()).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

#[test]
fn prints_one_json_line_per_file_in_order() {
    let dir = std::env::temp_dir().join(format!("bpm-key-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let good = dir.join("c.wav");
    std::fs::write(&good, wav()).unwrap();
    let missing = dir.join("missing.mp3");
    let out = bin()
        .args(["--threads", "2", "--profile", "temperley", "--prior", "120,1.0"])
        .arg(&good)
        .arg(&missing)
        .arg(&good)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "a missing file fails the run");
    let v = lines(&out.stdout);
    assert_eq!(v.len(), 3);
    assert_eq!(v[0]["file"], good.to_str().unwrap());
    assert_eq!(v[0]["algorithm_version"], ALGORITHM_VERSION);
    assert_eq!(v[0]["key"]["name"], "C major");
    assert_eq!(v[0]["key"]["camelot"], "8B");
    assert_eq!(v[0]["key"]["open_key"], "1d");
    let bpm = v[0]["tempo"]["bpm"].as_f64().unwrap();
    assert!((bpm - 100.0).abs() < 1.0, "{bpm}");
    assert!(v[1]["error"].as_str().unwrap().contains("cannot read audio"), "{}", v[1]);
    assert_eq!(v[0], v[2]);

    // Apart from `file` and `algorithm_version`, a line is the library's serde form of `Features`.
    let cfg = Config::builder().tempo_prior(120.0, 1.0).key_profile(bpm_key::KeyProfile::Temperley).build().unwrap();
    let parsed: Features = serde_json::from_value(v[0].clone()).unwrap();
    assert_eq!(parsed, analyze_file(&good, &cfg).unwrap());
}

#[test]
fn reads_stdin_and_rejects_bad_arguments() {
    let mut child = bin().args(["--human-tempo", "-"]).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(&wav()).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    let v = lines(&out.stdout);
    assert_eq!((v.len(), v[0]["file"].as_str()), (1, Some("-")));
    assert!(v[0]["tempo"]["bpm"].as_f64().is_some());

    for args in
        [&["--prior", "300,1"][..], &["--profile", "nope", "x"], &["--threads", "0", "x"], &[], &["--bogus", "x"]]
    {
        let out = bin().args(args).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(out.stdout.is_empty());
    }
    let out = bin().args(["--only", "tempo", "--"]).arg(wav_file()).output().unwrap();
    let v = lines(&out.stdout);
    assert!(v[0]["tempo"]["bpm"].is_number() && v[0]["key"].is_null() && v[0]["loudness_lufs"].is_null());
    assert_eq!(bin().args(["--only", "tempo,bpm", "x"]).output().unwrap().status.code(), Some(2));

    let help = bin().arg("--help").output().unwrap();
    assert!(help.status.success() && String::from_utf8_lossy(&help.stdout).contains("Usage: bpm-key"));
}
