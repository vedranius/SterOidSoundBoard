# Changelog

All notable changes are documented here. Versioning: [SemVer](https://semver.org).

## [0.4.0] - 2026-09-28

DigiLingua becomes a clinical workstation: **Praat-compatible voice analysis** (validated against
Praat 7), a **Praat-style sound editor**, an enlargeable **live sonagram** with legend and readouts,
and the DigiLingua web app's workflow (patient list, clinical presets, device/status panel,
sessions, progress). See [VALIDATION.md](VALIDATION.md).

### Added
- **Praat-compatible analysis engine** (`crates/engine/src/praat/`), a port of Praat's algorithms:
  pitch (AC + Viterbi path finder), glottal pulses (cc), voice report (voicing, 5 jitter and
  6 shimmer variants, HNR, NHR, mean autocorrelation), intensity, Burg formants and CPPS with the
  AVQI settings. Pitch, pulses, jitter, shimmer, HNR and intensity equal Praat 7.0 to all printed
  digits on 7 test signals incl. Croatian speech; formants within 0–2 %, CPPS within 0.5 %.
  Reproducible with `tools/validate.sh` (+ `tools/make_test_signals.py`, `tools/speech_hr.praat`).
- **Sound editor** in the analysis window (like Praat's): waveform with glottal pulses, spectrogram
  (Gaussian window, 5–50 ms, pre-emphasis, dynamic range, 4 colour maps, dB colour bar) with pitch,
  intensity and formant tracks and their scales, shared zoomable time axis (wheel, buttons, keys),
  selection, cursor, play selection/visible part, spectral slice at the cursor or LTAS of the
  selection with formant markers, live readouts (time, Hz, dB, F0, intensity, F1–F4).
  Pitch range / formant settings per recording with male/female/child presets.
- **Disfluency annotations** on a tier under the spectrogram (block, prolongation, sound/syllable/
  word repetition, interjection, revision, hard onset, voice break, whisper, secondary behaviour,
  other): summary with SLD count, %SS (manual or automatic syllable count), rate per minute,
  mean of the 3 longest SLD; **Praat TextGrid export**.
- **Full measures table** (Praat voice report groups, formants, intensity, timing) with orientation
  thresholds, tiles, and a **printable / PDF report** (clinic header, patient, recording, editor
  image, measures, disfluencies, clinical report, AI opinion, signature line).
- **Live sonagram**: server-side analyzer (10 ms frames, Praat Gaussian window 5/15/30 ms, 0.5 dB
  resolution up to 12 kHz, live F0 and intensity) streamed only to clients that show it; the view
  has range 0–4/5/8/12 kHz, dynamic range, auto/manual level, colour maps, 4–30 s history, F0 and
  intensity overlays, dB colour legend, axes, hover readout, freeze, PNG snapshot and full screen.
- **DigiLingua workstation** (after the DigiLingua web app): searchable patient list (initials,
  code, age, last session, active-session marker), input/output device selectors and an audio
  status panel (LED, sample rate, buffer, estimated latency, DSP load, xruns, Start/Stop),
  31-band EQ as **vertical sliders with frequency-group shading** or as a curve, quick curves,
  **clinical presets** (DigiLingua factory curves + DAF/FAF/Lombard/interruption starting points +
  your own, save/load/delete, current preset and "modified" badge), clickable **signal chain**,
  mic + L/R ear meters with **peak hold** and **per-ear mute**, keyboard shortcuts
  (Space audio, R reset EQ, D refresh devices, M mute, 1–3 tabs, / search, Esc).
- **Sessions**: the preset in use is stored; session detail shows all settings at the start of the
  session, notes and its recordings; **"apply settings of this session"**.
- **Progress** tab: charts per measure over time (F0, jitter, shimmer, HNR, CPPS, MPT, F0 range,
  intensity, speech/articulation rate, %SS, SLD) with normal ranges, change since the first
  recording, filter by task, table of all recordings and **CSV export** (Excel-friendly).
- **Clinic settings**: clinic and clinician for reports, default analysis settings, and
  **microphone calibration** to dB SPL (computed from a calibrator recording of known level).
- AI prompt includes all new measures, CPPS, formants, calibrated intensity and the disfluency
  summary; the AI uses the analysis settings chosen in the editor and gets the editor image.

### Changed
- Voice analysis rewritten on the Praat engine (report text in Praat sections; earlier recordings
  are re-analysed with the new engine). Analysis of the whole recording is cached.
- Heavy analysis runs outside the audio path and uses all cores for CPPS; Praat's sinc
  interpolation scheme makes pitch 3× faster. The editor shows quick measures first, CPPS follows.

### Notes
- Still no login — patient data and the AI key are reachable on the network; run with `--local`
  in clinics until PIN/login arrives (next release).

## [0.3.0] - 2026-09-28

DigiLingua: full patient profile and an **AI opinion** on voice recordings for the rehabilitator.

### Added
- **Patient profile**: sex, age (from birth date), diagnosis / ICD-10, current problem, history,
  medical history, medications, occupation and vocal load, smoking, hearing, native language,
  therapy goals, notes and **consent for AI processing**. Edited in a dialog; summary in the sidebar.
  Older `clinic.json` files load unchanged.
- **Recording task** (sustained /a/ /i/ /u/, reading, spontaneous speech, counting, …) chosen when
  recording and editable later, plus the clinician's observations per recording.
- **New acoustic measures** (also in the clinical report): F0 range in semitones, longest continuous
  phonation, voice breaks and their degree, silent pauses ≥ 250 ms (count, mean, share), syllable
  nuclei, speech rate and articulation rate (de Jong & Wempe) — useful for fluency/stuttering.
- **AI opinion** in the analysis window: the AI receives the acoustic measures, F0 contour summary,
  pseudonymised patient profile (**never name, code or birth date** — age only), recording task,
  clinician's notes, earlier recordings of the patient for comparison, an optional question and,
  optionally, the sonagram image. It returns a structured Croatian opinion (summary, interpretation,
  link to the problem, hypotheses to check, rehabilitation suggestions incl. DAF/FAF/noise settings,
  referral red flags, limitations). Opinions are stored per recording, can be reopened, copied,
  deleted and are included in the saved `.txt` report. "Pregled podataka za AI" shows exactly what is sent.
- **AI settings** (header "AI"): Anthropic Claude (default `claude-opus-5`, server-side refusal
  fallback), any OpenAI-compatible API (OpenAI, LM Studio, vLLM, OpenRouter…) or local **Ollama**
  (nothing leaves the machine). The API key stays in `ai.json` (owner-only permissions) and is never
  sent to browsers; "Test veze" checks provider, model and key.
- AI requests require the patient's recorded consent; retries on overload/rate limits; local and
  LAN AI servers bypass HTTP proxies from the environment.

### Notes
- The AI does not hear the audio (the Claude API has no audio input); it interprets measurements
  and the sonagram image. The opinion supports the rehabilitator and is not a diagnosis.
- Still no login: anyone on the network can use the configured AI key — use `--local` in clinics.

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
