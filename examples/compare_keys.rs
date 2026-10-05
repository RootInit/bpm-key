//! Converts keys between notations and checks harmonic compatibility for DJ-style mixing.
//!
//! `cargo run --example compare_keys -- Am 8B "F# minor" 4d`

use bpm_key::{same_tempo, Key};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args = if args.is_empty() { ["Am", "8B", "Em", "F# minor"].map(String::from).to_vec() } else { args };
    let keys: Vec<Key> = args.iter().filter_map(|a| a.parse().map_err(|e| eprintln!("{e}")).ok()).collect();
    for k in &keys {
        println!(
            "{:<10} {:<4} Camelot {:<3} Open Key {:<3} relative {}",
            k.name(),
            k.short_name(),
            k.camelot(),
            k.open_key(),
            k.relative().short_name()
        );
    }
    for (i, a) in keys.iter().enumerate() {
        for b in &keys[i + 1..] {
            let verdict = if a.is_compatible(b) { "mixes" } else { "clashes" };
            println!("{} -> {}: {verdict}", a.camelot(), b.camelot());
        }
    }
    println!("87 and 174 BPM are the same tempo up to octave: {}", same_tempo(87.0, 174.0, 0.04));
}
