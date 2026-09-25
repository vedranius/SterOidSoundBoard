//! Cross-platform audio I/O (cpal: ALSA/PipeWire on Linux, WASAPI on Windows).
//! The audio callback owns the Schedule; graph changes arrive via a lock-free
//! queue and old schedules are sent back to be freed off the audio thread.
use crate::graph::Schedule;
use crate::nodes::{new_buf, AtomicF32, Buf, MAX_BLOCK};
use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample, StreamConfig};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::Instant;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AudioConfig {
    pub host: Option<String>,
    pub input: Option<String>,
    pub output: Option<String>,
    /// Requested buffer size in frames (None = driver default).
    pub buffer: Option<u32>,
    /// Disable input (output-only use).
    #[serde(default)]
    pub no_input: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct HostDevices {
    pub host: String,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub default_input: Option<String>,
    pub default_output: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunningInfo {
    pub host: String,
    pub input: Option<String>,
    pub output: String,
    pub sample_rate: u32,
    pub buffer: Option<u32>,
    pub out_channels: u16,
    pub in_channels: u16,
}

/// Meters and stats written by the audio thread.
#[derive(Default)]
pub struct EngineStats {
    pub in_peak: [AtomicF32; 2],
    pub out_peak: [AtomicF32; 2],
    /// DSP load 0..1 (peak since last read)
    pub load: AtomicF32,
    pub xruns: AtomicU32,
    pub callback_frames: AtomicU32,
}

pub fn list_devices() -> Vec<HostDevices> {
    cpal::available_hosts()
        .into_iter()
        .filter_map(|id| cpal::host_from_id(id).ok())
        .map(|h| HostDevices {
            host: h.id().name().to_string(),
            inputs: h.input_devices().map(|d| d.filter_map(|x| x.name().ok()).collect()).unwrap_or_default(),
            outputs: h.output_devices().map(|d| d.filter_map(|x| x.name().ok()).collect()).unwrap_or_default(),
            default_input: h.default_input_device().and_then(|d| d.name().ok()),
            default_output: h.default_output_device().and_then(|d| d.name().ok()),
        })
        .collect()
}

pub struct AudioEngine {
    pub info: RunningInfo,
    tx: rtrb::Producer<Box<Schedule>>,
    garbage: rtrb::Consumer<Box<Schedule>>,
    stop: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl AudioEngine {
    pub fn start(cfg: &AudioConfig, stats: Arc<EngineStats>) -> Result<Self> {
        let (tx, cmd_rx) = rtrb::RingBuffer::<Box<Schedule>>::new(8);
        let (garbage_tx, garbage) = rtrb::RingBuffer::<Box<Schedule>>::new(16);
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<RunningInfo>>();
        let cfg = cfg.clone();
        // cpal::Stream is !Send on some platforms: own it on a dedicated thread.
        let thread = std::thread::Builder::new()
            .name("audio-io".into())
            .spawn(move || match open_streams(&cfg, stats, cmd_rx, garbage_tx) {
                Ok((info, streams)) => {
                    let _ = ready_tx.send(Ok(info));
                    let _ = stop_rx.recv();
                    drop(streams);
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                }
            })?;
        let info = ready_rx.recv().map_err(|_| anyhow!("audio thread died"))??;
        log::info!("audio started: {info:?}");
        Ok(AudioEngine { info, tx, garbage, stop: Some(stop_tx), thread: Some(thread) })
    }

    /// Hand a new schedule to the audio thread (lock-free).
    pub fn send_schedule(&mut self, s: Schedule) -> Result<()> {
        self.collect_garbage();
        self.tx.push(Box::new(s)).map_err(|_| anyhow!("schedule queue full"))
    }

    pub fn collect_garbage(&mut self) {
        while self.garbage.pop().is_ok() {}
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        if let Some(s) = self.stop.take() {
            let _ = s.send(());
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn find_host(name: &Option<String>) -> Result<cpal::Host> {
    match name {
        None => Ok(cpal::default_host()),
        Some(n) => {
            let id = cpal::available_hosts()
                .into_iter()
                .find(|h| h.name().eq_ignore_ascii_case(n))
                .ok_or_else(|| anyhow!("audio host '{n}' not available"))?;
            Ok(cpal::host_from_id(id)?)
        }
    }
}

type Streams = (Option<cpal::Stream>, cpal::Stream);

fn open_streams(
    cfg: &AudioConfig,
    stats: Arc<EngineStats>,
    cmd_rx: rtrb::Consumer<Box<Schedule>>,
    garbage_tx: rtrb::Producer<Box<Schedule>>,
) -> Result<(RunningInfo, Streams)> {
    let host = find_host(&cfg.host)?;
    let out_dev = match &cfg.output {
        Some(n) => host
            .output_devices()?
            .find(|d| d.name().map(|x| &x == n).unwrap_or(false))
            .ok_or_else(|| anyhow!("output device '{n}' not found"))?,
        None => host.default_output_device().ok_or_else(|| anyhow!("no default output device"))?,
    };
    let out_def = out_dev.default_output_config().context("output config")?;
    let sr = out_def.sample_rate();
    // Prefer the best sample format at the device's default rate.
    let out_def = match out_dev.supported_output_configs() {
        Ok(it) => it
            .filter(|c| c.min_sample_rate() <= sr && c.max_sample_rate() >= sr && fmt_rank(c.sample_format()) < 99)
            .min_by_key(|c| (fmt_rank(c.sample_format()), c.channels().abs_diff(2)))
            .map(|c| c.with_sample_rate(sr))
            .unwrap_or(out_def),
        Err(_) => out_def,
    };
    let buffer_size = match cfg.buffer {
        Some(b) => cpal::BufferSize::Fixed(b),
        None => cpal::BufferSize::Default,
    };
    let out_cfg = StreamConfig { channels: out_def.channels(), sample_rate: sr, buffer_size: buffer_size.clone() };

    // ---- input (optional)
    let in_dev = if cfg.no_input {
        None
    } else {
        match &cfg.input {
            Some(n) => Some(
                host.input_devices()?
                    .find(|d| d.name().map(|x| &x == n).unwrap_or(false))
                    .ok_or_else(|| anyhow!("input device '{n}' not found"))?,
            ),
            None => host.default_input_device(),
        }
    };
    // Capacity: ~0.5 s of stereo audio; latency is actively bounded in the output callback.
    let (in_prod, in_cons) = rtrb::RingBuffer::<f32>::new(sr.0 as usize);
    let mut in_info = (None, 0u16);
    let in_stream = if let Some(dev) = in_dev {
        let range = dev
            .supported_input_configs()?
            .filter(|c| c.min_sample_rate() <= sr && c.max_sample_rate() >= sr && fmt_rank(c.sample_format()) < 99)
            .min_by_key(|c| (fmt_rank(c.sample_format()), c.channels().abs_diff(2)))
            .ok_or_else(|| anyhow!("input device does not support {} Hz (output rate)", sr.0))?;
        let sc = range.with_sample_rate(sr);
        let in_cfg = StreamConfig { channels: sc.channels(), sample_rate: sr, buffer_size: buffer_size.clone() };
        in_info = (dev.name().ok(), sc.channels());
        let st = stats.clone();
        let s = match sc.sample_format() {
            SampleFormat::F32 => build_input::<f32>(&dev, &in_cfg, in_prod, st)?,
            SampleFormat::I16 => build_input::<i16>(&dev, &in_cfg, in_prod, st)?,
            SampleFormat::I32 => build_input::<i32>(&dev, &in_cfg, in_prod, st)?,
            SampleFormat::U16 => build_input::<u16>(&dev, &in_cfg, in_prod, st)?,
            f => return Err(anyhow!("unsupported input sample format {f:?}")),
        };
        Some(s)
    } else {
        None
    };

    let ctx = OutCtx {
        sched: None,
        cmd_rx,
        garbage_tx,
        input: in_stream.as_ref().map(|_| in_cons),
        inbuf: new_buf(),
        outbuf: new_buf(),
        stats: stats.clone(),
        sr: sr.0 as f32,
        denormals_set: false,
        input_primed: false,
    };
    let out_stream = match out_def.sample_format() {
        SampleFormat::F32 => build_output::<f32>(&out_dev, &out_cfg, ctx)?,
        SampleFormat::I16 => build_output::<i16>(&out_dev, &out_cfg, ctx)?,
        SampleFormat::I32 => build_output::<i32>(&out_dev, &out_cfg, ctx)?,
        SampleFormat::U16 => build_output::<u16>(&out_dev, &out_cfg, ctx)?,
        f => return Err(anyhow!("unsupported output sample format {f:?}")),
    };
    if let Some(s) = &in_stream {
        s.play()?;
    }
    out_stream.play()?;
    let info = RunningInfo {
        host: host.id().name().to_string(),
        input: in_info.0,
        output: out_dev.name().unwrap_or_default(),
        sample_rate: sr.0,
        buffer: cfg.buffer,
        out_channels: out_cfg.channels,
        in_channels: in_info.1,
    };
    Ok((info, (in_stream, out_stream)))
}

fn build_input<T>(dev: &cpal::Device, cfg: &StreamConfig, mut prod: rtrb::Producer<f32>, stats: Arc<EngineStats>) -> Result<cpal::Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let ch = cfg.channels as usize;
    let st = stats.clone();
    Ok(dev.build_input_stream(
        cfg,
        move |data: &[T], _| {
            let mut pk = [0f32; 2];
            for frame in data.chunks_exact(ch) {
                let l = f32::from_sample(frame[0]);
                let r = if ch > 1 { f32::from_sample(frame[1]) } else { l };
                pk[0] = pk[0].max(l.abs());
                pk[1] = pk[1].max(r.abs());
                if prod.slots() >= 2 {
                    let _ = prod.push(l);
                    let _ = prod.push(r);
                }
            }
            stats.in_peak[0].max(pk[0]);
            stats.in_peak[1].max(pk[1]);
        },
        move |e| {
            log::warn!("input stream error: {e}");
            st.xruns.fetch_add(1, Relaxed);
        },
        None,
    )?)
}

struct OutCtx {
    sched: Option<Box<Schedule>>,
    cmd_rx: rtrb::Consumer<Box<Schedule>>,
    garbage_tx: rtrb::Producer<Box<Schedule>>,
    input: Option<rtrb::Consumer<f32>>,
    inbuf: Buf,
    outbuf: Buf,
    stats: Arc<EngineStats>,
    sr: f32,
    denormals_set: bool,
    input_primed: bool,
}

impl OutCtx {
    fn swap_schedule(&mut self) {
        while let Ok(new) = self.cmd_rx.pop() {
            if let Some(old) = self.sched.replace(new) {
                // Freed on the control thread. If the queue is full we leak-drop here (rare).
                let _ = self.garbage_tx.push(old);
            }
        }
    }

    fn render(&mut self, frames: usize, mut write: impl FnMut(usize, f32, f32)) {
        if !self.denormals_set {
            set_flush_denormals();
            self.denormals_set = true;
        }
        self.swap_schedule();
        // Bound input latency: keep at most ~2 callbacks of audio queued.
        if let Some(inp) = &mut self.input {
            let avail = inp.slots() / 2;
            let limit = frames * 2 + 64;
            if avail > limit {
                if let Ok(chunk) = inp.read_chunk((avail - frames) * 2) {
                    chunk.commit_all();
                }
            }
        }
        let mut done = 0;
        while done < frames {
            let n = (frames - done).min(MAX_BLOCK);
            let mut under = false;
            for i in 0..n {
                let (l, r) = match &mut self.input {
                    Some(inp) if inp.slots() >= 2 => {
                        self.input_primed = true;
                        (inp.pop().unwrap_or(0.0), inp.pop().unwrap_or(0.0))
                    }
                    Some(_) => {
                        under = true;
                        (0.0, 0.0)
                    }
                    None => (0.0, 0.0),
                };
                self.inbuf[0][i] = l;
                self.inbuf[1][i] = r;
            }
            if under && done == 0 && self.input_primed {
                self.stats.xruns.fetch_add(1, Relaxed);
            }
            match &mut self.sched {
                Some(s) => {
                    let ib = s.input_mut();
                    ib[0][..n].copy_from_slice(&self.inbuf[0][..n]);
                    ib[1][..n].copy_from_slice(&self.inbuf[1][..n]);
                    s.run(n, &mut self.outbuf);
                }
                None => {
                    self.outbuf[0][..n].fill(0.0);
                    self.outbuf[1][..n].fill(0.0);
                }
            }
            let mut pk = [0f32; 2];
            for i in 0..n {
                // Safety limiter: never send NaN or >0 dBFS to the converter.
                let mut l = self.outbuf[0][i];
                let mut r = self.outbuf[1][i];
                if !l.is_finite() { l = 0.0; }
                if !r.is_finite() { r = 0.0; }
                l = l.clamp(-1.0, 1.0);
                r = r.clamp(-1.0, 1.0);
                pk[0] = pk[0].max(l.abs());
                pk[1] = pk[1].max(r.abs());
                write(done + i, l, r);
            }
            self.stats.out_peak[0].max(pk[0]);
            self.stats.out_peak[1].max(pk[1]);
            done += n;
        }
    }
}

fn build_output<T>(dev: &cpal::Device, cfg: &StreamConfig, mut ctx: OutCtx) -> Result<cpal::Stream>
where
    T: SizedSample + FromSample<f32>,
{
    let ch = cfg.channels as usize;
    let st = ctx.stats.clone();
    Ok(dev.build_output_stream(
        cfg,
        move |data: &mut [T], _| {
            let t0 = Instant::now();
            let frames = data.len() / ch;
            data.fill(T::EQUILIBRIUM);
            ctx.render(frames, |i, l, r| {
                let f = &mut data[i * ch..(i + 1) * ch];
                f[0] = T::from_sample(l);
                if ch > 1 {
                    f[1] = T::from_sample(r);
                }
            });
            let budget = frames as f32 / ctx.sr;
            ctx.stats.load.max(t0.elapsed().as_secs_f32() / budget);
            ctx.stats.callback_frames.store(frames as u32, Relaxed);
        },
        move |e| {
            log::warn!("output stream error: {e}");
            st.xruns.fetch_add(1, Relaxed);
        },
        None,
    )?)
}

fn fmt_rank(f: SampleFormat) -> u8 {
    match f {
        SampleFormat::F32 => 0,
        SampleFormat::I32 => 1,
        SampleFormat::I16 => 2,
        SampleFormat::U16 => 3,
        _ => 99,
    }
}

/// Flush-to-zero / denormals-are-zero on the audio thread.
fn set_flush_denormals() {
    #[cfg(target_arch = "x86_64")]
    #[allow(deprecated)]
    unsafe {
        use std::arch::x86_64::{_mm_getcsr, _mm_setcsr};
        _mm_setcsr(_mm_getcsr() | 0x8040);
    }
    #[cfg(target_arch = "aarch64")]
    unsafe {
        let mut fpcr: u64;
        std::arch::asm!("mrs {}, fpcr", out(reg) fpcr);
        fpcr |= 1 << 24; // FZ
        std::arch::asm!("msr fpcr, {}", in(reg) fpcr);
    }
}
