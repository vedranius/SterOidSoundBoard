//! WAV loading shared by the native app and the WebAssembly build.
use anyhow::Result;
use std::io::Read;
use std::path::Path;

/// Load a WAV file as mono f32: the louder channel (a mono mic on a stereo
/// interface often leaves one channel silent). Returns (samples, sample rate).
pub fn load_mono(path: &Path) -> Result<(Vec<f32>, u32)> {
    decode_mono(std::io::BufReader::new(std::fs::File::open(path)?))
}

/// [`load_mono`] from any reader (e.g. WAV bytes held in memory).
pub fn decode_mono<R: Read>(reader: R) -> Result<(Vec<f32>, u32)> {
    let mut r = hound::WavReader::new(reader)?;
    let spec = r.spec();
    let ch = spec.channels.max(1) as usize;
    let data: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => r.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample - 1)) as f32;
            r.samples::<i32>().map(|s| s.map(|v| v as f32 * scale)).collect::<Result<_, _>>()?
        }
    };
    let energy = |c: usize| data.iter().skip(c).step_by(ch).map(|v| v * v).sum::<f32>();
    let best = (0..ch.min(2)).max_by(|&a, &b| energy(a).total_cmp(&energy(b))).unwrap_or(0);
    Ok((data.iter().skip(best).step_by(ch).copied().collect(), spec.sample_rate))
}

