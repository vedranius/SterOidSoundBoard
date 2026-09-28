# Praat reference values for side-by-side validation of SterOidSoundBoard
# (same parameters). Usage: praat --run praat_compare.praat file.wav
form Compare
    sentence File x.wav
endform
sound = Read from file: file$
mono = Convert to mono
pitch = To Pitch (ac): 0, 75, 15, "no", 0.03, 0.45, 0.01, 0.35, 0.14, 600
selectObject: mono, pitch
pp = To PointProcess (cc)
pmin = 0.8 / 600
pmax = 1.25 / 75
selectObject: pitch
v1 = Get quantile: 0, 0, 0.5, "Hertz"
v2 = Get mean: 0, 0, "Hertz"
v3 = Get standard deviation: 0, 0, "Hertz"
nv = Count voiced frames
nf = Get number of frames
writeInfoLine: "median_pitch ", fixed$ (v1, 6)
appendInfoLine: "mean_pitch ", fixed$ (v2, 6)
appendInfoLine: "sd_pitch ", fixed$ (v3, 6)
selectObject: pp
np = Get number of points
nper = Get number of periods: 0, 0, pmin, pmax, 1.3
appendInfoLine: "pulses ", np
appendInfoLine: "periods ", nper
selectObject: mono, pitch, pp
report$ = Voice report: 0, 0, 75, 600, 1.3, 1.6, 0.03, 0.45
appendInfoLine: "unvoiced_fraction ", fixed$ (extractNumber (report$, "Fraction of locally unvoiced frames: "), 6)
appendInfoLine: "voice_breaks ", extractNumber (report$, "Number of voice breaks: ")
selectObject: pp
j1 = Get jitter (local): 0, 0, pmin, pmax, 1.3
j2 = Get jitter (local, absolute): 0, 0, pmin, pmax, 1.3
j3 = Get jitter (rap): 0, 0, pmin, pmax, 1.3
j4 = Get jitter (ppq5): 0, 0, pmin, pmax, 1.3
appendInfoLine: "jitter_local ", fixed$ (j1, 6)
appendInfoLine: "jitter_abs ", fixed$ (j2, 9)
appendInfoLine: "jitter_rap ", fixed$ (j3, 6)
appendInfoLine: "jitter_ppq5 ", fixed$ (j4, 6)
selectObject: mono, pp
s1 = Get shimmer (local): 0, 0, pmin, pmax, 1.3, 1.6
s2 = Get shimmer (local_dB): 0, 0, pmin, pmax, 1.3, 1.6
s3 = Get shimmer (apq3): 0, 0, pmin, pmax, 1.3, 1.6
s4 = Get shimmer (apq5): 0, 0, pmin, pmax, 1.3, 1.6
s5 = Get shimmer (apq11): 0, 0, pmin, pmax, 1.3, 1.6
appendInfoLine: "shimmer_local ", fixed$ (s1, 6)
appendInfoLine: "shimmer_db ", fixed$ (s2, 6)
appendInfoLine: "shimmer_apq3 ", fixed$ (s3, 6)
appendInfoLine: "shimmer_apq5 ", fixed$ (s4, 6)
appendInfoLine: "shimmer_apq11 ", fixed$ (s5, 6)
appendInfoLine: "hnr ", fixed$ (extractNumber (report$, "Mean harmonics-to-noise ratio: "), 6)
selectObject: mono
int = To Intensity: 100, 0, "yes"
im = Get mean: 0, 0, "dB"
appendInfoLine: "intensity_mean ", fixed$ (im, 6)
selectObject: mono
fm = To Formant (burg): 0, 5, 5500, 0.025, 50
for k to 4
    fq = Get quantile: k, 0, 0, "hertz", 0.5
    appendInfoLine: "F", k, " ", fixed$ (fq, 6)
endfor
selectObject: mono
pc = To PowerCepstrogram: 60, 0.002, 5000, 50
cp = Get CPPS: "no", 0.01, 0.001, 60, 330, 0.05, "parabolic", 0.001, 0, "Straight", "Robust"
appendInfoLine: "cpps ", fixed$ (cp, 6)
