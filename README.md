# SterOidSoundBoard

Real-time audio effects host for Linux, Windows and Raspberry Pi, controlled from any browser
(desktop, tablet, phone). Inspired by MOD Desktop and Patchbox OS, rebuilt from scratch.

**Status:** 0.5.0 — professional DigiLingua workstation (light/dark), Praat-validated analysis page; Music + DigiLingua modes on one engine; Praat-validated voice analysis with a Praat-style editor, live sonagram, progress tracking and AI opinion for clinicians. See [CHANGELOG](CHANGELOG.md) and the roadmap below.

## Two modes, one engine

| | **Music** | **DigiLingua** |
|---|---|---|
| For | musicians, live effects | speech/voice therapists (rehabilitacija govora i glasa) |
| View | pedalboard: drag nodes and cables | clinical panel: patient, session, EQ, DAF/FAF, noise, analysis |
| Engine | same real-time graph | same graph — the DigiLingua chain is an ordinary board |

Both modes edit the same board, so a therapist's settings can be fine-tuned on the pedalboard
and a musician can use any clinical node (31-band EQ, DAF, pitch shift, noise, interrupter).

**DigiLingua features:** searchable patient list with full profiles, therapy sessions (timer, notes,
settings snapshot, re-apply a session's settings), clinical presets (DigiLingua factory curves,
therapy starting points, your own), audio device/status panel, 31-band ISO EQ (sliders or curve),
DAF (0–5 s, no echo), FAF (±12 st), masking noise (white/pink/brown/narrow-band, per ear),
discontinuity filter, per-ear volume and mute, signal-chain view, L/R level meters with peak hold,
an enlargeable **live sonagram** (colour legend in dB, frequency/time axes, F0 and intensity
overlays, hover readout, 5–30 ms windows), lossless WAV recording and keyboard shortcuts.

**Voice analysis (Praat-compatible, [validated](VALIDATION.md)):** a Praat-style editor per
recording — waveform with glottal pulses, spectrogram with pitch, intensity and formant tracks,
zoom/scroll/play, spectral slice or LTAS, colour legend and cursor readouts — and Praat's full voice
report (pitch, pulses, voicing, jitter ×5, shimmer ×6, HNR/NHR), CPPS (AVQI settings), formants,
intensity (dB SPL with microphone calibration), phonation and fluency timing. Disfluency annotation
tier (blocks, prolongations, repetitions… → %SS, SSI-style duration) with Praat TextGrid export,
**progress charts** per patient, printable/PDF report and an **AI opinion** for the rehabilitator.

**AI (optional):** Anthropic Claude (default `claude-opus-5`, your own key), any OpenAI-compatible
API, or **Ollama** — free, local, nothing leaves the computer; install models from the AI settings.
The AI gets measurements, a pseudonymised profile (no name, code or birth date), the task, your
observations and optionally the sonagram image — never the audio. It needs the patient's recorded
consent. A live log shows what it is doing (model loading, GPU/CPU use, tokens/s) and the answer
appears as it is written. Configure it with the **AI** button in the header.

Open it directly with `http://<host>:8420/#digilingua`.

> Patient data stays on the machine (`clinic.json`, `recordings/` in the data directory).
> There is no login yet — in a clinic run with `--local` or on a trusted network.

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

## Data directory
Linux `~/.local/share/SterOidSoundBoard`, Windows `%APPDATA%\SterOidSoundBoard`:
`boards/default.json` (autosave), `presets/`, `clinic_presets/`, `recordings/*.wav`, `clinic.json`
(patients, sessions, recordings and annotations, AI opinions, clinic settings), `ai.json` (AI provider + key, owner-only), `audio.json`.

## Roadmap
- **0.2** ✅ DigiLingua mode, clinical nodes, recording, voice analysis, presets
- **0.3** ✅ patient profiles, fluency measures, AI opinion (Claude / OpenAI-compatible / Ollama)
- **0.4** ✅ Praat-validated analysis + editor, live sonagram, DigiLingua workstation (presets, devices, progress, reports)
- **0.5** ✅ DigiLingua-style professional UI, audio upload, saved analyses, Praat listings
- **0.6** PIN/login, native duplex backends (JACK/PipeWire, ASIO), Pisound support, state-preserving graph edits
- **0.7** LV2 plugin hosting + plugin manager (open-source plugin catalog)
- **0.8** Control-surface designer (custom tablet layouts), MIDI learn, OSC
- **0.9** AI Lab: describe an effect → Faust DSP generated, compiled (JIT) and loaded live
  (Claude / OpenAI-compatible / Ollama, user's choice)
- **1.0** Raspberry Pi image, snapshots, DigiLingua exercise protocols

## License
GPL-3.0-or-later. The voice analysis ports algorithms from [Praat](https://github.com/praat/praat.github.io)
(Paul Boersma & David Weenink, GPL-3.0-or-later).
