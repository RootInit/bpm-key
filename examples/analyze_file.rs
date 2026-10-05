//! Prints tempo and key for each file given on the command line.
//!
//! `cargo run --example analyze_file -- song.mp3 other.flac`

use bpm_key::{analyze_file, Config};

fn main() {
    let cfg = Config::default();
    for path in std::env::args().skip(1) {
        match analyze_file(&path, &cfg) {
            Err(e) => eprintln!("{path}: {e}"),
            Ok(f) => {
                let tempo = f.tempo.map_or("no tempo".to_string(), |t| {
                    let alt = t.octave_alternative().map_or(String::new(), |c| format!(" (or {:.1})", c.bpm));
                    format!("{:.1} BPM{alt}", t.bpm)
                });
                let key = f.key.map_or("no key".to_string(), |k| {
                    format!("{} / {} / {}, margin {:.3}", k.key.name(), k.key.camelot(), k.key.open_key(), k.margin)
                });
                println!("{path}: {tempo}; {key}; {:.1} LUFS", f.loudness_lufs.unwrap_or(f32::NAN));
            }
        }
    }
}
