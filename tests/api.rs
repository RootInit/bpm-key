use bpm_key::same_tempo;
use bpm_key::{analyze, Config, Error, Key, KeyProfile, PitchClass, ALGORITHM_VERSION};

#[test]
fn algorithm_version_is_one() {
    assert_eq!(ALGORITHM_VERSION, 1);
}

#[test]
fn builder_validates_and_overrides() {
    let c = Config::builder().tempo_prior(120.0, 1.0).key_profile(KeyProfile::Temperley).build().unwrap();
    assert_eq!((c.tempo_prior(), c.key_profile()), (Some((120.0, 1.0)), KeyProfile::Temperley));
    let flat = Config::builder().no_tempo_prior().build().unwrap();
    assert_eq!((flat.tempo_prior(), flat.key_profile()), (None, KeyProfile::Bgate));
    // A later prior replaces an earlier "none", and the edges of the range are allowed.
    let edge = Config::builder().no_tempo_prior().tempo_prior(40.0, 0.01).build().unwrap();
    assert_eq!(edge.tempo_prior(), Some((40.0, 0.01)));
    assert!(Config::builder().tempo_prior(240.0, 5.0).build().is_ok());

    assert!(matches!(Config::builder().tempo_prior(39.9, 1.0).build(), Err(Error::TempoPriorCenter(b)) if b == 39.9));
    assert!(matches!(Config::builder().tempo_prior(f32::NAN, 1.0).build(), Err(Error::TempoPriorCenter(_))));
    assert!(matches!(Config::builder().tempo_prior(120.0, 0.0).build(), Err(Error::TempoPriorWidth(w)) if w == 0.0));
    assert!(matches!(Config::builder().tempo_prior(120.0, f32::NAN).build(), Err(Error::TempoPriorWidth(_))));
    let msg = Config::builder().tempo_prior(300.0, 1.0).build().unwrap_err().to_string();
    assert_eq!(msg, "tempo prior 300 BPM is outside 40..=240 BPM");
}

#[test]
fn presets() {
    let h = Config::human_tempo();
    assert_eq!((h.tempo_prior(), h.key_profile()), (Some((100.0, 1.3)), KeyProfile::Bgate));
    assert_ne!(h, Config::default());
}

/// A click train at `bpm`, 30 s at 22 050 Hz.
fn clicks(bpm: f32) -> Vec<f32> {
    let sr = 22_050.0;
    let n = (30.0 * sr) as usize;
    let mut x = vec![0f32; n];
    let mut t = 0.0;
    while (t as usize) < n {
        for i in 0..400.min(n - t as usize) {
            x[t as usize + i] += (-(i as f32) / 60.0).exp() * ((i as f32 * 1.7).sin() + (i as f32 * 0.31).sin()) * 0.5;
        }
        t += 60.0 / bpm * sr;
    }
    x
}

#[test]
fn human_tempo_preset_counts_a_slow_pulse_slow() {
    // A bare 174 BPM click train is equally consistent with 87.
    let x = clicks(174.0);
    let default = analyze(&x, 1, 22_050, &Config::default()).unwrap().tempo.unwrap();
    let human = analyze(&x, 1, 22_050, &Config::human_tempo()).unwrap().tempo.unwrap();
    assert!((default.bpm - 174.0).abs() < 1.0, "default {}", default.bpm);
    assert!((human.bpm - 87.0).abs() < 0.5, "human {}", human.bpm);
    assert!(same_tempo(default.bpm, human.bpm, 0.01));
    let alt = default.octave_alternative().expect("the other octave is a candidate");
    assert!((alt.bpm - 87.0).abs() < 0.5, "{alt:?}");
    assert_eq!(default.half_time(), default.bpm / 2.0);
    assert_eq!(human.double_time(), human.bpm * 2.0);
}

#[test]
fn flat_prior_leans_slow_but_keeps_the_pulse_as_a_candidate() {
    let t = analyze(&clicks(128.0), 1, 22_050, &Config::builder().no_tempo_prior().build().unwrap())
        .unwrap()
        .tempo
        .unwrap();
    assert!((1..=4).any(|k| same_tempo(t.bpm * k as f32, 128.0, 0.01)), "{}", t.bpm);
    assert!(t.candidates.iter().any(|c| same_tempo(c.bpm, 128.0, 0.01)), "{:?}", t.candidates);
}

#[test]
fn same_tempo_counts_half_and_double_time() {
    assert!(same_tempo(87.0, 174.0, 0.04));
    assert!(same_tempo(174.0, 87.0, 0.04));
    assert!(same_tempo(120.0, 123.0, 0.04));
    assert!(!same_tempo(120.0, 126.0, 0.04));
    assert!(!same_tempo(120.0, 180.0, 0.04));
    assert!(!same_tempo(120.0, 480.0, 0.04));
    assert!(!same_tempo(0.0, 0.0, 0.04));
    assert!(!same_tempo(f32::NAN, 120.0, 0.04));
    assert!(!same_tempo(-120.0, -120.0, 0.04));
}

#[test]
fn pitch_class_names_parse_back() {
    for p in PitchClass::ALL {
        assert_eq!(p.name().parse::<PitchClass>().unwrap(), p);
        assert_eq!(PitchClass::from_index(p.index()), Some(p));
    }
    assert_eq!("Db".parse::<PitchClass>().unwrap(), PitchClass::CSharp);
    assert_eq!("G#".parse::<PitchClass>().unwrap(), PitchClass::AFlat);
    assert_eq!("Cb".parse::<PitchClass>().unwrap(), PitchClass::B);
    assert_eq!("B♭".parse::<PitchClass>().unwrap(), PitchClass::BFlat);
    assert_eq!("e".parse::<PitchClass>().unwrap(), PitchClass::E);
    assert!("H".parse::<PitchClass>().is_err());
    assert!("C#x".parse::<PitchClass>().is_err());
    assert_eq!(PitchClass::from_index(12), None);
    assert_eq!(PitchClass::A.transpose(3), PitchClass::C);
    assert_eq!(PitchClass::C.transpose(-1), PitchClass::B);
}

#[test]
fn all_24_keys_round_trip_through_every_notation() {
    let keys: Vec<Key> = Key::all().collect();
    assert_eq!(keys.len(), 24);
    let mut camelot = std::collections::HashSet::new();
    let mut open = std::collections::HashSet::new();
    for k in keys {
        assert_eq!(Key::from_camelot(&k.camelot()).unwrap(), k, "{k}");
        assert_eq!(Key::from_open_key(&k.open_key()).unwrap(), k, "{k}");
        assert_eq!(k.camelot().parse::<Key>().unwrap(), k);
        assert_eq!(k.open_key().parse::<Key>().unwrap(), k);
        assert_eq!(k.name().parse::<Key>().unwrap(), k);
        assert_eq!(k.short_name().parse::<Key>().unwrap(), k);
        assert_eq!(k.to_string(), k.name());
        assert_eq!(k.relative().relative(), k);
        assert_eq!(k.parallel().parallel(), k);
        assert_ne!(k.relative().mode, k.mode);
        assert_eq!(k.relative().camelot_number(), k.camelot_number());
        camelot.insert(k.camelot());
        open.insert(k.open_key());
    }
    assert_eq!((camelot.len(), open.len()), (24, 24));
}

#[test]
fn notation_matches_the_published_wheels() {
    let k = |s: &str| s.parse::<Key>().unwrap();
    for (name, cam, ok) in [
        ("C major", "8B", "1d"),
        ("A minor", "8A", "1m"),
        ("G major", "9B", "2d"),
        ("E minor", "9A", "2m"),
        ("F major", "7B", "12d"),
        ("D minor", "7A", "12m"),
        ("B major", "1B", "6d"),
        ("Ab minor", "1A", "6m"),
        ("F# major", "2B", "7d"),
        ("Eb minor", "2A", "7m"),
        ("Db major", "3B", "8d"),
        ("Bb minor", "3A", "8m"),
    ] {
        assert_eq!((k(name).camelot(), k(name).open_key()), (cam.to_string(), ok.to_string()), "{name}");
    }
    assert_eq!(k("Am").name(), "A minor");
    assert_eq!(k("a min").short_name(), "Am");
    assert_eq!(k("Ebm").name(), "Eb minor");
    assert_eq!(k("8a"), k("A minor"));
    assert_eq!(k("C").relative(), k("Am"));
    assert_eq!(k("Am").relative(), k("C"));
    assert_eq!(k("Am").parallel(), k("A major"));
    for bad in ["", "H", "13A", "0B", "8C", "Am7", "A dorian", "1x"] {
        assert!(matches!(bad.parse::<Key>(), Err(Error::InvalidKey(_))), "{bad:?} parsed");
    }
}

#[test]
fn compatibility_is_same_relative_or_one_step_on_the_wheel() {
    let k = |s: &str| s.parse::<Key>().unwrap();
    let am = k("8A");
    let compatible: Vec<String> = Key::all().filter(|o| am.is_compatible(o)).map(|o| o.camelot()).collect();
    let mut sorted = compatible.clone();
    sorted.sort();
    assert_eq!(sorted, ["7A", "8A", "8B", "9A"]);
    assert!(k("12B").is_compatible(&k("1B")));
    assert!(k("1B").is_compatible(&k("12B")));
    assert!(!k("8A").is_compatible(&k("9B")));
    assert!(!k("8A").is_compatible(&k("10A")));
    for a in Key::all() {
        assert_eq!(Key::all().filter(|b| a.is_compatible(b)).count(), 4);
        for b in Key::all() {
            assert_eq!(a.is_compatible(&b), b.is_compatible(&a));
        }
    }
}

#[test]
fn key_profiles_are_listed_and_named() {
    for p in KeyProfile::ALL {
        assert_eq!(p.to_string(), p.name());
        assert_eq!(p.major()[0], p.major().iter().cloned().fold(f32::MIN, f32::max), "{p}: tonic is the peak");
        assert_eq!(p.minor().len(), 12);
    }
    let msg = "nope".parse::<KeyProfile>().unwrap_err().to_string();
    assert!(msg.contains("bgate"), "{msg}");
}

#[test]
fn errors_display_and_chain() {
    let io = Error::from(std::io::Error::new(std::io::ErrorKind::NotFound, "gone"));
    assert!(std::error::Error::source(&io).is_some());
    assert_eq!(io.to_string(), "cannot read audio: gone");
    assert_eq!(Error::ZeroChannels.to_string(), "channel count is 0");
    assert!(Error::InvalidKey("x".into()).to_string().contains("\"x\""));
}

#[cfg(feature = "serde")]
#[test]
fn results_round_trip_through_serde() {
    use bpm_key::Mode;
    let mut x = clicks(120.0);
    let rate = 22_050.0;
    for (i, v) in x.iter_mut().enumerate() {
        let t = i as f32 / rate;
        *v += [261.63f32, 329.63, 392.0].iter().map(|f| (2.0 * std::f32::consts::PI * f * t).sin() * 0.2).sum::<f32>();
    }
    let f = analyze(&x, 1, 22_050, &Config::default()).unwrap();
    assert!(f.tempo.is_some() && f.key.is_some());
    let json = serde_json::to_string(&f).unwrap();
    let back: bpm_key::Features = serde_json::from_str(&json).unwrap();
    assert_eq!(back, f);
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let order = [
        "duration_secs",
        "tempo",
        "key",
        "tuning_cents",
        "chroma",
        "loudness_lufs",
        "loudness_range_lu",
        "rms_dbfs",
        "spectral_centroid_hz",
        "onset_strength",
    ];
    let positions: Vec<usize> = order.iter().map(|f| json.find(&format!("\"{f}\":")).unwrap()).collect();
    assert!(positions.is_sorted(), "{json}");
    assert_eq!(v.as_object().unwrap().len(), order.len());
    assert_eq!(v["key"]["tonic"], "C");
    assert_eq!(v["key"]["mode"], "major");
    assert_eq!(v["key"]["name"], "C major");
    assert_eq!(v["key"]["camelot"], "8B");
    assert_eq!(v["key"]["open_key"], "1d");
    assert!(v["key"]["margin"].is_number());
    assert_eq!(v["tempo"]["candidates"][0]["weight"], 1.0);

    let k: Key = serde_json::from_str(r#"{"tonic":"F#","mode":"minor"}"#).unwrap();
    assert_eq!(k, Key::new(PitchClass::FSharp, Mode::Minor));
    assert_eq!(serde_json::from_str::<Key>(&serde_json::to_string(&k).unwrap()).unwrap(), k);
    assert!(serde_json::from_str::<Key>(r#"{"tonic":"H","mode":"minor"}"#).is_err());
    assert_eq!(serde_json::to_string(&KeyProfile::Bgate).unwrap(), "\"bgate\"");
}

#[test]
fn skipped_analyses_are_none_and_the_rest_unchanged() {
    use bpm_key::Analysis;
    let mut x = clicks(120.0);
    for (i, v) in x.iter_mut().enumerate() {
        *v += (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 22_050.0).sin() * 0.2;
    }
    let full = analyze(&x, 1, 22_050, &Config::default()).unwrap();

    let tempo_only = Config::builder().only([Analysis::Tempo]).build().unwrap();
    let f = analyze(&x, 1, 22_050, &tempo_only).unwrap();
    assert_eq!(f.tempo, full.tempo);
    assert_eq!((f.duration_secs, f.rms_dbfs), (full.duration_secs, full.rms_dbfs));
    assert!(f.key.is_none() && f.chroma.is_none() && f.tuning_cents.is_none());
    assert!(f.loudness_lufs.is_none() && f.loudness_range_lu.is_none());
    assert!(f.spectral_centroid_hz.is_none() && f.onset_strength.is_none());

    let no_loudness = Config::human_tempo().to_builder().skip(Analysis::Loudness).build().unwrap();
    assert_eq!(no_loudness.tempo_prior(), Config::human_tempo().tempo_prior());
    let f = analyze(&x, 1, 22_050, &no_loudness).unwrap();
    assert!(f.loudness_lufs.is_none());
    assert_eq!((f.key, f.chroma, f.spectral_centroid_hz), (full.key, full.chroma, full.spectral_centroid_hz));

    let nothing = analyze(&x, 1, 22_050, &Config::builder().only([]).build().unwrap()).unwrap();
    assert!(nothing.tempo.is_none() && nothing.key.is_none() && nothing.rms_dbfs.is_some());

    for a in Analysis::ALL {
        assert!(Config::default().runs(a));
        assert_eq!(a.name().parse::<Analysis>().unwrap(), a);
    }
    assert!(matches!("bpm".parse::<Analysis>(), Err(Error::UnknownAnalysis(_))));
}
