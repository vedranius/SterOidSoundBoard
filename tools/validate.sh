#!/bin/bash
# Side-by-side: SterOidSoundBoard vs Praat on WAV files (see VALIDATION.md).
# Usage: PRAAT=/path/to/praat tools/validate.sh file.wav...
# Prints: measure  ours  praat  difference %   (flags: < ≥ 2 %, <<< ≥ 10 %)
set -e
root=$(cd "$(dirname "$0")/.." && pwd)
praat=${PRAAT:-praat}
cargo build --quiet --release --example praat_compare -p steroid-engine --manifest-path "$root/Cargo.toml"
for f in "$@"; do
  f=$(realpath "$f")   # Praat resolves relative paths against the script's folder
  ours=$("$root/target/release/examples/praat_compare" "$f")
  ref=$("$praat" --run "$root/tools/praat_compare.praat" "$f" 2>&1)
  python3 - "$ours" "$ref" "$f" <<'PY'
import sys
def parse(t):
    return {p[0]: p[1] for p in (l.split() for l in t.strip().splitlines()) if len(p) >= 2}
o, p = parse(sys.argv[1]), parse(sys.argv[2])
print(f"== {sys.argv[3].split('/')[-1]}")
for k in o:
    a, b = o[k], p.get(k, "?")
    try:
        fa, fb = float(a), float(b)
        d = (fa - fb) / abs(fb) * 100 if fb else 0.0
        if abs(fa - fb) < 1e-12: d = 0.0   # both zero up to rounding noise
        flag = "" if abs(d) < 2 else (" <" if abs(d) < 10 else " <<<")
        print(f"  {k:18s} {fa:14.6g} {fb:14.6g} {d:+7.2f}%{flag}")
    except ValueError:
        print(f"  {k:18s} {a:>14s} {b:>14s}")
PY
done
