#!/usr/bin/env python3
"""Synthetic test voices for validating SterOidSoundBoard against Praat
(see VALIDATION.md). Writes 16-bit mono 44.1 kHz WAV files into the current
directory. Deterministic (fixed seeds); standard library only."""
import math, random, struct, wave

sr = 44100


def write(name, x):
    w = wave.open(name, "wb")
    w.setnchannels(1); w.setsampwidth(2); w.setframerate(sr)
    w.writeframes(b"".join(struct.pack("<h", int(max(-1, min(1, v)) * 32767)) for v in x))
    w.close()


def voice(secs, f0, jit, shim, noise, seed=1, trem=0.0):
    """Glottal-like pulse train: period jitter `jit`, amplitude shimmer `shim`
    (relative, uniform), additive white noise `noise` (absolute)."""
    random.seed(seed)
    n = int(secs * sr)
    peaks = [(0.0, 0.5 * (1 + shim * random.uniform(-1, 1)))]
    while peaks[-1][0] < n:
        peaks.append((peaks[-1][0] + sr / f0 * (1 + jit * random.uniform(-1, 1)), 0.5 * (1 + shim * random.uniform(-1, 1))))
    x = [0.0] * n
    for (p0, a0), (p1, a1) in zip(peaks, peaks[1:]):
        for i in range(math.ceil(p0), min(math.ceil(p1), n)):
            u = (i - p0) / (p1 - p0)
            g = sum(math.cos(2 * math.pi * h * u) / h ** 1.3 for h in range(1, 25)) / 3.0
            x[i] = (a0 * (1 - u) + a1 * u) * g
    return [v * (1 + trem * math.sin(2 * math.pi * 5 * i / sr)) + noise * random.uniform(-1, 1) for i, v in enumerate(x)]


def resonate(x, forms):
    """Cascade of two-pole resonators (frequency, bandwidth) — a Klatt-style vowel."""
    for f, bw in forms:
        r = math.exp(-math.pi * bw / sr); b1 = 2 * r * math.cos(2 * math.pi * f / sr); b2 = -r * r
        y1 = y2 = 0.0; out = []
        for v in x:
            y = v + b1 * y1 + b2 * y2; y2 = y1; y1 = y; out.append(y)
        x = out
    pk = max(abs(v) for v in x)
    return [v / pk * 0.6 for v in x]


write("clean150.wav", voice(2.0, 150, 0, 0, 0))
write("pathol200.wav", voice(2.0, 200, 0.03, 0.09, 0.0, 2))
write("noisy120.wav", voice(2.0, 120, 0.005, 0.02, 0.08, 3))
write("breathy230.wav", voice(2.0, 230, 0.01, 0.05, 0.2, 4))
write("vowel_a.wav", resonate(voice(1.5, 110, 0.004, 0.02, 0.002, 5), [(700, 80), (1220, 90), (2600, 120), (3300, 150)]))
write("vowel_i.wav", resonate(voice(1.5, 210, 0.004, 0.02, 0.002, 6), [(310, 60), (2300, 100), (3000, 150), (3700, 200)]))
