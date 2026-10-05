# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the crate follows
[Semantic Versioning](https://semver.org/).

## [0.1.0] - 2026-10-04

First public release, with `ALGORITHM_VERSION` 1.

### Analysis

- `analyze` takes interleaved `f32` samples at any rate and channel count and returns `Features`: tempo
  with candidates, key with a margin, tuning, chroma, EBU R128 loudness and loudness range, RMS level,
  spectral centroid and onset strength.
- `analyze_bytes` and `analyze_file` decode MP3, WAV, FLAC, Ogg Vorbis and AAC/M4A through Symphonia. Each format has its own cargo feature.

### Configuration

- `Config::default()` (tempo prior 140 BPM / 1.3 octaves, `bgate` key profile), the
  `Config::human_tempo()` preset, and a builder with a validated tempo prior (or none) and a choice of key
  profile.
- `ConfigBuilder::only` and `skip` choose which groups of results (`Analysis::Tempo`, `Key`, `Loudness`,
  `Timbre`) are computed. `Config::to_builder` adjusts an existing config or preset.

### Key notation and tempo helpers

- `Key`, `PitchClass` and `Mode` parse and print names, short names, Camelot and Open Key notation, and
  provide relative and parallel keys, harmonic compatibility and a compact `code()`.
- `same_tempo` compares tempi up to half and double time.

### Key profiles

- Krumhansl, Temperley, `edma`, `bgate` and Sha'ath, with their published values and sources.

### Other

- `bpm_key::advanced` exposes the building blocks: STFT, onset envelope, tempo estimate, chroma, key correlation and resampling.
- An optional `serde` feature. A serialized key includes its name, Camelot and Open Key notation.
- An optional `cli` feature, which builds the `bpm-key` binary. It prints one JSON line per file, in
  the same shape as the `serde` output, and takes `--threads`, `--prior`, `--human-tempo`,
  `--profile` and `--only`.
