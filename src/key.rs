//! Key detection and the key notation types.

use std::fmt;
use std::str::FromStr;

use crate::stft::Spectrogram;
use crate::Error;

const MIN_HZ: f32 = 50.0;
const MAX_HZ: f32 = 3500.0;
/// Peaks quieter than this fraction of the frame's loudest bin are ignored (-40 dB).
const PEAK_FLOOR: f32 = 0.01;

/// One of the twelve pitch classes, `C = 0` up to `B = 11`.
///
/// Names use the spelling most common in popular-music key labels: `C#`, `Eb`, `F#`, `Ab`, `Bb`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
#[allow(missing_docs)]
pub enum PitchClass {
    C = 0,
    CSharp,
    D,
    EFlat,
    E,
    F,
    FSharp,
    G,
    AFlat,
    A,
    BFlat,
    B,
}

const PITCH_NAMES: [&str; 12] = ["C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"];

impl PitchClass {
    /// All twelve pitch classes in ascending order from C.
    pub const ALL: [PitchClass; 12] = [
        PitchClass::C,
        PitchClass::CSharp,
        PitchClass::D,
        PitchClass::EFlat,
        PitchClass::E,
        PitchClass::F,
        PitchClass::FSharp,
        PitchClass::G,
        PitchClass::AFlat,
        PitchClass::A,
        PitchClass::BFlat,
        PitchClass::B,
    ];

    /// The pitch class with this index (0 = C); `None` above 11.
    pub fn from_index(index: u8) -> Option<PitchClass> {
        PitchClass::ALL.get(index as usize).copied()
    }

    /// Semitones above C, 0..=11.
    pub fn index(self) -> u8 {
        self as u8
    }

    /// The name, e.g. `"C#"` or `"Bb"`.
    pub fn name(self) -> &'static str {
        PITCH_NAMES[self as usize]
    }

    /// The pitch class `semitones` above this one, wrapping around the octave.
    pub fn transpose(self, semitones: i32) -> PitchClass {
        PitchClass::ALL[(self as i32 + semitones).rem_euclid(12) as usize]
    }
}

impl fmt::Display for PitchClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Parses a letter A–G followed by any number of `#`/`♯` or `b`/`♭`, e.g. `"F#"`, `"Db"`, `"e"`.
impl FromStr for PitchClass {
    type Err = Error;

    fn from_str(s: &str) -> Result<PitchClass, Error> {
        let bad = || Error::InvalidKey(s.to_string());
        let mut chars = s.trim().chars();
        let base: i32 = match chars.next().map(|c| c.to_ascii_uppercase()) {
            Some('C') => 0,
            Some('D') => 2,
            Some('E') => 4,
            Some('F') => 5,
            Some('G') => 7,
            Some('A') => 9,
            Some('B') => 11,
            _ => return Err(bad()),
        };
        let mut shift = 0;
        for c in chars {
            match c {
                '#' | '♯' => shift += 1,
                'b' | '♭' => shift -= 1,
                _ => return Err(bad()),
            }
        }
        Ok(PitchClass::C.transpose(base + shift))
    }
}

/// Major or minor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize), serde(rename_all = "lowercase"))]
pub enum Mode {
    /// Major (ionian).
    Major,
    /// Minor (aeolian).
    Minor,
}

/// A musical key: a tonic and a mode, with name, Camelot and Open Key notations.
///
/// ```
/// use bpm_key::{Key, Mode, PitchClass};
///
/// let a_minor = Key::new(PitchClass::A, Mode::Minor);
/// assert_eq!(a_minor.name(), "A minor");
/// assert_eq!(a_minor.short_name(), "Am");
/// assert_eq!(a_minor.camelot(), "8A");
/// assert_eq!(a_minor.open_key(), "1m");
/// assert_eq!(a_minor.relative(), "C".parse::<Key>().unwrap());
/// assert!(a_minor.is_compatible(&"8B".parse().unwrap()));
/// ```
///
/// With the `serde` feature, a key serializes as `tonic` and `mode` plus the derived `name`, `camelot`
/// and `open_key` strings. Only `tonic` and `mode` are read back.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Key {
    /// The tonic (home note).
    pub tonic: PitchClass,
    /// Major or minor.
    pub mode: Mode,
}

impl Key {
    /// The key with this tonic and mode.
    pub fn new(tonic: PitchClass, mode: Mode) -> Key {
        Key { tonic, mode }
    }

    /// All 24 keys in [`code`](Key::code) order: C major, C minor, C# major, … B minor.
    pub fn all() -> impl Iterator<Item = Key> {
        (0..24).map(|c| Key::from_code(c).expect("codes 0..24 are keys"))
    }

    /// A compact numbering, `tonic * 2 + minor`: 0 = C major, 1 = C minor, …, 19 = A minor, 23 = B minor.
    pub fn code(self) -> u8 {
        self.tonic.index() * 2 + (self.mode == Mode::Minor) as u8
    }

    /// The key with this [`code`](Key::code); `None` above 23.
    pub fn from_code(code: u8) -> Option<Key> {
        let tonic = PitchClass::from_index(code / 2)?;
        Some(Key::new(tonic, if code % 2 == 1 { Mode::Minor } else { Mode::Major }))
    }

    /// `true` for minor keys.
    pub fn is_minor(self) -> bool {
        self.mode == Mode::Minor
    }

    /// The full name, e.g. `"A minor"` or `"Eb major"`.
    pub fn name(self) -> String {
        format!("{} {}", self.tonic, if self.is_minor() { "minor" } else { "major" })
    }

    /// The short name: the tonic, plus `m` for minor, e.g. `"Am"` or `"Eb"`.
    pub fn short_name(self) -> String {
        format!("{}{}", self.tonic, if self.is_minor() { "m" } else { "" })
    }

    /// Position on the circle of fifths of the relative major, 0 = C major / A minor, 1 = G / Em, ….
    fn fifths(self) -> u8 {
        let major = if self.is_minor() { self.tonic.transpose(3) } else { self.tonic };
        major.index() * 7 % 12
    }

    fn from_fifths(fifths: u8, mode: Mode) -> Key {
        let major = PitchClass::C.transpose(fifths as i32 * 7);
        Key::new(if mode == Mode::Minor { major.transpose(-3) } else { major }, mode)
    }

    /// The Camelot wheel number, 1..=12 (8 = C major / A minor).
    pub fn camelot_number(self) -> u8 {
        (self.fifths() + 7) % 12 + 1
    }

    /// Camelot notation: number and `A` (minor) or `B` (major), e.g. `"8A"` for A minor.
    pub fn camelot(self) -> String {
        format!("{}{}", self.camelot_number(), if self.is_minor() { 'A' } else { 'B' })
    }

    /// Open Key notation: number and `m` (minor) or `d` (major), e.g. `"1m"` for A minor, `"1d"` for C major.
    pub fn open_key(self) -> String {
        format!("{}{}", self.fifths() + 1, if self.is_minor() { 'm' } else { 'd' })
    }

    /// The key from Camelot notation (`"8A"`, case-insensitive).
    pub fn from_camelot(s: &str) -> Result<Key, Error> {
        let (n, mode) = split_wheel(s, 'a', 'b').ok_or_else(|| Error::InvalidKey(s.to_string()))?;
        Ok(Key::from_fifths((n + 12 - 8) % 12, mode))
    }

    /// The key from Open Key notation (`"1m"`, `"12d"`, case-insensitive).
    pub fn from_open_key(s: &str) -> Result<Key, Error> {
        let (n, mode) = split_wheel(s, 'm', 'd').ok_or_else(|| Error::InvalidKey(s.to_string()))?;
        Ok(Key::from_fifths(n - 1, mode))
    }

    /// The relative major of a minor key, or the relative minor of a major key (A minor ↔ C major).
    pub fn relative(self) -> Key {
        match self.mode {
            Mode::Minor => Key::new(self.tonic.transpose(3), Mode::Major),
            Mode::Major => Key::new(self.tonic.transpose(-3), Mode::Minor),
        }
    }

    /// The other mode on the same tonic (A minor ↔ A major).
    pub fn parallel(self) -> Key {
        Key::new(self.tonic, if self.is_minor() { Mode::Major } else { Mode::Minor })
    }

    /// Whether two keys mix harmonically by the usual DJ rule: the same key, the relative key, or one step
    /// either way on the Camelot wheel in the same mode (a fifth up or down).
    pub fn is_compatible(self, other: &Key) -> bool {
        if self.mode != other.mode {
            return self.relative() == *other;
        }
        let d = (self.fifths() + 12 - other.fifths()) % 12;
        d == 0 || d == 1 || d == 11
    }
}

/// Splits `"8A"` into (8, minor) given the minor and major letters.
fn split_wheel(s: &str, minor: char, major: char) -> Option<(u8, Mode)> {
    let s = s.trim();
    let letter = s.chars().last()?.to_ascii_lowercase();
    let mode = if letter == minor {
        Mode::Minor
    } else if letter == major {
        Mode::Major
    } else {
        return None;
    };
    let n: u8 = s[..s.len() - 1].parse().ok()?;
    (1..=12).contains(&n).then_some((n, mode))
}

impl fmt::Display for Key {
    /// Writes the full name, e.g. `A minor`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name())
    }
}

/// Parses a full name (`"A minor"`, `"Eb major"`, `"F# min"`), a short name (`"Am"`, `"C"`, `"Bbm"`),
/// Camelot (`"8A"`) or Open Key (`"1m"`) notation.
impl FromStr for Key {
    type Err = Error;

    fn from_str(s: &str) -> Result<Key, Error> {
        let t = s.trim();
        let bad = || Error::InvalidKey(s.to_string());
        if t.starts_with(|c: char| c.is_ascii_digit()) {
            return match t.chars().last().map(|c| c.to_ascii_lowercase()) {
                Some('a' | 'b') => Key::from_camelot(t),
                Some('m' | 'd') => Key::from_open_key(t),
                _ => Err(bad()),
            };
        }
        let split = t.find(|c: char| c.is_whitespace()).unwrap_or(t.len());
        let (head, tail) = t.split_at(split);
        let tail = tail.trim().to_ascii_lowercase();
        let (tonic, mode) = if !tail.is_empty() {
            let mode = match tail.as_str() {
                "major" | "maj" => Mode::Major,
                "minor" | "min" => Mode::Minor,
                _ => return Err(bad()),
            };
            (head, mode)
        } else if let Some(stem) = head.strip_suffix('m') {
            (stem, Mode::Minor)
        } else {
            (head, Mode::Major)
        };
        Ok(Key::new(tonic.parse().map_err(|_| bad())?, mode))
    }
}

/// A detected key with its correlation score and margin over the runner-up.
///
/// With the `serde` feature, the fields of [`Key`] are serialized inline next to `score` and `margin`:
/// `{"tonic": "A", "mode": "minor", "name": "A minor", "camelot": "8A", "open_key": "1m", "score": …,
/// "margin": …}`.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct KeyEstimate {
    /// The best of the 24 keys.
    pub key: Key,
    /// Pearson correlation of the clip's chroma with the winning rotated profile (-1..1).
    pub score: f32,
    /// Winning score minus the best of the other 23 keys. This is the confidence measure: on
    /// human-labelled clips, accuracy rises with the margin.
    pub margin: f32,
}

/// A major and a minor key profile: the expected weight of each pitch class relative to the tonic.
///
/// Each profile's numbers are as published. Á. Faraldo's thesis (*Tonality Estimation in Electronic
/// Dance Music: A Computational and Musically Informed Examination*, PhD thesis, Universitat Pompeu
/// Fabra, 2017, <http://hdl.handle.net/10803/463079>) prints all five to two decimals; it is the source
/// cited below as "Faraldo 2017".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize), serde(rename_all = "lowercase"))]
#[non_exhaustive]
pub enum KeyProfile {
    /// Probe-tone ratings of listeners: C. L. Krumhansl and E. J. Kessler, "Tracing the dynamic changes
    /// in perceived tonal organization in a spatial representation of musical keys", *Psychological
    /// Review* 89(4), 334–368, 1982; also C. L. Krumhansl, *Cognitive Foundations of Musical Pitch*,
    /// Oxford University Press, 1990. Values as printed in Faraldo 2017, Fig. 3.1.
    Krumhansl,
    /// Krumhansl-Kessler revised for key finding from scores: D. Temperley, "What's key for key? The
    /// Krumhansl-Schmuckler key-finding algorithm reconsidered", *Music Perception* 17(1), 65–100, 1999.
    /// Values as printed in Faraldo 2017, Fig. 3.5.
    Temperley,
    /// Derived from electronic dance music: Á. Faraldo, E. Gómez, S. Jordà and P. Herrera, "Key
    /// Estimation in Electronic Dance Music", *ECIR 2016*, LNCS 9626, 335–347, scaled so the tonic is 1.
    /// Values from Faraldo 2017, Fig. 6.5.
    Edma,
    /// The default and the best measured on popular music: profiles derived from the Beatport dataset with
    /// the non-diatonic degrees set to zero. Á. Faraldo, S. Jordà and P. Herrera, "A Multi-Profile
    /// Method for Key Estimation in EDM", *AES International Conference on Semantic Audio*, 2017;
    /// values from Faraldo 2017, Fig. 6.12 ("bgate").
    Bgate,
    /// Krumhansl-Kessler adjusted for electronic music: I. Sha'ath, *Estimation of Key in Digital Music
    /// Recordings*, MSc project report, Birkbeck College, University of London, 2011. Values as printed
    /// in Faraldo 2017, Fig. 3.12.
    Shaath,
}

/// Major and minor profiles indexed from the tonic, as published (see [`KeyProfile`]).
const PROFILES: [(&str, [f32; 12], [f32; 12]); 5] = [
    (
        "krumhansl",
        [6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88],
        [6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17],
    ),
    (
        "temperley",
        [5.0, 2.0, 3.5, 2.0, 4.5, 4.0, 2.0, 4.5, 2.0, 3.5, 1.5, 4.0],
        [5.0, 2.0, 3.5, 4.5, 2.0, 4.0, 2.0, 4.5, 3.5, 2.0, 1.5, 4.0],
    ),
    (
        "edma",
        [1.00, 0.29, 0.50, 0.40, 0.60, 0.56, 0.32, 0.80, 0.32, 0.45, 0.42, 0.39],
        [1.00, 0.31, 0.44, 0.58, 0.33, 0.49, 0.29, 0.78, 0.43, 0.29, 0.53, 0.32],
    ),
    (
        "bgate",
        [1.00, 0.00, 0.42, 0.00, 0.53, 0.37, 0.00, 0.77, 0.00, 0.38, 0.21, 0.30],
        [1.00, 0.00, 0.36, 0.39, 0.00, 0.38, 0.00, 0.74, 0.27, 0.00, 0.42, 0.23],
    ),
    (
        "shaath",
        [6.6, 2.0, 3.5, 2.3, 4.6, 4.0, 2.5, 5.2, 2.4, 3.7, 2.3, 3.4],
        [6.5, 2.7, 3.5, 5.4, 2.6, 3.5, 2.5, 5.2, 4.0, 2.7, 4.3, 3.2],
    ),
];

impl KeyProfile {
    /// Every profile.
    pub const ALL: [KeyProfile; 5] =
        [KeyProfile::Krumhansl, KeyProfile::Temperley, KeyProfile::Edma, KeyProfile::Bgate, KeyProfile::Shaath];

    /// The lowercase name, as accepted by [`str::parse`].
    pub fn name(self) -> &'static str {
        PROFILES[self as usize].0
    }

    /// The major profile, index 0 = tonic.
    pub fn major(self) -> &'static [f32; 12] {
        &PROFILES[self as usize].1
    }

    /// The minor profile, index 0 = tonic.
    pub fn minor(self) -> &'static [f32; 12] {
        &PROFILES[self as usize].2
    }
}

impl fmt::Display for KeyProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Parses a profile name, case-insensitively.
impl FromStr for KeyProfile {
    type Err = Error;

    fn from_str(s: &str) -> Result<KeyProfile, Error> {
        KeyProfile::ALL
            .into_iter()
            .find(|p| p.name().eq_ignore_ascii_case(s))
            .ok_or_else(|| Error::UnknownKeyProfile(s.to_string()))
    }
}

/// Clip-level pitch content, independent of the key profile.
#[derive(Clone, Debug, PartialEq)]
pub struct Tonal {
    /// Deviation of the clip's tuning from A440, in cents (-50..50).
    pub tuning_cents: f32,
    /// Mean chroma, max-normalised, index 0 = C.
    pub chroma: [f32; 12],
}

/// Spectral peaks of one frame as (frequency Hz, magnitude), with parabolic interpolation.
fn peaks(spec: &Spectrogram, t: usize, out: &mut Vec<(f32, f32)>) {
    out.clear();
    let m = spec.frame(t);
    let lo = ((MIN_HZ / spec.bin_hz(1.0)) as usize).max(1);
    let hi = ((MAX_HZ / spec.bin_hz(1.0)) as usize).min(spec.bins - 2);
    let floor = m[lo..=hi].iter().fold(0f32, |a, &b| a.max(b)) * PEAK_FLOOR;
    for k in lo..=hi {
        let (a, b, c) = (m[k - 1], m[k], m[k + 1]);
        if b > floor && b > a && b >= c {
            let (la, lb, lc) = ((a + 1e-12).ln(), (b + 1e-12).ln(), (c + 1e-12).ln());
            let denom = la - 2.0 * lb + lc;
            let shift = if denom.abs() > 1e-12 { (0.5 * (la - lc) / denom).clamp(-0.5, 0.5) } else { 0.0 };
            let amp = (lb - 0.25 * (la - lc) * shift).exp();
            out.push((spec.bin_hz(k as f32 + shift), amp));
        }
    }
}

fn semitones_from_a440(hz: f32) -> f32 {
    12.0 * (hz / 440.0).log2()
}

fn pearson(a: &[f32; 12], b: impl Fn(usize) -> f32) -> f32 {
    let ma = a.iter().sum::<f32>() / 12.0;
    let mb = (0..12).map(&b).sum::<f32>() / 12.0;
    let (mut num, mut va, mut vb) = (0.0, 0.0, 0.0);
    for (i, &ai) in a.iter().enumerate() {
        let (da, db) = (ai - ma, b(i) - mb);
        num += da * db;
        va += da * da;
        vb += db * db;
    }
    if va <= 0.0 || vb <= 0.0 {
        0.0
    } else {
        num / (va * vb).sqrt()
    }
}

/// Tuning and mean chroma of a spectrogram (the crate uses an 8192-point FFT with hop 2048 at 22 050 Hz).
/// `None` when no spectral peak is found.
pub fn tonal(spec: &Spectrogram) -> Option<Tonal> {
    let mut frame_peaks: Vec<Vec<(f32, f32)>> = Vec::with_capacity(spec.frames);
    let mut buf = Vec::new();
    for t in 0..spec.frames {
        peaks(spec, t, &mut buf);
        frame_peaks.push(buf.clone());
    }

    // Tuning: magnitude-weighted circular mean of each peak's offset from the 12-TET grid.
    let (mut re, mut im) = (0f64, 0f64);
    for &(hz, amp) in frame_peaks.iter().flatten() {
        let phase = 2.0 * std::f64::consts::PI * semitones_from_a440(hz) as f64;
        re += amp as f64 * phase.cos();
        im += amp as f64 * phase.sin();
    }
    if re == 0.0 && im == 0.0 {
        return None;
    }
    let tuning = (im.atan2(re) / (2.0 * std::f64::consts::PI)) as f32; // in semitones, -0.5..0.5

    let mut chroma = [0f32; 12];
    let mut used = 0usize;
    for fp in &frame_peaks {
        let mut c = [0f32; 12];
        for &(hz, amp) in fp {
            let pc = (semitones_from_a440(hz) - tuning + 9.0).rem_euclid(12.0);
            let nearest = pc.round();
            let d = pc - nearest;
            let w = (std::f32::consts::PI * d).cos().powi(2);
            c[(nearest as usize) % 12] += w * amp;
        }
        let max = c.iter().fold(0f32, |a, &b| a.max(b));
        if max > 0.0 {
            c.iter().zip(chroma.iter_mut()).for_each(|(v, acc)| *acc += v / max);
            used += 1;
        }
    }
    if used == 0 {
        return None;
    }
    let max = chroma.iter().fold(0f32, |a, &b| a.max(b));
    chroma.iter_mut().for_each(|v| *v /= max);

    Some(Tonal { tuning_cents: 100.0 * tuning, chroma })
}

/// Correlates the chroma with all 24 rotations of `(major, minor)`; `None` when nothing correlates.
pub fn best_key(chroma: &[f32; 12], major: &[f32; 12], minor: &[f32; 12]) -> Option<KeyEstimate> {
    let mut scores: Vec<(f32, u8, bool)> = Vec::with_capacity(24);
    for tonic in 0..12 {
        scores.push((pearson(chroma, |i| major[(i + 12 - tonic) % 12]), tonic as u8, false));
        scores.push((pearson(chroma, |i| minor[(i + 12 - tonic) % 12]), tonic as u8, true));
    }
    scores.sort_by(|a, b| b.0.total_cmp(&a.0));
    let (score, tonic, minor) = scores[0];
    // pearson returns 0 for a flat chroma, so this also rejects featureless input.
    if score.is_nan() || score <= 0.0 {
        return None;
    }
    let key = Key::new(PitchClass::ALL[tonic as usize], if minor { Mode::Minor } else { Mode::Major });
    Some(KeyEstimate { key, score, margin: score - scores[1].0 })
}

#[cfg(feature = "serde")]
mod serde_impls {
    use super::{Key, KeyEstimate, Mode, PitchClass};
    use serde::ser::SerializeStruct;
    use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

    impl Serialize for PitchClass {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            s.serialize_str(self.name())
        }
    }

    impl<'de> Deserialize<'de> for PitchClass {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            let s = <std::borrow::Cow<'de, str>>::deserialize(d)?;
            s.parse().map_err(de::Error::custom)
        }
    }

    fn key_fields<S: SerializeStruct>(st: &mut S, k: &Key) -> Result<(), S::Error> {
        st.serialize_field("tonic", &k.tonic)?;
        st.serialize_field("mode", &k.mode)?;
        st.serialize_field("name", &k.name())?;
        st.serialize_field("camelot", &k.camelot())?;
        st.serialize_field("open_key", &k.open_key())
    }

    impl Serialize for Key {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            let mut st = s.serialize_struct("Key", 5)?;
            key_fields(&mut st, self)?;
            st.end()
        }
    }

    impl Serialize for KeyEstimate {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            let mut st = s.serialize_struct("KeyEstimate", 7)?;
            key_fields(&mut st, &self.key)?;
            st.serialize_field("score", &self.score)?;
            st.serialize_field("margin", &self.margin)?;
            st.end()
        }
    }

    #[derive(Deserialize)]
    struct KeyRepr {
        tonic: PitchClass,
        mode: Mode,
    }

    #[derive(Deserialize)]
    struct KeyEstimateRepr {
        tonic: PitchClass,
        mode: Mode,
        score: f32,
        margin: f32,
    }

    impl<'de> Deserialize<'de> for Key {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            let r = KeyRepr::deserialize(d)?;
            Ok(Key::new(r.tonic, r.mode))
        }
    }

    impl<'de> Deserialize<'de> for KeyEstimate {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            let r = KeyEstimateRepr::deserialize(d)?;
            Ok(KeyEstimate { key: Key::new(r.tonic, r.mode), score: r.score, margin: r.margin })
        }
    }
}
