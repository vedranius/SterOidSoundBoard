//! Recorder: drains the audio thread's recorder ring into a WAV file on a
//! writer thread (24-bit PCM stereo). The audio thread only pushes samples.
use crate::audio::EngineStats;
use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

pub struct Recorder {
    stop: Arc<AtomicBool>,
    frames: Arc<AtomicU64>,
    sample_rate: u32,
    thread: JoinHandle<(rtrb::Consumer<f32>, Result<u64>)>,
    stats: Arc<EngineStats>,
}

impl Recorder {
    /// Start writing `cons` into `path`. `source`: 0 = input (dry), 1 = output.
    /// On failure the ring is handed back so recording stays possible.
    pub fn start(
        mut cons: rtrb::Consumer<f32>,
        path: &Path,
        sample_rate: u32,
        source: u32,
        stats: Arc<EngineStats>,
    ) -> std::result::Result<Recorder, (rtrb::Consumer<f32>, anyhow::Error)> {
        let spec = hound::WavSpec { channels: 2, sample_rate, bits_per_sample: 24, sample_format: hound::SampleFormat::Int };
        let mut wav = match hound::WavWriter::create(path, spec) {
            Ok(w) => w,
            Err(e) => return Err((cons, anyhow!("cannot create {}: {e}", path.display()))),
        };
        // discard anything left over from a previous take
        if let Ok(c) = cons.read_chunk(cons.slots()) {
            c.commit_all();
        }
        let stop = Arc::new(AtomicBool::new(false));
        let frames = Arc::new(AtomicU64::new(0));
        let (st, fr) = (stop.clone(), frames.clone());
        stats.rec_dropped.store(0, Relaxed);
        stats.rec_src.store(source, Relaxed);
        let path: PathBuf = path.to_path_buf();
        let thread = std::thread::Builder::new().name("recorder".into()).spawn(move || {
            let mut written = 0u64;
            let mut res = Ok(());
            loop {
                let stopping = st.load(Relaxed);
                let n = cons.slots() & !1;
                if n > 0 {
                    if let Ok(chunk) = cons.read_chunk(n) {
                        let (a, b) = chunk.as_slices();
                        for &v in a.iter().chain(b) {
                            if res.is_ok() {
                                res = wav.write_sample((v.clamp(-1.0, 1.0) * 8_388_607.0) as i32);
                            }
                        }
                        chunk.commit_all();
                        written += n as u64 / 2;
                        fr.store(written, Relaxed);
                    }
                } else if stopping {
                    break;
                } else {
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
            let res = res.and_then(|_| wav.finalize()).map(|_| written).map_err(|e| anyhow!("{}: {e}", path.display()));
            (cons, res)
        })
        .expect("spawn recorder thread"); // like std::thread::spawn: OS out of threads
        stats.rec_on.store(true, Relaxed);
        Ok(Recorder { stop, frames, sample_rate, thread, stats })
    }

    pub fn seconds(&self) -> f32 {
        self.frames.load(Relaxed) as f32 / self.sample_rate as f32
    }

    /// Stop, flush and close the file. Returns the ring (for reuse) and frames written.
    pub fn finish(self) -> (Option<rtrb::Consumer<f32>>, Result<u64>) {
        self.stats.rec_on.store(false, Relaxed);
        // give an in-flight audio callback time to finish its pushes
        std::thread::sleep(Duration::from_millis(30));
        self.stop.store(true, Relaxed);
        match self.thread.join() {
            Ok((c, r)) => (Some(c), r),
            Err(_) => (None, Err(anyhow!("recorder thread panicked"))),
        }
    }
}

/// Load a WAV file as mono f32: the louder channel (a mono mic on a stereo
/// interface often leaves one channel silent). Returns (samples, sample rate).
pub fn load_mono(path: &Path) -> Result<(Vec<f32>, u32)> {
    let mut r = hound::WavReader::open(path)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_reloads_louder_channel() {
        let dir = std::env::temp_dir().join(format!("ssb-rec-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.wav");
        let stats = Arc::new(EngineStats::default());
        let (mut prod, cons) = rtrb::RingBuffer::<f32>::new(4096);
        let rec = Recorder::start(cons, &path, 48000, 0, stats.clone()).map_err(|e| e.1).unwrap();
        assert!(stats.rec_on.load(Relaxed));
        for k in 0..48000 {
            while prod.slots() < 2 {
                std::thread::sleep(Duration::from_millis(1));
            }
            prod.push(0.0).unwrap(); // silent left
            prod.push(((k % 100) as f32 / 100.0) - 0.5).unwrap();
        }
        let (c, n) = rec.finish();
        assert!(c.is_some());
        assert_eq!(n.unwrap(), 48000);
        let (x, sr) = load_mono(&path).unwrap();
        assert_eq!((x.len(), sr), (48000, 48000));
        assert!((x[37] - (-0.13)).abs() < 1e-4, "{}", x[37]);
        let _ = std::fs::remove_dir_all(dir);
    }
}
