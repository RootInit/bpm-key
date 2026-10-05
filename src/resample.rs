//! Mono mixdown and sample-rate conversion.

/// Mean of the channels of interleaved samples. Panics if `channels` is 0.
pub fn mixdown(interleaved: &[f32], channels: usize) -> Vec<f32> {
    let c = channels;
    interleaved.chunks_exact(c).map(|f| f.iter().sum::<f32>() / c as f32).collect()
}

/// Windowed-sinc (Blackman) resampler with a precomputed polyphase kernel table; the
/// cutoff sits a little below the lower Nyquist. Returns the input unchanged when the rates are equal.
pub fn resample(x: &[f32], from: u32, to: u32) -> Vec<f32> {
    use std::f64::consts::PI;
    if from == to || x.is_empty() {
        return x.to_vec();
    }
    const PHASES: usize = 256;
    let ratio = from as f64 / to as f64;
    let cutoff = 0.95 * (to.min(from) as f64 / from as f64);
    let half = 16.0 * ratio.max(1.0);
    let reach = half.ceil() as isize;
    let width = (2 * reach + 1) as usize;
    let mut table = vec![0f32; PHASES * width];
    for p in 0..PHASES {
        let frac = p as f64 / PHASES as f64;
        let row = &mut table[p * width..(p + 1) * width];
        let mut sum = 0.0;
        for (k, slot) in row.iter_mut().enumerate() {
            let d = (k as isize - reach) as f64 - frac;
            if d.abs() >= half {
                continue;
            }
            let arg = PI * d * cutoff;
            let sinc = if arg.abs() < 1e-9 { 1.0 } else { arg.sin() / arg };
            let w = 0.42 + 0.5 * (PI * d / half).cos() + 0.08 * (2.0 * PI * d / half).cos();
            *slot = (sinc * w) as f32;
            sum += sinc * w;
        }
        row.iter_mut().for_each(|v| *v /= sum as f32);
    }
    let n_out = (x.len() as f64 / ratio) as usize;
    let mut y = Vec::with_capacity(n_out);
    for i in 0..n_out {
        let t = i as f64 * ratio;
        let base = t.floor();
        let mut p = ((t - base) * PHASES as f64).round() as usize;
        let mut base = base as isize;
        if p == PHASES {
            p = 0;
            base += 1;
        }
        let row = &table[p * width..(p + 1) * width];
        let start = base - reach;
        let mut acc = 0f32;
        if start >= 0 && (start as usize + width) <= x.len() {
            let xs = &x[start as usize..start as usize + width];
            acc = xs.iter().zip(row).map(|(a, b)| a * b).sum();
        } else {
            for (k, &h) in row.iter().enumerate() {
                let j = start + k as isize;
                if j >= 0 && (j as usize) < x.len() {
                    acc += x[j as usize] * h;
                }
            }
        }
        y.push(acc);
    }
    y
}
