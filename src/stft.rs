//! Short-time Fourier transform.

use realfft::RealFftPlanner;

/// A magnitude spectrogram.
#[derive(Clone, Debug)]
pub struct Spectrogram {
    /// Magnitudes, row-major: `frames * bins`.
    pub mag: Vec<f32>,
    /// Bins per frame, `n_fft / 2 + 1`.
    pub bins: usize,
    /// Number of frames.
    pub frames: usize,
    /// FFT length in samples.
    pub n_fft: usize,
    /// Hop between frames in samples.
    pub hop: usize,
    /// Sample rate in Hz.
    pub sr: f32,
}

impl Spectrogram {
    /// The magnitudes of frame `t`.
    pub fn frame(&self, t: usize) -> &[f32] {
        &self.mag[t * self.bins..(t + 1) * self.bins]
    }
    /// The frequency in Hz of (possibly fractional) bin `k`.
    pub fn bin_hz(&self, k: f32) -> f32 {
        k * self.sr / self.n_fft as f32
    }
    /// Frames per second.
    pub fn fps(&self) -> f32 {
        self.sr / self.hop as f32
    }
}

/// The magnitude spectrogram of `x` with a Hann window of `n_fft` samples, advanced by `hop`.
///
/// Magnitudes are scaled so a full-scale sine peaks near 1.0. The first frame starts at sample 0 with
/// no padding, and a partial frame at the end is dropped.
pub fn stft(x: &[f32], sr: f32, n_fft: usize, hop: usize) -> Spectrogram {
    let bins = n_fft / 2 + 1;
    let window: Vec<f32> =
        (0..n_fft).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n_fft as f32).cos()).collect();
    let scale = 2.0 / window.iter().sum::<f32>();
    let fft = RealFftPlanner::<f32>::new().plan_fft_forward(n_fft);
    let mut input = fft.make_input_vec();
    let mut output = fft.make_output_vec();
    let mut scratch = fft.make_scratch_vec();

    let frames = if x.len() >= n_fft { (x.len() - n_fft) / hop + 1 } else { 0 };
    let mut mag = Vec::with_capacity(frames * bins);
    for t in 0..frames {
        let seg = &x[t * hop..t * hop + n_fft];
        for ((dst, &s), &w) in input.iter_mut().zip(seg).zip(&window) {
            *dst = s * w;
        }
        fft.process_with_scratch(&mut input, &mut output, &mut scratch).expect("fft sizes match");
        mag.extend(output.iter().map(|c| c.norm() * scale));
    }
    Spectrogram { mag, bins, frames, n_fft, hop, sr }
}
