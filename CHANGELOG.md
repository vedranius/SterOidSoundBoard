# Changelog

All notable changes are documented here. Versioning: [SemVer](https://semver.org).

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
