//! Small radix-2 FFT and the live spectrum analyser. Control-thread only
//! (allocates at construction), never used on the audio thread.
use std::f32::consts::PI;

pub struct Fft {
    n: usize,
    tw: Vec<(f32, f32)>,
    rev: Vec<usize>,
}

impl Fft {
    /// `n` must be a power of two.
    pub fn new(n: usize) -> Self {
        assert!(n.is_power_of_two() && n >= 2);
        let bits = n.trailing_zeros();
        let rev = (0..n).map(|i| i.reverse_bits() >> (usize::BITS - bits)).collect();
        let tw = (0..n / 2).map(|k| (-2.0 * PI * k as f32 / n as f32).sin_cos()).map(|(s, c)| (c, s)).collect();
        Fft { n, tw, rev }
    }

    pub fn len(&self) -> usize {
        self.n
    }

    /// In-place forward transform (inverse = conjugate, forward, conjugate, /n).
    pub fn forward(&self, re: &mut [f32], im: &mut [f32]) {
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
            for start in (0..n).step_by(len) {
                for k in 0..len / 2 {
                    let (c, s) = self.tw[k * step];
                    let (a, b) = (start + k, start + k + len / 2);
                    let tr = re[b] * c - im[b] * s;
                    let ti = re[b] * s + im[b] * c;
                    re[b] = re[a] - tr;
                    im[b] = im[a] - ti;
                    re[a] += tr;
                    im[a] += ti;
                }
            }
            len <<= 1;
        }
    }

    /// Autocorrelation of real `x` (zero padded to n) via |X|² → IFFT.
    pub fn autocorr(&self, x: &[f32], out: &mut Vec<f32>) {
        let n = self.n;
        let mut re = vec![0.0; n];
        let mut im = vec![0.0; n];
        re[..x.len().min(n)].copy_from_slice(&x[..x.len().min(n)]);
        self.forward(&mut re, &mut im);
        for k in 0..n {
            re[k] = re[k] * re[k] + im[k] * im[k];
            im[k] = 0.0;
        }
        // power spectrum is real and symmetric: forward == inverse up to 1/n
        self.forward(&mut re, &mut im);
        out.clear();
        out.extend(re.iter().map(|v| v / n as f32));
    }
}

pub fn hann(n: usize) -> Vec<f32> {
    (0..n).map(|i| 0.5 - 0.5 * (2.0 * PI * i as f32 / n as f32).cos()).collect()
}

/// Rolling spectrum of the monitored signal for live meters/sonagrams.
pub struct Spectrum {
    fft: Fft,
    win: Vec<f32>,
    ring: Vec<f32>,
    pos: usize,
    fresh: usize,
}

impl Spectrum {
    pub fn new(n: usize) -> Self {
        Spectrum { fft: Fft::new(n), win: hann(n), ring: vec![0.0; n], pos: 0, fresh: 0 }
    }

    pub fn push(&mut self, x: f32) {
        self.ring[self.pos] = x;
        self.pos = (self.pos + 1) % self.ring.len();
        self.fresh += 1;
    }

    /// True if new audio arrived since the last `bins` call.
    pub fn has_fresh(&self) -> bool {
        self.fresh > 0
    }

    /// `count` bins, linear 0..`fmax` Hz, as dBFS (sine at 0 dBFS ≈ 0 dB), max-pooled.
    pub fn bins(&mut self, sr: f32, fmax: f32, count: usize) -> Vec<f32> {
        self.fresh = 0;
        let n = self.fft.len();
        let mut re: Vec<f32> = (0..n).map(|i| self.ring[(self.pos + i) % n] * self.win[i]).collect();
        let mut im = vec![0.0; n];
        self.fft.forward(&mut re, &mut im);
        let norm = 4.0 / n as f32; // Hann coherent gain 0.5, one-sided ×2
        let top = ((fmax / sr * n as f32) as usize).clamp(count, n / 2);
        (0..count)
            .map(|b| {
                let (a, z) = (b * top / count, ((b + 1) * top / count).max(b * top / count + 1));
                let m = (a..z).map(|k| (re[k] * re[k] + im[k] * im[k]).sqrt() * norm).fold(0.0, f32::max);
                (20.0 * (m + 1e-9).log10()).max(-120.0)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spectrum_finds_sine() {
        let sr = 48000.0;
        let mut s = Spectrum::new(2048);
        for k in 0..4096 {
            s.push((2.0 * PI * 1000.0 * k as f32 / sr).sin());
        }
        let b = s.bins(sr, 12000.0, 240); // 50 Hz per bin
        let (i, v) = b.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).unwrap();
        assert_eq!(i, 20);
        assert!(v.abs() < 1.5, "{v} dB");
    }

    #[test]
    fn autocorr_matches_direct() {
        let x: Vec<f32> = (0..300).map(|i| ((i * 7919) % 101) as f32 / 50.0 - 1.0).collect();
        let f = Fft::new(1024);
        let mut r = vec![];
        f.autocorr(&x, &mut r);
        for lag in [0, 1, 17, 150] {
            let d: f32 = (0..x.len() - lag).map(|i| x[i] * x[i + lag]).sum();
            assert!((r[lag] - d).abs() < 1e-2 * d.abs().max(1.0), "lag {lag}: {} vs {d}", r[lag]);
        }
    }
}
