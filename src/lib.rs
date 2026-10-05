#![cfg_attr(feature = "decode", doc = include_str!("../README.md"))]
#![cfg_attr(
    not(feature = "decode"),
    doc = "Tempo (BPM) and musical key detection for audio clips. See the README for an overview."
)]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

#[cfg(feature = "decode-core")]
mod decode;
mod key;
mod resample;
mod stft;
mod tempo;

use std::fmt;

pub use key::{Key, KeyEstimate, KeyProfile, Mode, PitchClass};
pub use tempo::{same_tempo, Candidate, Tempo, MAX_BPM, MIN_BPM};

/// Building blocks of the analysis, for custom pipelines.
///
/// These follow the crate's internals closely and **may change between minor versions**. Everything
/// outside this module follows semver.
pub mod advanced {
    pub use crate::key::{best_key, tonal, Tonal};
    pub use crate::resample::{mixdown, resample};
    pub use crate::stft::{stft, Spectrogram};
    pub use crate::tempo::{estimate as estimate_tempo, onset_envelope, Prior};
}

/// Version of the analysis algorithm.
///
/// Bumped whenever a change can alter any output value for the same input and [`Config`], so stored
/// results can be recomputed selectively.
pub const ALGORITHM_VERSION: u32 = 1;

/// Sample rate (Hz) that every clip is mixed down and resampled to before tempo, key and timbre analysis.
pub const ANALYSIS_RATE: u32 = 22_050;

/// Clips whose mean frame RMS is below this (dBFS) are treated as silent.
const SILENCE_DB: f32 = -60.0;

/// Errors returned by this crate.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The channel count was 0.
    ZeroChannels,
    /// The sample rate was 0.
    ZeroSampleRate,
    /// The sample count is not a whole number of frames.
    PartialFrame {
        /// Number of samples given.
        samples: usize,
        /// Channel count given.
        channels: usize,
    },
    /// The tempo prior's centre lies outside [`MIN_BPM`]..=[`MAX_BPM`].
    TempoPriorCenter(f32),
    /// The tempo prior's width is not positive and finite.
    TempoPriorWidth(f32),
    /// No [`KeyProfile`] has this name.
    UnknownKeyProfile(String),
    /// No [`Analysis`] has this name.
    UnknownAnalysis(String),
    /// Text that is not a key or pitch class in any supported notation.
    InvalidKey(String),
    /// The audio could not be probed or decoded.
    Decode(String),
    /// The file could not be read.
    Io(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::ZeroChannels => f.write_str("channel count is 0"),
            Error::ZeroSampleRate => f.write_str("sample rate is 0"),
            Error::PartialFrame { samples, channels } => {
                write!(f, "{samples} samples is not a whole number of {channels}-channel frames")
            }
            Error::TempoPriorCenter(bpm) => write!(f, "tempo prior {bpm} BPM is outside {MIN_BPM}..={MAX_BPM} BPM"),
            Error::TempoPriorWidth(oct) => write!(f, "tempo prior width {oct} octaves must be positive and finite"),
            Error::UnknownKeyProfile(s) => {
                write!(f, "unknown key profile {s:?}; expected one of {}", KeyProfile::ALL.map(|p| p.name()).join(", "))
            }
            Error::UnknownAnalysis(s) => {
                write!(f, "unknown analysis {s:?}; expected one of {}", Analysis::ALL.map(|a| a.name()).join(", "))
            }
            Error::InvalidKey(s) => write!(f, "{s:?} is not a key (e.g. \"A minor\", \"Am\", \"8A\" or \"1m\")"),
            Error::Decode(m) => write!(f, "cannot decode audio: {m}"),
            Error::Io(e) => write!(f, "cannot read audio: {e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Error {
        Error::Io(e)
    }
}

/// A group of [`Features`] fields that can be computed or skipped as a unit.
///
/// `duration_secs` and `rms_dbfs` are always computed: they are cheap, and the RMS level decides whether
/// a clip is silent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Analysis {
    /// `tempo`.
    Tempo,
    /// `key`, `tuning_cents` and `chroma`.
    Key,
    /// `loudness_lufs` and `loudness_range_lu`.
    Loudness,
    /// `spectral_centroid_hz` and `onset_strength`.
    Timbre,
}

impl Analysis {
    /// Every analysis.
    pub const ALL: [Analysis; 4] = [Analysis::Tempo, Analysis::Key, Analysis::Loudness, Analysis::Timbre];

    /// The lowercase name, as accepted by [`str::parse`].
    pub fn name(self) -> &'static str {
        match self {
            Analysis::Tempo => "tempo",
            Analysis::Key => "key",
            Analysis::Loudness => "loudness",
            Analysis::Timbre => "timbre",
        }
    }

    fn bit(self) -> u8 {
        1 << self as u8
    }
}

impl fmt::Display for Analysis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Parses an analysis name, case-insensitively.
impl std::str::FromStr for Analysis {
    type Err = Error;

    fn from_str(s: &str) -> Result<Analysis, Error> {
        Analysis::ALL
            .into_iter()
            .find(|a| a.name().eq_ignore_ascii_case(s.trim()))
            .ok_or_else(|| Error::UnknownAnalysis(s.to_string()))
    }
}

const ALL_ANALYSES: u8 = 0b1111;

/// Analysis settings: which [`Analysis`] groups to run, the tempo prior and the key profile.
///
/// [`Config::default`] runs everything with the best settings measured on popular music: a tempo prior
/// centred on 140 BPM, 1.3 octaves wide, and the [`KeyProfile::Bgate`] key profile.
///
/// ```
/// use bpm_key::{Analysis, Config};
///
/// // Only tempo and key; loudness and timbre are left as `None`.
/// let config = Config::builder().only([Analysis::Tempo, Analysis::Key]).build()?;
/// assert!(config.runs(Analysis::Key));
/// assert!(!config.runs(Analysis::Loudness));
/// # Ok::<(), bpm_key::Error>(())
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Config {
    prior: Option<tempo::Prior>,
    profile: KeyProfile,
    analyses: u8,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            prior: Some(tempo::Prior { bpm: 140.0, octaves: 1.3 }),
            profile: KeyProfile::Bgate,
            analyses: ALL_ANALYSES,
        }
    }
}

impl Config {
    /// A builder starting from [`Config::default`].
    pub fn builder() -> ConfigBuilder {
        Config::default().to_builder()
    }

    /// A builder starting from this config, for adjusting a preset:
    /// `Config::human_tempo().to_builder().skip(Analysis::Loudness).build()`.
    pub fn to_builder(&self) -> ConfigBuilder {
        let Config { prior, profile, analyses } = *self;
        ConfigBuilder { prior, profile, analyses }
    }

    /// Reports tempo the way people tap along: a prior centred on 100 BPM, 1.3 octaves wide, with
    /// [`KeyProfile::Bgate`].
    ///
    /// The default prior reports most songs under about 100 BPM at double time. This preset counts them
    /// at the slow tempo, as human annotators do, but tends to halve songs above about 150 BPM.
    pub fn human_tempo() -> Config {
        Config { prior: Some(tempo::Prior { bpm: 100.0, octaves: 1.3 }), ..Config::default() }
    }

    /// Whether this config runs `analysis`.
    pub fn runs(&self, analysis: Analysis) -> bool {
        self.analyses & analysis.bit() != 0
    }

    /// The tempo prior's centre (BPM) and width (octaves), or `None` for a flat prior.
    pub fn tempo_prior(&self) -> Option<(f32, f32)> {
        self.prior.map(|p| (p.bpm, p.octaves))
    }

    /// The key profile.
    pub fn key_profile(&self) -> KeyProfile {
        self.profile
    }
}

/// Builds a validated [`Config`]. Created by [`Config::builder`] or [`Config::to_builder`].
#[derive(Clone, Copy, Debug)]
#[must_use]
pub struct ConfigBuilder {
    prior: Option<tempo::Prior>,
    profile: KeyProfile,
    analyses: u8,
}

impl ConfigBuilder {
    /// Runs only these analyses. The fields of the others are left as `None`, which saves their
    /// computation.
    pub fn only(mut self, analyses: impl IntoIterator<Item = Analysis>) -> Self {
        self.analyses = analyses.into_iter().fold(0, |mask, a| mask | a.bit());
        self
    }

    /// Skips `analysis`, leaving its fields as `None`.
    pub fn skip(mut self, analysis: Analysis) -> Self {
        self.analyses &= !analysis.bit();
        self
    }

    /// A log-normal tempo prior centred on `center` BPM ([`MIN_BPM`]..=[`MAX_BPM`]) with a standard
    /// deviation of `octaves`. Wider priors leave the metrical level more to the audio.
    pub fn tempo_prior(mut self, center: f32, octaves: f32) -> Self {
        self.prior = Some(tempo::Prior { bpm: center, octaves });
        self
    }

    /// No tempo prior: every tempo from [`MIN_BPM`] to [`MAX_BPM`] is equally likely.
    ///
    /// Slow readings then tend to win (a 128 BPM pulse may come out as 42.7, a third of it), because the
    /// beats also land on every multiple of a slow period. The real pulse usually stays among the
    /// [`Tempo::candidates`].
    pub fn no_tempo_prior(mut self) -> Self {
        self.prior = None;
        self
    }

    /// The key profile.
    pub fn key_profile(mut self, profile: KeyProfile) -> Self {
        self.profile = profile;
        self
    }

    /// Validates the settings.
    ///
    /// # Errors
    ///
    /// [`Error::TempoPriorCenter`] or [`Error::TempoPriorWidth`] for an out-of-range tempo prior.
    pub fn build(self) -> Result<Config, Error> {
        if let Some(p) = self.prior {
            if !(MIN_BPM..=MAX_BPM).contains(&p.bpm) {
                return Err(Error::TempoPriorCenter(p.bpm));
            }
            if !(p.octaves.is_finite() && p.octaves > 0.0) {
                return Err(Error::TempoPriorWidth(p.octaves));
            }
        }
        Ok(Config { prior: self.prior, profile: self.profile, analyses: self.analyses })
    }
}

/// The analysis of one clip.
///
/// Every field except `duration_secs` is `None` when the clip is silent, too short or contains
/// non-finite samples, and fields of an [`Analysis`] the [`Config`] skips are always `None`.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct Features {
    /// Clip length in seconds.
    pub duration_secs: f32,
    /// Global tempo.
    pub tempo: Option<Tempo>,
    /// Key under the configured [`KeyProfile`].
    pub key: Option<KeyEstimate>,
    /// Deviation of the clip's tuning from A440, in cents (-50..50).
    pub tuning_cents: Option<f32>,
    /// Mean chroma, scaled so the strongest pitch class is 1. Index 0 is C, 1 is C#, and so on.
    pub chroma: Option<[f32; 12]>,
    /// EBU R128 integrated loudness in LUFS, measured on the channels as given.
    pub loudness_lufs: Option<f32>,
    /// EBU R128 loudness range in LU, measured on the channels as given.
    pub loudness_range_lu: Option<f32>,
    /// Mean frame RMS level in dBFS.
    pub rms_dbfs: Option<f32>,
    /// Magnitude-weighted mean spectral centroid in Hz: a rough measure of brightness.
    pub spectral_centroid_hz: Option<f32>,
    /// Mean onset strength: a rough measure of rhythmic busyness, comparable between clips.
    pub onset_strength: Option<f32>,
}

/// Analyses interleaved samples with `channels` channels at `rate` Hz.
///
/// Loudness is measured on all channels at the original rate, everything else on a mono mix resampled
/// to [`ANALYSIS_RATE`]. Mono loudness is single-channel R128, about 3 LU below the same signal given as
/// two identical channels.
///
/// # Errors
///
/// [`Error::ZeroChannels`], [`Error::ZeroSampleRate`] or [`Error::PartialFrame`] when the samples
/// cannot be split into frames. Audio that merely has nothing to measure is not an error; see
/// [`Features`].
pub fn analyze(samples: &[f32], channels: usize, rate: u32, cfg: &Config) -> Result<Features, Error> {
    if channels == 0 {
        return Err(Error::ZeroChannels);
    }
    if rate == 0 {
        return Err(Error::ZeroSampleRate);
    }
    if samples.len() % channels != 0 {
        return Err(Error::PartialFrame { samples: samples.len(), channels });
    }
    let duration_secs = (samples.len() / channels) as f32 / rate as f32;
    let features = if samples.iter().all(|v| v.is_finite()) {
        analyze_valid(samples, channels, rate, cfg)
    } else {
        Features::default()
    };
    Ok(Features { duration_secs, ..features })
}

/// Decodes and analyses an encoded clip held in memory. The format is detected from the content.
///
/// Supports the formats enabled by cargo features: MP3, WAV, FLAC, Ogg Vorbis and AAC/M4A with the
/// default `decode` feature. A truncated file is analysed up to where it breaks off.
///
/// # Errors
///
/// [`Error::Decode`] when the data is not audio in an enabled format.
#[cfg(feature = "decode-core")]
pub fn analyze_bytes(bytes: impl Into<Vec<u8>>, cfg: &Config) -> Result<Features, Error> {
    let pcm = decode::decode_bytes(bytes.into())?;
    analyze(&pcm.interleaved, pcm.channels, pcm.rate, cfg)
}

/// Decodes and analyses an audio file. See [`analyze_bytes`] for the supported formats.
///
/// # Errors
///
/// [`Error::Io`] when the file cannot be read, [`Error::Decode`] when it is not audio in an enabled
/// format.
#[cfg(feature = "decode-core")]
pub fn analyze_file(path: impl AsRef<std::path::Path>, cfg: &Config) -> Result<Features, Error> {
    let path = path.as_ref();
    let file = std::fs::File::open(path)?;
    let pcm = decode::decode(Box::new(file), path.extension().and_then(|e| e.to_str()))?;
    analyze(&pcm.interleaved, pcm.channels, pcm.rate, cfg)
}

fn analyze_valid(interleaved: &[f32], channels: usize, rate: u32, cfg: &Config) -> Features {
    let mono = if channels == 1 { interleaved.to_vec() } else { resample::mixdown(interleaved, channels) };
    let mono = resample::resample(&mono, rate, ANALYSIS_RATE);
    let rms_dbfs = rms_dbfs(&mono);
    if !rms_dbfs.is_some_and(|db| db >= SILENCE_DB) {
        return Features::default();
    }
    let mut f = Features { rms_dbfs, ..Features::default() };
    let sr = ANALYSIS_RATE as f32;

    if cfg.runs(Analysis::Loudness) {
        (f.loudness_lufs, f.loudness_range_lu) = loudness(interleaved, channels, rate);
    }
    if cfg.runs(Analysis::Tempo) || cfg.runs(Analysis::Timbre) {
        let rhythm = stft::stft(&mono, sr, 1024, 256);
        let env = tempo::onset_envelope(&rhythm);
        if cfg.runs(Analysis::Tempo) {
            f.tempo = tempo::estimate(&env, rhythm.fps(), cfg.prior);
        }
        if cfg.runs(Analysis::Timbre) {
            f.spectral_centroid_hz = spectral_centroid(&rhythm);
            f.onset_strength = (!env.is_empty()).then(|| env.iter().sum::<f32>() / env.len() as f32);
        }
    }
    if cfg.runs(Analysis::Key) {
        if let Some(tonal) = key::tonal(&stft::stft(&mono, sr, 8192, 2048)) {
            f.key = key::best_key(&tonal.chroma, cfg.profile.major(), cfg.profile.minor());
            f.tuning_cents = Some(tonal.tuning_cents);
            f.chroma = Some(tonal.chroma);
        }
    }
    f
}

fn spectral_centroid(spec: &stft::Spectrogram) -> Option<f32> {
    let (mut num, mut den) = (0f64, 0f64);
    for t in 0..spec.frames {
        for (k, &m) in spec.frame(t).iter().enumerate() {
            num += (spec.bin_hz(k as f32) * m) as f64;
            den += m as f64;
        }
    }
    (den > 0.0).then(|| (num / den) as f32)
}

fn rms_dbfs(mono: &[f32]) -> Option<f32> {
    if mono.is_empty() {
        return None;
    }
    let frames = mono.chunks(1024).map(|c| (c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32).sqrt());
    let mean = frames.sum::<f32>() / mono.len().div_ceil(1024) as f32;
    Some(20.0 * (mean + 1e-9).log10())
}

fn loudness(interleaved: &[f32], channels: usize, rate: u32) -> (Option<f32>, Option<f32>) {
    use ebur128::{EbuR128, Mode};
    let Ok(mut meter) = EbuR128::new(channels as u32, rate, Mode::I | Mode::LRA) else {
        return (None, None);
    };
    if meter.add_frames_f32(interleaved).is_err() {
        return (None, None);
    }
    let finite = |v: Result<f64, _>| v.ok().filter(|x: &f64| x.is_finite()).map(|x| x as f32);
    (finite(meter.loudness_global()), finite(meter.loudness_range()))
}
