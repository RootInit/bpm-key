use std::f32::consts::PI;

use bpm_key::advanced::resample;
use bpm_key::{analyze, Config, Error, Features, Key, KeyProfile, Mode, PitchClass, ANALYSIS_RATE};

const SR: u32 = ANALYSIS_RATE;

fn clicks(bpm: f32, secs: f32) -> Vec<f32> {
    let n = (secs * SR as f32) as usize;
    let period = 60.0 / bpm * SR as f32;
    let mut x = vec![0f32; n];
    let mut t = 0.0;
    while (t as usize) < n {
        for i in 0..400.min(n - t as usize) {
            let env = (-(i as f32) / 60.0).exp();
            x[t as usize + i] += env * ((i as f32 * 1.7).sin() + (i as f32 * 0.31).sin()) * 0.5;
        }
        t += period;
    }
    x
}

fn tones(midi: &[f32], cents: f32, secs: f32, rate: u32) -> Vec<f32> {
    let n = (secs * rate as f32) as usize;
    (0..n)
        .map(|i| {
            let t = i as f32 / rate as f32;
            midi.iter()
                .map(|&m| {
                    let f = 440.0 * 2f32.powf((m - 69.0 + cents / 100.0) / 12.0);
                    (2.0 * PI * f * t).sin() * 0.2 + (4.0 * PI * f * t).sin() * 0.08 + (6.0 * PI * f * t).sin() * 0.04
                })
                .sum()
        })
        .collect()
}

fn cfg(bpm: f32, octaves: f32) -> Config {
    Config::builder().tempo_prior(bpm, octaves).build().unwrap()
}

fn analyse_mono(x: &[f32], rate: u32, cfg: &Config) -> Result<Features, Error> {
    analyze(x, 1, rate, cfg)
}

#[test]
fn default_config_is_the_chosen_settings() {
    let c = Config::default();
    assert_eq!((c.tempo_prior(), c.key_profile()), (Some((140.0, 1.3)), KeyProfile::Bgate));
    assert_eq!(Config::builder().build().unwrap(), c);
}

#[test]
fn bad_config_values_are_errors() {
    for (bpm, oct) in [
        (0.0, 1.0),
        (-5.0, 1.0),
        (f32::NAN, 1.0),
        (300.0, 1.0),
        (30.0, 1.0),
        (120.0, 0.0),
        (120.0, -1.0),
        (120.0, f32::INFINITY),
        (f32::INFINITY, 1.0),
    ] {
        assert!(Config::builder().tempo_prior(bpm, oct).build().is_err(), "{bpm} / {oct} accepted");
    }
    assert_eq!("bgate".parse::<KeyProfile>().unwrap(), KeyProfile::Bgate);
    assert_eq!("BGATE".parse::<KeyProfile>().unwrap(), KeyProfile::Bgate);
    for p in KeyProfile::ALL {
        assert_eq!(p.name().parse::<KeyProfile>().unwrap(), p);
    }
    assert!(matches!("bgat".parse::<KeyProfile>(), Err(Error::UnknownKeyProfile(s)) if s == "bgat"));
    assert!("".parse::<KeyProfile>().is_err());
}

#[test]
fn click_trains_hit_their_tempo() {
    for bpm in [87.0, 100.0, 128.0, 140.0] {
        let f = analyse_mono(&clicks(bpm, 30.0), SR, &Config::default()).unwrap();
        let got = f.tempo.unwrap().bpm;
        assert!((got - bpm).abs() / bpm < 0.005, "expected {bpm}, got {got}");
    }
}

#[test]
fn the_prior_decides_an_octave_ambiguous_click_train() {
    // A bare 174 BPM click train is equally consistent with 87; each prior keeps the other reading as a
    // candidate.
    let x = clicks(174.0, 30.0);
    let slow = analyse_mono(&x, SR, &cfg(120.0, 1.0)).unwrap().tempo.unwrap();
    assert!((slow.bpm - 87.0).abs() < 0.5, "got {}", slow.bpm);
    assert!(slow.candidates.iter().any(|c| (c.bpm - 174.0).abs() < 1.0), "{:?}", slow.candidates);
    let fast = analyse_mono(&x, SR, &Config::default()).unwrap().tempo.unwrap();
    assert!((fast.bpm - 174.0).abs() < 1.0, "got {}", fast.bpm);
    assert!(fast.candidates.iter().any(|c| (c.bpm - 87.0).abs() < 0.5), "{:?}", fast.candidates);
    assert_eq!(fast.candidates[0].bpm, fast.bpm);
    assert_eq!(fast.candidates[0].weight, 1.0);
    assert!(fast.candidates.len() <= 3);
}

#[test]
fn c_major_triad_is_c_major_under_every_profile() {
    let x = tones(&[60.0, 64.0, 67.0, 48.0], 0.0, 10.0, SR);
    for p in KeyProfile::ALL {
        let k = analyse_mono(&x, SR, &Config::builder().key_profile(p).build().unwrap()).unwrap().key.unwrap();
        assert_eq!(k.key, Key::new(PitchClass::C, Mode::Major), "{p:?}: {k:?}");
        assert_eq!(k.key.code(), 0);
        assert!(k.margin > 0.0);
    }
}

#[test]
fn detuned_a_minor_triad_reports_tuning_and_key() {
    let f = analyse_mono(&tones(&[57.0, 60.0, 64.0, 45.0], 30.0, 10.0, SR), SR, &Config::default()).unwrap();
    assert!((f.tuning_cents.unwrap() - 30.0).abs() < 5.0, "tuning {:?}", f.tuning_cents);
    let k = f.key.unwrap();
    assert_eq!((k.key.tonic, k.key.mode, k.key.code()), (PitchClass::A, Mode::Minor, 19));
    let chroma = f.chroma.unwrap();
    assert_eq!(chroma.iter().cloned().fold(0f32, f32::max), 1.0);
}

#[test]
fn key_code_is_tonic_times_two_plus_minor() {
    let code = |t: u8, minor: bool| {
        Key::new(PitchClass::from_index(t).unwrap(), if minor { Mode::Minor } else { Mode::Major }).code()
    };
    assert_eq!(code(0, false), 0);
    assert_eq!(code(0, true), 1);
    assert_eq!(code(9, true), 19);
    assert_eq!(code(11, true), 23);
    for c in 0..24 {
        assert_eq!(Key::from_code(c).unwrap().code(), c);
    }
    assert_eq!(Key::from_code(24), None);
}

#[test]
fn input_at_another_rate_is_resampled() {
    let x = tones(&[57.0, 60.0, 64.0, 45.0], 0.0, 10.0, 48_000);
    let f = analyse_mono(&x, 48_000, &Config::default()).unwrap();
    assert_eq!(f.key.unwrap().key.code(), 19);
    assert!((f.duration_secs - 10.0).abs() < 0.01, "{}", f.duration_secs);
}

#[test]
fn descriptors_are_present_for_music() {
    let mut x = clicks(120.0, 20.0);
    for (a, b) in x.iter_mut().zip(tones(&[60.0, 64.0, 67.0], 0.0, 20.0, SR)) {
        *a += b;
    }
    let f = analyse_mono(&x, SR, &Config::default()).unwrap();
    assert!(f.loudness_lufs.unwrap() < 0.0 && f.loudness_lufs.unwrap() > -40.0, "{:?}", f.loudness_lufs);
    assert!(f.loudness_range_lu.is_some());
    assert!(f.rms_dbfs.unwrap() < 0.0 && f.rms_dbfs.unwrap() > -40.0);
    assert!(f.spectral_centroid_hz.unwrap() > 100.0);
    assert!(f.onset_strength.unwrap() > 0.0);
}

fn assert_all_missing(f: &Features) {
    assert!(f.tempo.is_none(), "tempo {:?}", f.tempo);
    assert!(f.key.is_none(), "key {:?}", f.key);
    assert!(f.tuning_cents.is_none() && f.chroma.is_none());
    assert!(f.loudness_lufs.is_none() && f.loudness_range_lu.is_none() && f.rms_dbfs.is_none());
    assert!(f.spectral_centroid_hz.is_none() && f.onset_strength.is_none());
}

#[test]
fn silence_is_missing_not_zero_bpm_or_c_major() {
    assert_all_missing(&analyse_mono(&vec![0.0; 30 * SR as usize], SR, &Config::default()).unwrap());
    // Barely-there noise (about -90 dBFS) must not produce a key either.
    let hiss: Vec<f32> =
        (0..30 * SR as usize).map(|i| 3e-5 * ((i as f32 * 12.9898).sin() * 43758.547).fract()).collect();
    assert_all_missing(&analyse_mono(&hiss, SR, &Config::default()).unwrap());
    assert_all_missing(&analyze(&vec![0.0; 2 * 44_100], 2, 44_100, &Config::default()).unwrap());
}

#[test]
fn tiny_empty_and_broken_clips_are_missing() {
    assert_all_missing(&analyse_mono(&[], SR, &Config::default()).unwrap());
    let short = tones(&[60.0], 0.0, 0.05, SR);
    let f = analyse_mono(&short, SR, &Config::default()).unwrap();
    assert!(f.tempo.is_none() && f.key.is_none());
    let mut nan = tones(&[60.0, 64.0, 67.0], 0.0, 5.0, SR);
    nan[1000] = f32::NAN;
    assert_all_missing(&analyse_mono(&nan, SR, &Config::default()).unwrap());
}

#[test]
fn bad_shapes_are_errors() {
    let c = Config::default();
    assert!(matches!(analyse_mono(&[0.0; 10], 0, &c), Err(Error::ZeroSampleRate)));
    assert!(matches!(analyze(&[0.0; 10], 0, 44_100, &c), Err(Error::ZeroChannels)));
    assert!(matches!(analyze(&[0.0; 11], 2, 44_100, &c), Err(Error::PartialFrame { samples: 11, channels: 2 })));
    let e = analyze(&[0.0; 11], 2, 44_100, &c).unwrap_err();
    assert_eq!(e.to_string(), "11 samples is not a whole number of 2-channel frames");
    assert!(std::error::Error::source(&e).is_none());
}

#[test]
fn resampler_preserves_a_tone() {
    let x: Vec<f32> = (0..44_100).map(|i| (2.0 * PI * 1000.0 * i as f32 / 44_100.0).sin()).collect();
    let y = resample(&x, 44_100, 22_050);
    assert_eq!(y.len(), 22_050);
    let rms = (y[100..22_000].iter().map(|v| v * v).sum::<f32>() / 21_900.0).sqrt();
    assert!((rms - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.01, "rms {rms}");
}
