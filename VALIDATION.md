# Validation against Praat

SterOidSoundBoard's voice analysis (DigiLingua → *Analiza glasa*) is a port of
Praat's algorithms (Paul Boersma & David Weenink, <https://www.praat.org>,
source <https://github.com/praat/praat.github.io>, GPL-3.0-or-later like this
project). This page shows how closely the port reproduces Praat and how to
check it yourself.

## What is ported

| Measure | Praat command and settings | Rust code |
|---|---|---|
| Pitch | `To Pitch (ac): 0, 75, 15, no, 0.03, 0.45, 0.01, 0.35, 0.14, 600` (Hanning window, sinc-70 peak refinement, Viterbi path finder) | `crates/engine/src/praat/pitch.rs` |
| Glottal pulses | `To PointProcess (cc)` | `praat/pulses.rs` |
| Jitter, shimmer, voicing, HNR | `Voice report: 0, 0, 75, 600, 1.3, 1.6, 0.03, 0.45` | `praat/voice.rs` |
| Intensity | `To Intensity: 100, 0, yes` (Kaiser window, mean subtracted, dB re 2·10⁻⁵ Pa) | `praat/intensity.rs` |
| Formants | `To Formant (burg): 0, 5, 5500, 0.025, 50` (Praat's FFT resampling, pre-emphasis, Gaussian window, Burg LPC) | `praat/formant.rs` |
| CPPS | AVQI settings (Maryn & Weenink 2015): `To PowerCepstrogram: 60, 0.002, 5000, 50` + `Get CPPS: no, 0.01, 0.001, 60, 330, 0.05, parabolic, 0.001, 0, straight, robust` | `praat/cepstrum.rs` |
| Resampling, sinc interpolation | `Sound: Resample` (FFT low-pass + sinc depth 50), `NUM_interpolate_sinc` (same order of operations) | `praat/mod.rs` |

Praat details that matter for identical numbers are reproduced on purpose, e.g.
the "0.001, 0" CPPS trend range means *the whole quefrency domain* in Praat,
and Praat's Siegel repeated-median slope ignores the last point's median.

## Results

Seven signals, Praat 7.0.02 (built from source) vs SterOidSoundBoard 0.4.0.
"=" means equal to all printed digits (at least 6 significant digits).

| measure | clean150 | pathol200 | noisy120 | breathy230 | vowel_a | vowel_i | speech_hr |
|---|---:|---:|---:|---:|---:|---:|---:|
| F0 median | = | = | = | = | = | = | = |
| F0 mean | = | = | = | = | = | = | = |
| F0 SD | -0.06 % | = | = | = | = | = | = |
| pulses | = | = | = | = | = | = | = |
| periods | = | = | = | = | = | = | = |
| unvoiced fraction | = | = | = | = | = | = | = |
| voice breaks | = | = | = | = | = | = | = |
| jitter local | = | = | = | = | = | = | = |
| jitter local, absolute | = | = | = | = | = | = | = |
| jitter rap | = | = | = | = | = | = | = |
| jitter ppq5 | = | = | = | = | = | = | = |
| shimmer local | = | = | = | = | = | = | = |
| shimmer local, dB | = | = | = | = | = | = | = |
| shimmer apq3 | = | = | = | = | = | = | = |
| shimmer apq5 | = | = | = | = | = | = | = |
| shimmer apq11 | = | = | = | = | = | = | = |
| HNR | = | = | = | = | = | = | = |
| intensity mean | = | = | = | = | = | = | = |
| F1 median | = | -0.03 % | +2.07 % | +0.52 % | = | = | = |
| F2 median | = | -0.02 % | -0.16 % | +1.99 % | = | = | = |
| F3 median | = | -0.01 % | -0.40 % | +0.28 % | = | = | = |
| F4 median | = | -0.02 % | +0.03 % | +0.48 % | = | = | +0.03 % |
| CPPS | +0.50 % | -0.11 % | -0.07 % | -0.20 % | +0.02 % | +0.10 % | +0.04 % |

*F0 SD of clean150*: 0.001639 vs 0.00164 Hz — the same value printed with
different rounding.

**Summary**

- Pitch, pulses, voicing, all jitter and shimmer variants, HNR and intensity:
  identical to Praat on every signal, including connected speech.
- Formants: identical on clean vowels and speech; on very noisy or breathy
  signals (HNR 2–9 dB) within 0.5–2 %. There Burg LPC poles lie close together
  and Praat's polynomial root polisher and ours (Aberth–Ehrlich) settle a few
  frames on different roots; medians over the signal then differ slightly.
- CPPS: within 0.5 % (≤ 0.13 dB) everywhere.

### The signals

- `clean150`, `pathol200`, `noisy120`, `breathy230`: synthetic glottal pulse
  trains (150–230 Hz) with controlled jitter, shimmer and noise
  (`tools/make_test_signals.py`).
- `vowel_a`, `vowel_i`: the same source through formant resonators
  (/a/ 700/1220/2600/3300 Hz, /i/ 310/2300/3000/3700 Hz).
- `speech_hr`: Croatian sentence from Praat's eSpeak synthesizer
  (`tools/speech_hr.praat`).

## Reproduce

1. Build Praat (the "barren" command-line edition is enough; see Praat's `HOW_TO_BUILD_ONE.md`):
   ```
   git clone --depth 1 https://github.com/praat/praat.github.io praat && cd praat
   make PRAAT_GRAPHICS=barren PRAAT_AUDIO=none -j4     # → ./praat_barren
   ```
2. Make the test signals and compare:
   ```
   mkdir val && cd val
   python3 ../tools/make_test_signals.py
   /path/to/praat_barren --run ../tools/speech_hr.praat
   PRAAT=/path/to/praat_barren ../tools/validate.sh *.wav
   ```
   `tools/validate.sh` builds `crates/engine/examples/praat_compare.rs`, runs it
   and `tools/praat_compare.praat` on each file and prints both values and the
   difference; `<` marks ≥ 2 %, `<<<` ≥ 10 %.

Any WAV file works, including your own recordings: exported recordings
(`⬇ WAV` in the analysis view) can be compared directly.

## Scope

- Validated: the measures above for whole files, with Praat's default settings
  listed in the table. Other pitch ranges and formant settings use the same code.
- The analysis view's spectrogram picture is drawn in the browser (Praat-style
  Gaussian window and dynamic range) and is a display, not a measurement.
- The live sonagram's F0 and intensity (real time, 10 ms frames) are a
  single-frame approximation for visual feedback, not Praat's full algorithm.
- Speech rate / syllable nuclei follow de Jong & Wempe's approach on Praat
  intensity; pause statistics use the intensity track. They are not part of
  Praat itself and are not in the table.
- Clinical thresholds shown in the UI (e.g. jitter < 1.04 %) come from the
  MDVP literature and are orientation values only.
