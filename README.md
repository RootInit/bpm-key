# bpm-key

[![crates.io](https://img.shields.io/crates/v/bpm-key.svg)](https://crates.io/crates/bpm-key)
[![docs.rs](https://img.shields.io/docsrs/bpm-key)](https://docs.rs/bpm-key)
![license](https://img.shields.io/crates/l/bpm-key.svg)

Tempo (BPM) and musical key detection for audio clips, in pure Rust.

Give `bpm-key` a song, or a 30 second preview of one, and it tells you how fast it is and what key it's
in. Along the way it also measures tuning, chroma, EBU R128 loudness, RMS level, brightness and rhythmic
busyness. There are no C libraries, no machine learning models and no `unsafe` code; MP3, WAV, FLAC, Ogg
Vorbis and AAC/M4A are decoded with [Symphonia](https://crates.io/crates/symphonia).

## Usage

```toml
[dependencies]
bpm-key = "0.1"
```

```rust,no_run
use bpm_key::{analyze_file, Config};

fn main() -> Result<(), bpm_key::Error> {
    let features = analyze_file("song.mp3", &Config::default())?;

    if let Some(tempo) = &features.tempo {
        println!("{:.1} BPM", tempo.bpm);
    }
    if let Some(key) = &features.key {
        println!("{} ({}), confidence {:.2}", key.key, key.key.camelot(), key.margin);
    }
    Ok(())
}
```

If the audio is already in memory, use `analyze_bytes` for an encoded file, or `analyze` for decoded
samples (interleaved `f32`, any sample rate and channel count).

A clip with nothing to measure, such as silence, a fraction of a second of audio or samples that are
`NaN`, still analyses successfully: the fields come back as `None` rather than as a made-up 0 BPM or
C major. Only input that can't be read at all is an `Error`.

### What you get back

| field | meaning |
|---|---|
| `duration_secs` | length of the clip |
| `tempo` | `bpm`, a `confidence` from 0 to 1, and up to three `candidates` (usually including the half- or double-time reading) |
| `key` | the `Key`, its correlation `score`, and the `margin` over the runner-up, which works as a confidence |
| `tuning_cents` | how far the clip's tuning is from A440 |
| `chroma` | relative strength of the 12 pitch classes, starting from C, with the strongest at 1 |
| `loudness_lufs`, `loudness_range_lu` | EBU R128 integrated loudness and loudness range |
| `rms_dbfs` | mean RMS level |
| `spectral_centroid_hz` | a rough measure of brightness |
| `onset_strength` | a rough measure of rhythmic busyness |

With the `serde` feature all result types serialize. A key is written with its derived notations, so
a result looks like this (numbers rounded):

```json
{
  "duration_secs": 30.0,
  "tempo": {
    "bpm": 140.61,
    "confidence": 0.52,
    "candidates": [
      { "bpm": 140.61, "weight": 1.0 },
      { "bpm": 93.96, "weight": 0.75 },
      { "bpm": 70.31, "weight": 0.72 }
    ]
  },
  "key": {
    "tonic": "E",
    "mode": "major",
    "name": "E major",
    "camelot": "12B",
    "open_key": "5d",
    "score": 0.75,
    "margin": 0.035
  },
  "tuning_cents": 5.67,
  "chroma": [0.67, 0.83, 0.63, 0.75, 1.0, 0.62, 0.78, 0.73, 0.84, 0.85, 0.49, 0.71],
  "loudness_lufs": -13.88,
  "loudness_range_lu": 3.32,
  "rms_dbfs": -18.77,
  "spectral_centroid_hz": 2138.55,
  "onset_strength": 0.0195
}
```

## Command line

```sh
cargo install bpm-key --features cli
```

```sh
bpm-key song.mp3 other.flac       # one JSON line per file, in the order given
cat clip.m4a | bpm-key -          # read from stdin
bpm-key --threads 8 --profile temperley --prior 120,1.0 *.mp3
bpm-key --human-tempo *.mp3       # count slow songs the way people tap along
bpm-key --only tempo,key *.mp3    # skip loudness and timbre
```

Each line is the JSON shown above plus `file` and `algorithm_version`. A file that can't be read prints
`{"file": …, "error": …}` instead, and the exit status becomes 1.

## Half time or double time?

A song at 87 BPM is also, quite reasonably, a song at 174 BPM. The detector finds the pulse reliably, and
a *tempo prior* then decides which of these metrical levels to report.

The default prior is centred on 140 BPM and reports most songs under about 100 BPM at double time, which
is how Deezer's catalogue counts them. `Config::human_tempo()` centres it on 100 BPM instead and reports
slow songs the way people tap along, at the cost of halving songs above about 150 BPM. Either way, the
other reading is usually in `tempo.candidates`, and `same_tempo` compares tempi up to a factor of two.

## Configuration

`Config::builder()` starts from the defaults and lets you change any of them:

```rust
use bpm_key::{Analysis, Config, KeyProfile};

let config = Config::builder()
    .tempo_prior(120.0, 1.0) // centre in BPM, width in octaves
    .key_profile(KeyProfile::Temperley)
    .only([Analysis::Tempo, Analysis::Key])
    .build()
    .expect("prior is in range");
```

- **Tempo prior.** `tempo_prior(center, octaves)` sets your own, and `no_tempo_prior()` removes it, which
  tends to favour slow readings.
- **Key profile.** This is what the detector expects a major and a minor key to sound like. The default,
  `Bgate`, measured best on popular music; `Krumhansl`, `Temperley`, `Edma` and `Shaath` are there too.
- **What to compute.** If you only need some of the results, `only([...])` or `skip(...)` leaves the
  rest out and saves their work. The groups are `Analysis::Tempo`, `Key` (key, tuning and chroma),
  `Loudness` and `Timbre` (spectral centroid and onset strength). Skipped fields come back as `None`;
  the ones you keep are identical to a full run. Every group shares the decoding and resampling step, so
  asking for tempo alone saves about a third of the time rather than most of it.

To tweak a preset, turn it back into a builder:
`Config::human_tempo().to_builder().skip(Analysis::Loudness).build()`.

## Key notation

`Key` parses and prints the usual notations and knows which keys mix well together:

```rust
use bpm_key::{same_tempo, Key};

let am: Key = "Am".parse().unwrap(); // or "A minor", "8A" (Camelot), "1m" (Open Key)
assert_eq!(am.camelot(), "8A");
assert_eq!(am.open_key(), "1m");
assert_eq!(am.relative().to_string(), "C major");
assert!(am.is_compatible(&"9A".parse().unwrap())); // same key, relative, or one step on the wheel

assert!(same_tempo(87.0, 174.0, 0.04));
```

## Accuracy

Measured with the default config on 30 second previews of popular music, counting a tempo as correct
within ±4 %.

**Tempo.** On 530 songs with full-track human annotations (Harmonix, Isophonics, Billboard), the tempo
is right up to a factor of two or three for 96 % of songs, and at exactly the annotated level for 68 %.
Across all 2,182 human-labelled songs those figures are 90 % and 58 %. The gap is mostly the default
prior reporting slow songs at double time. On a held-out half of the human-labelled songs,
`Config::human_tempo()` raised the exact-level figure from 61 % to about 70 % and lowered the other from
90 % to 89 %; these two figures were estimated offline from the detector's scores rather than measured
with the preset.

**Key.** The MIREX-weighted score against 937 human key labels is 0.64 (54 % exactly right), and 0.74
(65 %) on the 107 Isophonics songs that never change key. For comparison, Spotify's own key labels score
0.53 on 848 of the same songs. The most common mistake is naming the fifth above the real key.

**Speed.** About 0.15 CPU-seconds per 30 second MP3 clip including decoding, or 5–7 clips per second
per core.

## Limitations

- It is tuned and evaluated on 30 second clips of Western popular music. Whole tracks work, but a key
  change is averaged into a single key.
- Tempo is one global value. Songs with tempo changes or rubato get their dominant periodicity.
- The key model assumes major or minor tonality. Modal, atonal, drone or purely percussive music gets a
  key with a low margin, or none.

## How it works

Everything except loudness is computed on a mono mix resampled to 22,050 Hz.

- **Tempo** follows `librosa.beat.tempo` (D. P. W. Ellis, "Beat Tracking by Dynamic Programming",
  *J. New Music Research* 36(1), 2007): log-magnitude spectral flux, its autocorrelation, a comb over
  the first four multiples of each lag at sub-frame resolution, and a log-normal prior.
- **Key** uses a harmonic pitch class profile built from tuning-corrected spectral peaks (E. Gómez,
  *Tonal Description of Music Audio Signals*, PhD thesis, UPF, 2006), averaged over the clip and
  correlated with the 24 rotations of a key profile. The `KeyProfile` docs cite each profile's source.
- **Loudness** is EBU R128, via the [`ebur128`](https://crates.io/crates/ebur128) crate.

The building blocks are public in `bpm_key::advanced` for custom pipelines, but that module may change
between minor versions. `ALGORITHM_VERSION` goes up whenever any result can change for the same input
and config, so stored results can be recomputed selectively.

## Cargo features

| feature | default | what it enables |
|---|---|---|
| `decode` | yes | `analyze_bytes` and `analyze_file`, with every format below |
| `mp3`, `wav`, `flac`, `ogg`, `aac` | via `decode` | one format each; `ogg` is Ogg Vorbis, `aac` is AAC in MP4/M4A |
| `serde` | no | `Serialize` and `Deserialize` for the result types |
| `cli` | no | the `bpm-key` binary |

To decode only MP3, use `default-features = false, features = ["mp3"]`. With no decoding features at
all, only `analyze` is available.

The minimum supported Rust version is 1.86.

## License

Licensed under either of [Apache License, Version 2.0](https://www.apache.org/licenses/LICENSE-2.0)
or [MIT license](https://opensource.org/licenses/MIT), at your option.
