//! Channel routing between the sound card and the board: an input matrix
//! (every device input channel → board input L/R) and an output matrix
//! (board L/R and the dry input L/R → every device output channel, e.g. a
//! headphone pair on a multi-channel interface). Gains are linear 0–2.
//! Shared by the native audio thread (atomics) and the browser worklet (JSON).
use crate::nodes::AtomicF32;
use serde::{Deserialize, Serialize};

pub const MAX_CH: usize = 32;
/// Output matrix sources, in column order.
pub const OUT_SOURCES: [&str; 4] = ["board_l", "board_r", "input_l", "input_r"];

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Routing {
    /// Row per device input channel: gain to board input [L, R]. Empty = default.
    pub inputs: Vec<[f32; 2]>,
    /// Row per device output channel: gains of [board L, board R, input L, input R]. Empty = default.
    pub outputs: Vec<[f32; 4]>,
}

impl Routing {
    /// Default input row: channel 0 → L, 1 → R; a mono device feeds both.
    pub fn default_input(ch: usize, n: usize) -> [f32; 2] {
        match (ch, n) {
            (0, 1) => [1.0, 1.0],
            (0, _) => [1.0, 0.0],
            (1, _) => [0.0, 1.0],
            _ => [0.0, 0.0],
        }
    }
    /// Default output row: channel 0 ← board L, 1 ← board R.
    pub fn default_output(ch: usize) -> [f32; 4] {
        match ch {
            0 => [1.0, 0.0, 0.0, 0.0],
            1 => [0.0, 1.0, 0.0, 0.0],
            _ => [0.0; 4],
        }
    }
    pub fn input(&self, ch: usize, n: usize) -> [f32; 2] {
        match self.inputs.get(ch) {
            Some(r) if !self.inputs.is_empty() => r.map(clean),
            _ if self.inputs.is_empty() => Self::default_input(ch, n),
            _ => [0.0; 2],
        }
    }
    pub fn output(&self, ch: usize) -> [f32; 4] {
        match self.outputs.get(ch) {
            Some(r) => r.map(clean),
            None if self.outputs.is_empty() => Self::default_output(ch),
            None => [0.0; 4],
        }
    }
    /// Keep at most MAX_CH rows with finite gains in 0..=2.
    pub fn sanitized(mut self) -> Self {
        self.inputs.truncate(MAX_CH);
        self.outputs.truncate(MAX_CH);
        self.inputs.iter_mut().for_each(|r| *r = r.map(clean));
        self.outputs.iter_mut().for_each(|r| *r = r.map(clean));
        self
    }
}

fn clean(g: f32) -> f32 {
    if g.is_finite() { g.clamp(0.0, 2.0) } else { 0.0 }
}

/// The matrix as the audio thread reads it (lock-free), plus per-channel peaks.
#[derive(Default)]
pub struct LiveRouting {
    pub ins: [[AtomicF32; 2]; MAX_CH],
    pub outs: [[AtomicF32; 4]; MAX_CH],
    pub in_peak: [AtomicF32; MAX_CH],
    pub out_peak: [AtomicF32; MAX_CH],
}

impl LiveRouting {
    pub fn apply(&self, r: &Routing, in_ch: usize, out_ch: usize) {
        for c in 0..MAX_CH {
            let i = if c < in_ch { r.input(c, in_ch) } else { [0.0; 2] };
            let o = if c < out_ch { r.output(c) } else { [0.0; 4] };
            for k in 0..2 {
                self.ins[c][k].set(i[k]);
            }
            for k in 0..4 {
                self.outs[c][k].set(o[k]);
            }
        }
    }
    /// Per-channel peaks since the last call (input, output).
    pub fn take_peaks(&self, in_ch: usize, out_ch: usize) -> (Vec<f32>, Vec<f32>) {
        (
            self.in_peak[..in_ch.min(MAX_CH)].iter().map(|a| a.take()).collect(),
            self.out_peak[..out_ch.min(MAX_CH)].iter().map(|a| a.take()).collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_custom_rows() {
        let r = Routing::default();
        assert_eq!(r.input(0, 1), [1.0, 1.0]);
        assert_eq!(r.input(1, 2), [0.0, 1.0]);
        assert_eq!(r.output(1), [0.0, 1.0, 0.0, 0.0]);
        assert_eq!(r.output(5), [0.0; 4]);
        let r = Routing { inputs: vec![[0.0, 0.0], [0.0, 0.0], [1.0, 1.0]], outputs: vec![[0.0; 4], [0.0; 4], [1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0]] }.sanitized();
        assert_eq!(r.input(2, 4), [1.0, 1.0]);
        assert_eq!(r.input(3, 4), [0.0, 0.0]);
        assert_eq!(r.output(3), [0.0, 1.0, 0.0, 0.0]);
        let bad = Routing { inputs: vec![[f32::NAN, 9.0]], outputs: vec![] }.sanitized();
        assert_eq!(bad.inputs[0], [0.0, 2.0]);
    }
}
