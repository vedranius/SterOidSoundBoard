# SterOidSoundBoard — context for Claude Code

## What this is
Cross-platform real-time audio effects host (successor idea to MOD Desktop + Pisound/Patchbox OS),
rebuilt from scratch in Rust. One executable = audio engine + HTTP/WebSocket server + embedded web UI.
Controlled from any browser (desktop, tablet, phone). Targets: Linux x64, Windows x64, Raspberry Pi 4/5 (arm64).
Owner: Vedran (communicates in Croatian, casual/direct; wants precise, copy-paste-ready answers).

## Hard requirements
- Real-time audio on every platform is non-negotiable: the audio thread must never lock, allocate, block or log.
- One-click start: user runs a single executable, everything is bundled, no installer dependencies.
- Install any open-source audio plugin (LV2 first); build custom effects with AI (Faust DSP -> JIT).
- Control-surface designer: separate tab to build tablet UIs bound to board parameters (MIDI-like control).
- AI providers: Claude API, any OpenAI-compatible API, local Ollama — user's choice, user's own key.

## Layout
- `crates/engine` (steroid-engine): `audio.rs` cpal I/O, `graph.rs` Board/Schedule (DAG, topo sort,
  lock-free swap via rtrb, garbage returned to control thread), `nodes.rs` built-in DSP + ParamSpec.
- `crates/app` (steroidsoundboard): `main.rs` args/startup, `state.rs` App state, `server.rs` axum REST + WS.
- `web/` vanilla JS UI, embedded with rust-embed.
- `packaging/linux/` systemd unit + RPi install script. `.github/workflows/build.yml` CI + releases on `v*` tags.

## Conventions
- SemVer. On every release: bump `VERSION` and `[workspace.package].version` in `Cargo.toml`,
  add a `## [x.y.z] - date` section to `CHANGELOG.md` (CI extracts it as release notes), commit, annotated tag `vX.Y.Z`, push main + tag.
- After each update give Vedran a summary + current version.
- `cargo test -p steroid-engine --lib` must pass; zero compiler warnings.
- Every DSP node output goes through NaN/inf guard; output is hard-clamped to 0 dBFS.

## Status
v0.1.0 done (engine foundation, 5 built-in nodes, web pedalboard, CI). Windows/arm64 CI not yet verified.

## Next: v0.2.0
1. Verify CI green on all 3 targets; fix whatever breaks (Windows first).
2. Windows ASIO (cpal `asio` feature; ASIO SDK in CI, check its license) + WASAPI exclusive option.
3. Native duplex on Linux (JACK/PipeWire via cpal `jack` feature) to remove the input ring-buffer latency.
4. Pisound support on RPi (button + MIDI), RT tuning check.
5. State-preserving graph edits (reuse processors across Schedule rebuilds so delay tails don't reset).
6. Multi-channel routing (beyond first 2 channels).
Roadmap after: 0.3 LV2 hosting + plugin manager, 0.4 control-surface designer + MIDI learn/OSC,
0.5 AI Lab (Faust), 0.6 RPi image, auth/PIN, presets.
