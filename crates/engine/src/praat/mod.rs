//! Praat-compatible acoustic analysis.
//!
//! Ports of the algorithms in Praat (P. Boersma & D. Weenink, University of
//! Amsterdam, <https://github.com/praat/praat.github.io>, GPL-3.0-or-later):
//! pitch (autocorrelation + Viterbi path finder), glottal pulses (cc),
//! jitter / shimmer / harmonicity (Voice report), intensity, formants (Burg)
//! and CPPS. Same defaults as Praat so values are comparable with Praat and
//! with the clinical literature that uses it. Control thread only.
//!
//! Conventions follow Praat's `Sampled`: sample `i` (0-based) sits at time
//! `(i + 0.5) / sr`.
pub mod cepstrum;
pub mod formant;
pub mod intensity;
pub mod pitch;
pub mod pulses;
pub mod voice;

use std::f64::consts::PI;

/// A mono sound in f64 (Praat computes in double precision).
#[derive(Clone)]
pub struct Sound {
    pub x: Vec<f64>,
    pub sr: f64,
}

impl Sound {
    pub fn new(x: &[f32], sr: f32) -> Sound {
        Sound { x: x.iter().map(|&v| v as f64).collect(), sr: sr as f64 }
    }
    pub fn dx(&self) -> f64 {
        1.0 / self.sr
    }
    pub fn duration(&self) -> f64 {
        self.x.len() as f64 / self.sr
    }
    /// Time of 0-based sample `i`.
    #[inline]
    pub fn t(&self, i: isize) -> f64 {
        (i as f64 + 0.5) / self.sr
    }
    /// Real (0-based) index of time `t`.
    #[inline]
    pub fn index(&self, t: f64) -> f64 {
        t * self.sr - 0.5
    }
    pub fn low_index(&self, t: f64) -> isize {
        self.index(t).floor() as isize
    }
    pub fn high_index(&self, t: f64) -> isize {
        self.index(t).ceil() as isize
    }
    pub fn nearest_index(&self, t: f64) -> isize {
        self.index(t).round() as isize
    }
    #[inline]
    pub fn at(&self, i: isize) -> f64 {
        if i < 0 || i as usize >= self.x.len() { 0.0 } else { self.x[i as usize] }
    }
    /// Praat's `Sampled_getWindowSamples`: indices strictly inside [tmin, tmax].
    pub fn window_samples(&self, tmin: f64, tmax: f64) -> (isize, isize) {
        let a = self.high_index(tmin).max(0);
        let b = self.low_index(tmax).min(self.x.len() as isize - 1);
        (a, b)
    }

    /// First-order pre-emphasis above `freq` Hz (in place, as Praat).
    pub fn pre_emphasize(&mut self, freq: f64) {
        let a = (-2.0 * PI * freq / self.sr).exp();
        for i in (1..self.x.len()).rev() {
            self.x[i] -= a * self.x[i - 1];
        }
    }

    /// Praat `Sound_resample (…, 50)`: FFT anti-aliasing filter (with 1000
    /// samples of zero padding each side) when downsampling, then sinc
    /// interpolation of depth 50. Very long sounds (FFT > 2^23 points) use
    /// a streaming windowed-sinc low-pass instead, to bound memory.
    pub fn resample(&self, new_sr: f64) -> Sound {
        let up = new_sr / self.sr;
        if (up - 1.0).abs() < 1e-6 {
            return self.clone();
        }
        let dur = self.duration();
        let n_out = (dur * new_sr).round() as usize;
        let mut src = self.x.clone();
        if up < 1.0 {
            const PAD: usize = 1000;
            let nfft = (self.x.len() + 2 * PAD).next_power_of_two();
            if nfft > 1 << 23 {
                return self.resample_streaming(new_sr);
            }
            let fft = Fft64::new(nfft);
            let mut re = vec![0.0; nfft];
            let mut im = vec![0.0; nfft];
            re[PAD..PAD + self.x.len()].copy_from_slice(&self.x);
            fft.forward(&mut re, &mut im);
            // Praat zeroes packed-real indices ≥ floor(up·nfft) (1-based:
            // re(k) at 2k+1, im(k) at 2k+2) and the Nyquist bin.
            let cut = (up * nfft as f64).floor() as usize;
            for k in 1..nfft / 2 {
                if 2 * k + 1 >= cut {
                    re[k] = 0.0;
                    re[nfft - k] = 0.0;
                }
                if 2 * k + 2 >= cut {
                    im[k] = 0.0;
                    im[nfft - k] = 0.0;
                }
            }
            re[nfft / 2] = 0.0;
            im[nfft / 2] = 0.0;
            // inverse = conj · forward · conj / n
            for v in im.iter_mut() {
                *v = -*v;
            }
            fft.forward(&mut re, &mut im);
            for (i, v) in src.iter_mut().enumerate() {
                *v = re[i + PAD] / nfft as f64;
            }
        }
        let x1 = 0.5 * (dur - (n_out as f64 - 1.0) / new_sr);
        let y = (0..n_out)
            .map(|j| {
                let t = x1 + j as f64 / new_sr;
                praat_sinc(&src, t * self.sr + 0.5, 50) // 1-based index as in Praat
            })
            .collect();
        Sound { x: y, sr: new_sr }
    }

    /// Streaming band-limited resampler for very long sounds.
    fn resample_streaming(&self, new_sr: f64) -> Sound {
        let ratio = new_sr / self.sr;
        let cutoff = ratio.min(1.0);
        const DEPTH: f64 = 24.0;
        let half = (DEPTH / cutoff).ceil() as isize;
        let n_out = (self.duration() * new_sr).round() as usize;
        let x1 = 0.5 * (self.duration() - (n_out as f64 - 1.0) / new_sr);
        let y = (0..n_out)
            .map(|j| {
                let xi = self.index(x1 + j as f64 / new_sr);
                let c = xi.floor() as isize;
                let mut acc = 0.0;
                for i in (c - half + 1)..=(c + half) {
                    let d = (xi - i as f64) * cutoff;
                    let w = if d.abs() >= DEPTH { 0.0 } else { 0.5 + 0.5 * (PI * d / DEPTH).cos() };
                    acc += self.at(i) * sinc(d) * w;
                }
                acc * cutoff
            })
            .collect();
        Sound { x: y, sr: new_sr }
    }

    /// Praat's `Sampled_shortTermAnalysis`: (number of frames, time of first frame).
    pub fn short_term(&self, window: f64, dt: f64) -> (usize, f64) {
        let dur = self.duration();
        if window > dur || dt <= 0.0 {
            return (0, 0.0);
        }
        let n = ((dur - window) / dt).floor() as usize + 1;
        let t1 = 0.5 * dur - 0.5 * (n as f64 - 1.0) * dt;
        (n, t1)
    }
}

/// Praat `NUM_interpolate_sinc`: `x` is a 1-based index into `y`; depth is
/// clipped at the edges, with constant extrapolation outside.
pub fn praat_sinc(y: &[f64], x: f64, max_depth: usize) -> f64 {
    let n = y.len();
    if n == 0 {
        return 0.0;
    }
    if x < 1.0 {
        return y[0];
    }
    if x > n as f64 {
        return y[n - 1];
    }
    let midleft = x.floor() as usize;
    if x == midleft as f64 {
        return y[midleft - 1];
    }
    let midright = midleft + 1;
    let depth = max_depth.min(midright - 1).min(n - midleft);
    if depth == 0 {
        return y[(x.round() as usize).clamp(1, n) - 1];
    }
    if depth == 1 {
        return y[midleft - 1] + (x - midleft as f64) * (y[midright - 1] - y[midleft - 1]);
    }
    if depth == 2 {
        let (yl, yr) = (y[midleft - 1], y[midright - 1]);
        let dyl = 0.5 * (yr - y[midleft - 2]);
        let dyr = 0.5 * (y[midright] - yl);
        let (fil, fir) = (x - midleft as f64, midright as f64 - x);
        return yl * fir + yr * fil - fil * fir * (0.5 * (dyr - dyl) + (fil - 0.5) * (dyl + dyr - 2.0 * (yr - yl)));
    }
    let d = depth as f64 + 0.5;
    let (left, right) = (midright - depth, midleft + depth);
    let mut acc = 0.0;
    for i in left..=right {
        let u = (x - i as f64).abs();
        acc += y[i - 1] * sinc(u) * 0.5 * (1.0 + (PI * u / d).cos());
    }
    acc
}

#[inline]
pub fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-12 { 1.0 } else { (PI * x).sin() / (PI * x) }
}

/// Hann-windowed sinc interpolation of `y` (defined on integer positions,
/// `y(k)` supplied by the closure) at real position `x`.
pub fn interpolate_sinc(y: impl Fn(isize) -> f64, x: f64, depth: usize) -> f64 {
    let c = x.floor() as isize;
    if (x - c as f64).abs() < 1e-12 {
        return y(c);
    }
    let d = depth as f64;
    let mut acc = 0.0;
    for k in (c - depth as isize + 1)..=(c + depth as isize) {
        let u = x - k as f64;
        let w = 0.5 + 0.5 * (PI * u / (d + 0.5)).cos();
        acc += y(k) * sinc(u) * w;
    }
    acc
}

/// Maximise a smooth function on [a, b] (golden-section search). Returns (x, f(x)).
pub fn maximize(f: impl Fn(f64) -> f64, mut a: f64, mut b: f64, iters: usize) -> (f64, f64) {
    let g = 0.5 * (5f64.sqrt() - 1.0);
    let mut c = b - g * (b - a);
    let mut d = a + g * (b - a);
    let (mut fc, mut fd) = (f(c), f(d));
    for _ in 0..iters {
        if fc > fd {
            b = d;
            d = c;
            fd = fc;
            c = b - g * (b - a);
            fc = f(c);
        } else {
            a = c;
            c = d;
            fc = fd;
            d = a + g * (b - a);
            fd = f(d);
        }
    }
    let x = 0.5 * (a + b);
    (x, f(x))
}

/// Modified Bessel function of the first kind, order 0 (power series).
pub fn bessel_i0(x: f64) -> f64 {
    let (mut sum, mut term, q) = (1.0, 1.0, x * x / 4.0);
    for k in 1..500 {
        term *= q / (k as f64 * k as f64);
        sum += term;
        if term < sum * 1e-17 {
            break;
        }
    }
    sum
}

/// Radix-2 complex FFT in f64 (analysis only, allocates).
pub struct Fft64 {
    n: usize,
    tw: Vec<(f64, f64)>,
    rev: Vec<usize>,
}

impl Fft64 {
    pub fn new(n: usize) -> Fft64 {
        assert!(n.is_power_of_two() && n >= 2);
        let bits = n.trailing_zeros();
        Fft64 {
            n,
            tw: (0..n / 2).map(|k| (-2.0 * PI * k as f64 / n as f64).sin_cos()).map(|(s, c)| (c, s)).collect(),
            rev: (0..n).map(|i| i.reverse_bits() >> (usize::BITS - bits)).collect(),
        }
    }
    pub fn len(&self) -> usize {
        self.n
    }
    /// Forward transform (sign -1). Inverse = conj/forward/conj and 1/n.
    pub fn forward(&self, re: &mut [f64], im: &mut [f64]) {
        let n = self.n;
        for i in 0..n {
            let j = self.rev[i];
            if j > i {
                re.swap(i, j);
                im.swap(i, j);
            }
        }
        let mut len = 2;
        while len <= n {
            let step = n / len;
            for s in (0..n).step_by(len) {
                for k in 0..len / 2 {
                    let (c, si) = self.tw[k * step];
                    let (a, b) = (s + k, s + k + len / 2);
                    let tr = re[b] * c - im[b] * si;
                    let ti = re[b] * si + im[b] * c;
                    re[b] = re[a] - tr;
                    im[b] = im[a] - ti;
                    re[a] += tr;
                    im[a] += ti;
                }
            }
            len <<= 1;
        }
    }
    /// Power spectrum |X|² of real `x` zero-padded to n (bins 0..=n/2).
    pub fn power(&self, x: &[f64]) -> Vec<f64> {
        let mut re = vec![0.0; self.n];
        let mut im = vec![0.0; self.n];
        re[..x.len().min(self.n)].copy_from_slice(&x[..x.len().min(self.n)]);
        self.forward(&mut re, &mut im);
        (0..=self.n / 2).map(|k| re[k] * re[k] + im[k] * im[k]).collect()
    }
    /// Autocorrelation of real `x` (zero-padded): r[lag] for lag 0..n.
    pub fn autocorr(&self, x: &[f64]) -> Vec<f64> {
        let mut re = vec![0.0; self.n];
        let mut im = vec![0.0; self.n];
        re[..x.len().min(self.n)].copy_from_slice(&x[..x.len().min(self.n)]);
        self.forward(&mut re, &mut im);
        for k in 0..self.n {
            re[k] = re[k] * re[k] + im[k] * im[k];
            im[k] = 0.0;
        }
        self.forward(&mut re, &mut im); // power spectrum is real & even
        re.iter().map(|v| v / self.n as f64).collect()
    }
}

/// Praat `Sampled_getMean (…, interpolate = true)`: mean of the linearly
/// interpolated curve through samples `v` (first at `x1`, step `dx`) over
/// [xmin, xmax] ∩ [dmin, dmax] (the object's domain).
pub fn sampled_mean(v: &[f64], x1: f64, dx: f64, dmin: f64, dmax: f64, xmin: f64, xmax: f64) -> Option<f64> {
    let n = v.len() as isize;
    let (xmin, xmax) = (xmin.max(dmin), xmax.min(dmax));
    if n == 0 || xmax <= xmin {
        return None;
    }
    let val = |i: isize| -> Option<f64> { (i >= 1 && i <= n).then(|| v[(i - 1) as usize]) };
    let imin = (((xmin - x1) / dx).ceil() as isize + 1).max(1);
    let imax = (((xmax - x1) / dx).floor() as isize + 1).min(n);
    let (mut sum, mut range) = (0.0, 0.0);
    if imax >= imin {
        let left_edge = x1 - 0.5 * dx;
        let right_edge = left_edge + n as f64 * dx;
        for i in imin..=imax {
            sum += v[(i - 1) as usize];
            range += 1.0;
        }
        if xmin > left_edge {
            let mut phase = (x1 + (imin - 1) as f64 * dx - xmin) / dx;
            let (r, l) = (val(imin), val(imin - 1));
            if let Some(r) = r {
                range -= 0.5;
                sum -= 0.5 * r;
                if let Some(l) = l {
                    range += phase;
                    sum += phase * (r + 0.5 * phase * (l - r));
                } else {
                    phase = phase.min(0.5);
                    range += phase;
                    sum += phase * r;
                }
            }
        }
        if xmax < right_edge {
            let mut phase = (xmax - (x1 + (imax - 1) as f64 * dx)) / dx;
            let (l, r) = (val(imax), val(imax + 1));
            if let Some(l) = l {
                range -= 0.5;
                sum -= 0.5 * l;
                if let Some(r) = r {
                    range += phase;
                    sum += phase * (l + 0.5 * phase * (r - l));
                } else {
                    phase = phase.min(0.5);
                    range += phase;
                    sum += phase * l;
                }
            }
        }
    } else {
        // no sample centre inside: value of the interpolated curve at the midpoint
        let mid = 0.5 * (xmin + xmax);
        let idx = (mid - x1) / dx + 1.0;
        let l = idx.floor() as isize;
        let f = idx - l as f64;
        return match (val(l), val(l + 1)) {
            (Some(a), Some(b)) => Some(a + f * (b - a)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            _ => None,
        };
    }
    (range > 0.0).then(|| sum / range)
}

/// Linear-interpolated quantile of sorted data (Praat's NUMquantile).
pub fn quantile(sorted: &[f64], q: f64) -> Option<f64> {
    let n = sorted.len();
    if n == 0 {
        return None;
    }
    let place = q * n as f64 + 0.5;
    let left = place.floor();
    if left < 1.0 {
        return Some(sorted[0]);
    }
    if left >= n as f64 {
        return Some(sorted[n - 1]);
    }
    let i = left as usize; // 1-based left
    Some(sorted[i - 1] + (place - left) * (sorted[i] - sorted[i - 1]))
}

#[cfg(test)]
pub(crate) mod testsig {
    use std::f64::consts::PI;

    pub fn rng(seed: &mut u32) -> f64 {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 17;
        *seed ^= *seed << 5;
        *seed as f64 / u32::MAX as f64 * 2.0 - 1.0
    }

    /// Glottal-like source: pulses at explicit instants with per-cycle
    /// amplitude, continuous 10-harmonic cycle shape (peak at each instant).
    pub fn voice(sr: f64, secs: f64, f0: f64, jitter: f64, shimmer: f64, noise: f64) -> Vec<f64> {
        let n = (secs * sr) as usize;
        let mut seed = 12345u32;
        let mut peaks = vec![(0.0f64, 0.5 * (1.0 + shimmer * rng(&mut seed)))];
        while peaks.last().unwrap().0 < n as f64 {
            let t = sr / f0 * (1.0 + jitter * rng(&mut seed));
            peaks.push((peaks.last().unwrap().0 + t, 0.5 * (1.0 + shimmer * rng(&mut seed))));
        }
        let mut x = vec![0.0; n];
        for w in peaks.windows(2) {
            let ((p0, a0), (p1, a1)) = (w[0], w[1]);
            for (i, v) in x.iter_mut().enumerate().take((p1.ceil() as usize).min(n)).skip(p0.ceil() as usize) {
                let u = (i as f64 - p0) / (p1 - p0);
                let g: f64 = (1..=10).map(|h| (2.0 * PI * h as f64 * u).cos() / h as f64).sum::<f64>() / 2.9;
                *v = (a0 * (1.0 - u) + a1 * u) * g;
            }
        }
        for v in x.iter_mut() {
            *v += noise * rng(&mut seed);
        }
        x
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_keeps_a_sine() {
        let sr = 48000.0;
        let s = Sound { x: (0..48000).map(|i| (2.0 * PI * 440.0 * (i as f64 + 0.5) / sr).sin()).collect(), sr };
        let r = s.resample(11025.0);
        assert_eq!(r.x.len(), 11025);
        for (j, v) in r.x.iter().enumerate().skip(200).take(10000) {
            let want = (2.0 * PI * 440.0 * r.t(j as isize)).sin();
            assert!((v - want).abs() < 2e-3, "{j}: {v} vs {want}");
        }
        // a tone above the new Nyquist is removed
        let hi = Sound { x: (0..48000).map(|i| (2.0 * PI * 9000.0 * i as f64 / sr).sin()).collect(), sr };
        let rms = (hi.resample(11025.0).x[500..10000].iter().map(|v| v * v).sum::<f64>() / 9500.0).sqrt();
        assert!(rms < 0.01, "alias rms {rms}");
    }

    #[test]
    fn sinc_interpolation_and_maximizer() {
        let y = |k: isize| (0.3 * k as f64).cos();
        let v = interpolate_sinc(y, 10.4, 40);
        assert!((v - (0.3f64 * 10.4).cos()).abs() < 1e-3, "{v}");
        let (x, fx) = maximize(|x| -(x - 1.234) * (x - 1.234) + 2.0, 0.0, 3.0, 60);
        assert!((x - 1.234).abs() < 1e-6 && (fx - 2.0).abs() < 1e-9);
    }

    #[test]
    fn sampled_mean_integrates_the_line() {
        // samples of y = x at x = 0,1,…,9; mean over [2.25, 6.75] of the line is 4.5
        let v: Vec<f64> = (0..10).map(|i| i as f64).collect();
        let m = sampled_mean(&v, 0.0, 1.0, -0.5, 9.5, 2.25, 6.75).unwrap();
        assert!((m - 4.5).abs() < 1e-12, "{m}");
    }

    #[test]
    fn bessel_and_quantile() {
        assert!((bessel_i0(1.0) - 1.2660658777520084).abs() < 1e-12);
        assert_eq!(quantile(&[1.0, 2.0, 3.0, 4.0], 0.5), Some(2.5));
    }
}
