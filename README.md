# SterOidSoundBoard

Real-time audio effects host for Linux, Windows and Raspberry Pi, controlled from any browser
(desktop, tablet, phone). Inspired by MOD Desktop and Patchbox OS, rebuilt from scratch.

**Status:** 0.1.0 — engine foundation. See [CHANGELOG](CHANGELOG.md) and the roadmap below.

## Run

| Platform | Download | Start |
|---|---|---|
| Windows x64 | `steroidsoundboard-*-windows-x64.zip` | double-click `steroidsoundboard.exe` |
| Linux x64 | `steroidsoundboard-*-linux-x64.tar.gz` | double-click / `./steroidsoundboard` |
| Raspberry Pi 4/5 (64-bit OS) | `steroidsoundboard-*-linux-arm64.tar.gz` | `./steroidsoundboard --headless` or `./install-rpi.sh` |

The browser opens at `http://127.0.0.1:8420`. From a tablet on the same network open the
`Tablet/phone` URL printed in the log.

## Architecture

```
┌──────────── steroidsoundboard (one process) ─────────────┐
│  web UI (embedded)  ◄── HTTP/WebSocket ──►  browsers / tablets
│        │                                         │
│  control thread: board, validation, compile      │
│        │  lock-free ring (Schedule)  ▲ garbage   │
│        ▼                             │           │
│  audio thread: Schedule.run() — no locks/allocs  │
│        │                                         │
│  cpal: ALSA/PipeWire · WASAPI (ASIO/JACK: 0.2)   │
└──────────────────────────────────────────────────┘
```

## Build

```bash
# Linux: sudo apt install libasound2-dev pkg-config
cargo build --release
./target/release/steroidsoundboard
```

## Roadmap
- **0.2** native duplex backends (JACK/PipeWire, ASIO), Pisound support, state-preserving graph edits
- **0.3** LV2 plugin hosting + plugin manager (open-source plugin catalog)
- **0.4** Control-surface designer (custom tablet layouts), MIDI learn, OSC
- **0.5** AI Lab: describe an effect → Faust DSP generated, compiled (JIT) and loaded live
  (Claude / OpenAI-compatible / Ollama, user's choice)
- **0.6** Raspberry Pi image, auth/PIN, presets/snapshots

## License
GPL-3.0-or-later
