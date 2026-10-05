//! Tempo detection, in the style of librosa's `beat.tempo`.

use crate::stft::Spectrogram;

/// Slowest tempo the detector reports, in BPM.
pub const MIN_BPM: f32 = 40.0;
/// Fastest tempo the detector reports, in BPM.
pub const MAX_BPM: f32 = 240.0;
const HARMONICS: usize = 4;
/// Lag resolution in frames: about 0.03 BPM at 120 BPM and 86 frames/s.
const LAG_STEP: f32 = 0.01;

/// One candidate period of the onset pattern.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct Candidate {
    /// Tempo in BPM.
    pub bpm: f32,
    /// Prior-weighted autocorrelation relative to the best candidate, so 1 for the first candidate and
    /// less for the others.
    pub weight: f32,
}

/// A clip's global tempo.
///
/// The detector picks one metrical level, but a beat at 87 BPM is equally a beat at 174: the
/// [`candidates`](Tempo::candidates) usually contain the other levels, and [`same_tempo`] compares tempi
/// up to that ambiguity.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct Tempo {
    /// Tempo in BPM, 40..=240.
    pub bpm: f32,
    /// Normalised autocorrelation at the chosen period (0..1): how periodic the onsets are.
    pub confidence: f32,
    /// Up to three distinct periods, best first; the first is `bpm`.
    pub candidates: Vec<Candidate>,
}

impl Tempo {
    /// Half of [`bpm`](Tempo::bpm): the reading a listener tapping every other beat would give.
    pub fn half_time(&self) -> f32 {
        self.bpm / 2.0
    }

    /// Twice [`bpm`](Tempo::bpm).
    pub fn double_time(&self) -> f32 {
        self.bpm * 2.0
    }

    /// The most likely alternative reading: a candidate at half or double [`bpm`](Tempo::bpm) (within
    /// 4 %), if the detector found one.
    pub fn octave_alternative(&self) -> Option<&Candidate> {
        self.candidates.iter().skip(1).find(|c| {
            let r = c.bpm / self.bpm;
            (r / 2.0 - 1.0).abs() <= 0.04 || (r * 2.0 - 1.0).abs() <= 0.04
        })
    }
}

/// Whether two tempi are the same within a relative `tolerance` (`0.04` for ±4 %), counting half and
/// double time as the same tempo.
///
/// Non-finite or non-positive tempi are never the same as anything.
///
/// ```
/// use bpm_key::same_tempo;
///
/// assert!(same_tempo(87.0, 174.0, 0.04));
/// assert!(same_tempo(120.0, 123.0, 0.04));
/// assert!(!same_tempo(120.0, 180.0, 0.04));
/// ```
pub fn same_tempo(a: f32, b: f32, tolerance: f32) -> bool {
    if !(a.is_finite() && b.is_finite() && a > 0.0 && b > 0.0) {
        return false;
    }
    [1.0, 2.0, 0.5].iter().any(|k| ((a * k) / b - 1.0).abs() <= tolerance)
}

/// Log-normal tempo prior: centre in BPM and standard deviation in octaves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Prior {
    /// Centre of the prior, in BPM.
    pub bpm: f32,
    /// Standard deviation of the prior, in octaves.
    pub octaves: f32,
}

/// Onset strength per frame: mean half-wave-rectified difference of log-compressed magnitude,
/// with a local mean subtracted so only the pulse remains.
pub fn onset_envelope(spec: &Spectrogram) -> Vec<f32> {
    let max_bin = ((11_000.0 / spec.sr * spec.n_fft as f32) as usize).min(spec.bins);
    let log: Vec<f32> = spec.mag.iter().map(|&m| (1.0 + 1000.0 * m).ln()).collect();
    let mut flux = vec![0f32; spec.frames];
    for t in 1..spec.frames {
        let (prev, cur) = (&log[(t - 1) * spec.bins..], &log[t * spec.bins..]);
        flux[t] = (0..max_bin).map(|k| (cur[k] - prev[k]).max(0.0)).sum::<f32>() / max_bin as f32;
    }
    let half = (0.25 * spec.fps()) as usize;
    let mut env = vec![0f32; flux.len()];
    for t in 0..flux.len() {
        let (lo, hi) = (t.saturating_sub(half), (t + half + 1).min(flux.len()));
        let mean = flux[lo..hi].iter().sum::<f32>() / (hi - lo) as f32;
        env[t] = (flux[t] - mean).max(0.0);
    }
    env
}

fn autocorr(x: &[f32], max_lag: usize) -> Vec<f32> {
    let n = x.len();
    (0..=max_lag.min(n.saturating_sub(1)))
        .map(|l| x[..n - l].iter().zip(&x[l..]).map(|(a, b)| a * b).sum::<f32>() / (n - l) as f32)
        .collect()
}

fn interp(r: &[f32], lag: f32) -> f32 {
    let i = lag.floor() as usize;
    if i + 1 >= r.len() {
        return 0.0;
    }
    let f = lag - i as f32;
    r[i] * (1.0 - f) + r[i + 1] * f
}

fn prior_weight(p: Option<Prior>, bpm: f32) -> f32 {
    let Some(p) = p else { return 1.0 };
    let z = (bpm / p.bpm).log2() / p.octaves;
    (-0.5 * z * z).exp()
}

/// Tempo of an onset envelope sampled at `fps` frames per second.
///
/// `prior` weights the candidate periods; `None` weights them all equally. Returns `None` when the
/// envelope is too short (under about four of the slowest beat periods) or has no energy.
pub fn estimate(env: &[f32], fps: f32, prior: Option<Prior>) -> Option<Tempo> {
    let lag_of = |bpm: f32| 60.0 * fps / bpm;
    let (min_lag, max_lag) = (lag_of(MAX_BPM), lag_of(MIN_BPM));
    let r = autocorr(env, (HARMONICS as f32 * max_lag).ceil() as usize + 2);
    if (r.len() as f32) < HARMONICS as f32 * max_lag + 1.0 || r[0] <= 0.0 {
        return None;
    }

    // A true beat period also correlates at 2x..4x its lag, while a 3:2 or 4:3 cross-rhythm
    // does not; the prior then settles the remaining period-doubling ambiguity.
    let comb = |lag: f32| (1..=HARMONICS).map(|k| interp(&r, k as f32 * lag).max(0.0)).sum::<f32>() / HARMONICS as f32;
    let steps = ((max_lag - min_lag) / LAG_STEP) as usize + 1;
    let lags: Vec<f32> = (0..steps).map(|i| min_lag + i as f32 * LAG_STEP).collect();
    let score: Vec<f32> = lags.iter().map(|&l| comb(l) * prior_weight(prior, 60.0 * fps / l)).collect();

    let mut order: Vec<usize> =
        (1..steps - 1).filter(|&i| score[i] >= score[i - 1] && score[i] >= score[i + 1]).collect();
    order.sort_by(|&a, &b| score[b].total_cmp(&score[a]));
    let mut picks: Vec<usize> = Vec::new();
    for i in order {
        if score[i] <= 0.0 || picks.len() == 3 {
            break;
        }
        if picks.iter().all(|&p| (lags[p] / lags[i]).ln().abs() > 0.04) {
            picks.push(i);
        }
    }
    let &best = picks.first()?;
    Some(Tempo {
        bpm: lag_of(lags[best]),
        confidence: (comb(lags[best]) / r[0]).clamp(0.0, 1.0),
        candidates: picks.iter().map(|&i| Candidate { bpm: lag_of(lags[i]), weight: score[i] / score[best] }).collect(),
    })
}
