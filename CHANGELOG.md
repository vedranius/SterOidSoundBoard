# Changelog

All notable changes are documented here. Versioning: [SemVer](https://semver.org).

## [0.2.0] - 2026-09-28

SterOidSoundBoard now has two modes on **one engine**: **Music** (the pedalboard) and
**DigiLingua** (speech/voice rehabilitation for clinicians, merged from the DigiLingua apps).
DigiLingua is not a separate audio path: it is an ordinary board whose nodes carry roles,
so both modes share the same real-time engine, graph, presets and live multi-tablet sync.

### Added
- **Mode switch** in the header: Music ↔ DigiLingua (remembered per browser, `#digilingua` URL).
- **New engine nodes** (usable in both modes, all real-time safe and unit-tested):
  - *31-band EQ* — ISO 1/3-octave graphic EQ 20 Hz–20 kHz, −48…+24 dB per band, flat = bit-transparent.
  - *DAF Delay* — clean 0–5000 ms delay with separate dry/delayed levels (dry 0 % = no echo).
  - *Pitch Shift / FAF* — ±12 semitone shifter (frequency-altered feedback, harmonizer/octaver).
  - *Noise* — white / pink / brown / 1/3-octave narrow-band, calibrated RMS level, per-ear routing.
  - *Interrupter* — periodic click-free mute/duck (DigiLingua "discontinuity", stutter gate).
  - *Channels / Balance* — mono mic from left/right/sum to both ears, 0–200 % per ear.
- **DigiLingua view**: patients (add/edit/delete incl. all their data), therapy sessions with timer,
  notes and a snapshot of all settings, draggable 31-band EQ with factory curves (Ravno, Govor,
  Telefon, Bas, Visoki), DAF/FAF/noise/discontinuity/mic/ears panels, live sonagram 0–8 kHz and VU.
  One-click "Load DigiLingua chain" builds the clinical board.
- **Recording** in the engine: lossless 24-bit WAV of the dry microphone or processed output,
  written by a separate thread (the audio thread only pushes into a wait-free ring), per patient/session.
- **Voice analysis** (Rust, Praat-style): F0 track (autocorrelation, Boersma 1993), jitter
  (local, absolute, RAP), shimmer (local, dB), HNR, intensity, with a Croatian clinical report
  against reference norms (jitter < 1.04 %, shimmer < 3.81 %, HNR > 20 dB). Analysis window:
  waveform selection, STFT sonagram with F0 contour, playback of the selection, copy/save report.
- **Presets**: save/load/delete whole boards; built-in templates (DigiLingua chain, empty board).
- **Output mute** (header button; auto-mute on the "Analiza glasa" tab to avoid echo) — meters,
  spectrum and recording keep working.
- **Live spectrum** over WebSocket (240 bins, 0–12 kHz) from input or output.
- Engine: smoothers use f64 state (a 5 s delay-time glide no longer stalls short of its target).

### Changed
- Large nodes (31-band EQ) fold their bands in the pedalboard card.
- REST API: `/api/board/replace`, `/api/templates`, `/api/presets`, `/api/patients`,
  `/api/sessions`, `/api/record/{start,stop}`, `/api/recordings[/{id}/wav|/analyze]`.

### Security note
- Patient data is stored locally (`clinic.json` + `recordings/` in the data dir) and there is
  **no authentication yet**: in clinics start with `--local` or use a trusted network only.

## [0.1.0] - 2026-09-25

First working foundation: new real-time engine written from scratch in Rust.

### Added
- **Real-time audio engine** (`steroid-engine`)
  - Cross-platform I/O via cpal: ALSA (and PipeWire through ALSA) on Linux, WASAPI on Windows.
  - Lock-free graph updates: the control thread compiles a `Schedule` and passes it to the
    audio thread through a wait-free ring buffer; old schedules are freed off the audio thread.
  - Parameters are atomics — changing a knob never locks or allocates on the audio thread.
  - Parameter smoothing (no zipper noise), flush-to-zero denormals (x86_64 + aarch64).
  - Safety stage on every node and on the output: NaN/inf removal and hard clamp to 0 dBFS.
  - DSP load, peak meters (input, output, per node), xrun counter.
  - Built-in nodes: Gain, Filter/EQ (LP/HP/BP/Peak biquad), Drive, Delay (smoothed, feedback tone), Tremolo.
  - DAG graph with parallel routing, automatic summing and cycle rejection.
- **Single executable** (`steroidsoundboard`): engine + HTTP/WebSocket server + embedded web UI.
  - Auto-starts audio with the last used device, opens the browser, prints the LAN URL for tablets.
  - `--headless`, `--port`, `--bind`, `--local` flags.
  - Autosave of the board every 2 s and on shutdown (Ctrl+C / SIGTERM).
- **Web UI**: pedalboard with drag & drop nodes and cables, sliders, bypass, meters,
  audio device/driver/buffer selection. Multi-client live sync (several tablets at once).
- **Raspberry Pi 4/5 (arm64)**: systemd service + install script (RT limits, performance governor).
- **CI**: GitHub Actions builds Linux x64, Linux arm64 and Windows x64; tagged pushes create releases.

### Known limitations
- Input and output are two streams joined by a bounded ring buffer (adds ≈1 buffer of latency).
  Native duplex backends (JACK/PipeWire, ASIO) come in 0.2.0.
- Node state (e.g. delay tail) resets when the graph topology changes (not on parameter changes).
- Only stereo (first two channels of the interface).
- No authentication yet — use `--local` on untrusted networks.
