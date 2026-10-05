//! Decoding through Symphonia, on WAV and FLAC files generated here (no audio ships with the crate).

use std::f32::consts::PI;
use std::path::PathBuf;

use bpm_key::same_tempo;
use bpm_key::{analyze, analyze_bytes, analyze_file, Config, Error, Features, Key};

/// An A minor triad over a 120 BPM click, `secs` long, as interleaved samples with identical channels.
fn music(rate: u32, channels: usize, secs: f32) -> Vec<f32> {
    let n = (secs * rate as f32) as usize;
    let period = (0.5 * rate as f32) as usize;
    let mut out = Vec::with_capacity(n * channels);
    for i in 0..n {
        let t = i as f32 / rate as f32;
        let tone: f32 = [220.0f32, 261.63, 329.63, 110.0].iter().map(|f| (2.0 * PI * f * t).sin() * 0.12).sum();
        let j = i % period;
        let click = if j < 400 {
            (-(j as f32) / 60.0).exp() * ((j as f32 * 1.7).sin() + (j as f32 * 0.31).sin()) * 0.4
        } else {
            0.0
        };
        out.extend(std::iter::repeat_n(tone + click, channels));
    }
    out
}

fn assert_a_minor_120(f: &Features) {
    let tempo = f.tempo.as_ref().expect("tempo");
    assert!(same_tempo(tempo.bpm, 120.0, 0.01), "tempo {}", tempo.bpm);
    assert_eq!(f.key.expect("key").key, "Am".parse::<Key>().unwrap());
}

fn wav(samples: &[f32], channels: u16, rate: u32, float: bool) -> Vec<u8> {
    let bytes_per = if float { 4 } else { 2 };
    let data_len = (samples.len() * bytes_per) as u32;
    let mut w = Vec::new();
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data_len).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&(if float { 3u16 } else { 1u16 }).to_le_bytes());
    w.extend_from_slice(&channels.to_le_bytes());
    w.extend_from_slice(&rate.to_le_bytes());
    w.extend_from_slice(&(rate * channels as u32 * bytes_per as u32).to_le_bytes());
    w.extend_from_slice(&(channels * bytes_per as u16).to_le_bytes());
    w.extend_from_slice(&(bytes_per as u16 * 8).to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&data_len.to_le_bytes());
    for &s in samples {
        if float {
            w.extend_from_slice(&s.to_le_bytes());
        } else {
            w.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes());
        }
    }
    w
}

fn crc8(data: &[u8]) -> u8 {
    data.iter().fold(0u8, |mut crc, &b| {
        crc ^= b;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 { (crc << 1) ^ 0x07 } else { crc << 1 };
        }
        crc
    })
}

fn crc16(data: &[u8]) -> u16 {
    data.iter().fold(0u16, |mut crc, &b| {
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 { (crc << 1) ^ 0x8005 } else { crc << 1 };
        }
        crc
    })
}

/// A 16-bit FLAC stream with fixed-size blocks of VERBATIM subframes (valid, just uncompressed).
fn flac(samples: &[f32], channels: usize, rate: u32) -> Vec<u8> {
    const BLOCK: usize = 16_384;
    let frames = samples.len() / channels;
    assert!(frames.div_ceil(BLOCK) < 128, "frame numbers must fit one UTF-8 byte");
    let pcm: Vec<i16> = samples.iter().map(|s| (s.clamp(-1.0, 1.0) * 32767.0).round() as i16).collect();
    let mut f = b"fLaC".to_vec();
    f.extend_from_slice(&[0x80, 0, 0, 34]); // last metadata block, STREAMINFO, 34 bytes
    f.extend_from_slice(&(BLOCK as u16).to_be_bytes());
    f.extend_from_slice(&(BLOCK as u16).to_be_bytes());
    f.extend_from_slice(&[0; 6]); // frame sizes unknown
    let packed: u64 = (rate as u64) << 44 | ((channels as u64 - 1) << 41) | (15u64 << 36) | frames as u64;
    f.extend_from_slice(&packed.to_be_bytes());
    f.extend_from_slice(&[0; 16]); // no MD5
    for (n, start) in (0..frames).step_by(BLOCK).enumerate() {
        let len = BLOCK.min(frames - start);
        let mut fr = vec![0xFF, 0xF8, 0x70, ((channels as u8 - 1) << 4) | 0x08, n as u8];
        fr.extend_from_slice(&(len as u16 - 1).to_be_bytes());
        fr.push(crc8(&fr));
        for c in 0..channels {
            fr.push(0x02); // VERBATIM, no wasted bits
            for i in start..start + len {
                fr.extend_from_slice(&pcm[i * channels + c].to_be_bytes());
            }
        }
        let crc = crc16(&fr);
        fr.extend_from_slice(&crc.to_be_bytes());
        f.extend_from_slice(&fr);
    }
    f
}

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bpm-key-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

#[test]
fn float_wav_decodes_to_exactly_the_samples() {
    let x = music(44_100, 2, 12.0);
    let direct = analyze(&x, 2, 44_100, &Config::default()).unwrap();
    let decoded = analyze_bytes(wav(&x, 2, 44_100, true), &Config::default()).unwrap();
    assert_eq!(decoded, direct);
    assert_a_minor_120(&decoded);
}

#[test]
fn pcm16_wav_file_is_analysed() {
    let path = tmp("a.wav");
    std::fs::write(&path, wav(&music(48_000, 1, 12.0), 1, 48_000, false)).unwrap();
    let f = analyze_file(&path, &Config::default()).unwrap();
    assert!((f.duration_secs - 12.0).abs() < 0.01, "{}", f.duration_secs);
    assert_a_minor_120(&f);
}

#[test]
fn flac_is_analysed_like_the_same_pcm_in_wav() {
    let x = music(44_100, 2, 12.0);
    let from_flac = analyze_bytes(flac(&x, 2, 44_100), &Config::default()).unwrap();
    let from_wav = analyze_bytes(wav(&x, 2, 44_100, false), &Config::default()).unwrap();
    assert_eq!(from_flac, from_wav);
    assert_a_minor_120(&from_flac);
    let path = tmp("a.flac");
    std::fs::write(&path, flac(&x, 2, 44_100)).unwrap();
    assert_eq!(analyze_file(&path, &Config::default()).unwrap(), from_flac);
}

#[test]
fn garbage_and_missing_input_are_errors() {
    assert!(matches!(analyze_bytes(Vec::new(), &Config::default()), Err(Error::Decode(_))));
    let junk = b"definitely not an audio file".repeat(100);
    assert!(matches!(analyze_bytes(junk.as_slice(), &Config::default()), Err(Error::Decode(_))));
    let e = analyze_file(tmp("missing.mp3"), &Config::default()).unwrap_err();
    assert!(matches!(&e, Error::Io(io) if io.kind() == std::io::ErrorKind::NotFound), "{e:?}");
}

#[test]
fn truncated_wav_keeps_what_decoded() {
    let x = music(22_050, 1, 12.0);
    let mut bytes = wav(&x, 1, 22_050, false);
    bytes.truncate(bytes.len() / 2 + 1);
    let f = analyze_bytes(bytes, &Config::default()).unwrap();
    assert!((f.duration_secs - 6.0).abs() < 0.1, "{}", f.duration_secs);
}

/// MP3, Ogg Vorbis and AAC/M4A need an encoder; this runs only where `ffmpeg` is installed.
#[test]
fn compressed_formats_via_ffmpeg_when_available() {
    if std::process::Command::new("ffmpeg").arg("-version").output().is_err() {
        eprintln!("ffmpeg not found; skipping");
        return;
    }
    let src = tmp("src.wav");
    std::fs::write(&src, wav(&music(44_100, 2, 12.0), 2, 44_100, false)).unwrap();
    for (ext, codec) in [("mp3", "libmp3lame"), ("ogg", "libvorbis"), ("m4a", "aac")] {
        let out = tmp(&format!("enc.{ext}"));
        let ok = std::process::Command::new("ffmpeg")
            .args(["-loglevel", "error", "-y", "-i"])
            .arg(&src)
            .args(["-c:a", codec, "-b:a", "128k"])
            .arg(&out)
            .status()
            .is_ok_and(|s| s.success());
        if !ok {
            eprintln!("ffmpeg has no {codec} encoder; skipping {ext}");
            continue;
        }
        let f = analyze_file(&out, &Config::default()).unwrap_or_else(|e| panic!("{ext}: {e}"));
        assert_a_minor_120(&f);
        let g = analyze_bytes(std::fs::read(&out).unwrap(), &Config::default()).unwrap();
        assert_eq!(g, f, "{ext}: bytes and file decode alike");
    }
}
