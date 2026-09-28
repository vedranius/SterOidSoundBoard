# SterOidSoundBoard — context for Claude Code

## What this is
Cross-platform real-time audio effects host (successor idea to MOD Desktop + Pisound/Patchbox OS),
rebuilt from scratch in Rust. One executable = audio engine + HTTP/WebSocket server + embedded web UI.
Controlled from any browser (desktop, tablet, phone). Targets: Linux x64, Windows x64, Raspberry Pi 4/5 (arm64).
Owner: Vedran (communicates in Croatian, casual/direct; wants precise, copy-paste-ready answers).

Two modes on ONE engine/workflow: **Music** (pedalboard) and **DigiLingua** (speech/voice
rehabilitation for clinicians; merged from repos vedranius/DigiLingua and vedranius/digilingua-lite).
DigiLingua is never a separate audio path: it is an ordinary Board whose nodes carry `role`
tags (mic, eq, daf, faf, interrupt, noise, ears) that `web/clinic.js` binds to. New clinical
features = new generic nodes/engine services + a view, usable by musicians too.
DigiLingua UI text is Croatian; Music UI is English.

## Hard requirements
- Real-time audio on every platform is non-negotiable: the audio thread must never lock, allocate, block or log.
- One-click start: user runs a single executable, everything is bundled, no installer dependencies.
- Install any open-source audio plugin (LV2 first); build custom effects with AI (Faust DSP -> JIT).
- Control-surface designer: separate tab to build tablet UIs bound to board parameters (MIDI-like control).
- AI providers: Claude API, any OpenAI-compatible API, local Ollama — user's choice, user's own key.

## Layout
- `crates/engine` (steroid-engine): `audio.rs` cpal I/O + taps (spectrum ring, recorder ring, mute),
  `graph.rs` Board/Schedule (DAG, topo sort, lock-free swap via rtrb, garbage returned to control
  thread, `sanitize()` for foreign boards, node roles), `nodes.rs` built-in DSP + ParamSpec
  (11 nodes incl. Eq31, Daf, PitchShift, Noise, Interrupter, Channels), `fft.rs` FFT + live spectrum,
  `analysis.rs` voice analysis (F0/jitter/shimmer/HNR + Croatian report), `record.rs` WAV writer
  thread + loader, `templates.rs` built-in boards (DigiLingua chain).
- `crates/app` (steroidsoundboard): `main.rs` args/startup, `state.rs` App state (presets, recording,
  clinic ops, AI jobs; lock order `inner` → `clinic`), `clinic.rs` patients/sessions/recordings/
  AI-report store, `ai.rs` AI providers (Anthropic Messages API raw HTTP via ureq+rustls/ring,
  default `claude-opus-5` + `fallbacks: "default"`; OpenAI-compatible; Ollama) and the Croatian
  clinical prompt (pseudonymised: never name/code/birth date), `server.rs` axum REST + WS.
- `web/` vanilla JS UI, embedded with rust-embed: `app.js` core + pedalboard (HOOKS, setParam,
  setBypass shared by views), `clinic.js` DigiLingua view.
- `packaging/linux/` systemd unit + RPi install script. `.github/workflows/build.yml` CI + releases on `v*` tags.

## Conventions
- SemVer. On every release: bump `VERSION` and `[workspace.package].version` in `Cargo.toml`,
  add a `## [x.y.z] - date` section to `CHANGELOG.md` (CI extracts it as release notes), commit, annotated tag `vX.Y.Z`, push main + tag.
- After each update give Vedran a summary + current version.
- `cargo test -p steroid-engine --lib` must pass; zero compiler warnings.
- Every DSP node output goes through NaN/inf guard; output is hard-clamped to 0 dBFS.

## Status
v0.1.0 engine foundation, pedalboard, CI.
v0.2.0 DigiLingua mode + 6 new nodes, recording, voice analysis, presets/templates, mute, live spectrum.
v0.3.0 full patient profile + AI consent, recording task/notes, fluency measures, AI opinion
(Claude / OpenAI-compatible / Ollama). CI green on all 3 targets since 0.2.0.
No auth yet (patient data + AI key on LAN!) — PIN/login is top priority in 0.4.
Tags can't be pushed from Claude's cloud sessions (git proxy): Vedran pushes `vX.Y.Z` himself.

## Next: v0.4.0
0. PIN/login (patient data on LAN), optional per-clinic data dir.
1. Verify CI green on all 3 targets; fix whatever breaks (Windows first).
2. Windows ASIO (cpal `asio` feature; ASIO SDK in CI, check its license) + WASAPI exclusive option.
3. Native duplex on Linux (JACK/PipeWire via cpal `jack` feature) to remove the input ring-buffer latency.
4. Pisound support on RPi (button + MIDI), RT tuning check.
5. State-preserving graph edits (reuse processors across Schedule rebuilds so delay tails don't reset).
6. Multi-channel routing (beyond first 2 channels).
Roadmap after: 0.5 LV2 hosting + plugin manager, 0.6 control-surface designer + MIDI learn/OSC,
0.7 AI Lab (Faust), 0.8 RPi image, snapshots, DigiLingua exercise protocols + progress charts
(analysis history per patient), PDF report export.
