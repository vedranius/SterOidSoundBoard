#!/usr/bin/env bash
# Builds the browser edition (static site for Cloudflare Pages or any static
# host) into dist/ (or the directory given as $1):
#   - the Rust engine + clinic logic compiled to WebAssembly (crates/web)
#   - the same web UI as the desktop app, plus web/local/* (in-page server,
#     storage, audio worklet, AI), manifest, service worker, security headers
# Needs: rustup target add wasm32-unknown-unknown
set -euo pipefail
cd "$(dirname "$0")/.."
OUT="${1:-dist}"
VERSION="$(tr -d '[:space:]' < VERSION)"

cargo build --release -p steroid-web --target wasm32-unknown-unknown
WASM=target/wasm32-unknown-unknown/release/steroid_web.wasm

rm -rf "$OUT"
mkdir -p "$OUT"
cp -r web/. "$OUT/"
cp "$WASM" "$OUT/steroid_web.wasm"
if command -v wasm-opt >/dev/null 2>&1; then
  wasm-opt -O3 --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext -o "$OUT/steroid_web.wasm" "$OUT/steroid_web.wasm"
fi
# site-level files live at the root
mv "$OUT/local/_headers" "$OUT/_headers"
mv "$OUT/local/sw.js" "$OUT/sw.js"
mv "$OUT/local/manifest.webmanifest" "$OUT/manifest.webmanifest"

sed -i "s/__SSB_VERSION__/$VERSION/" "$OUT/local/server.js"

# index.html: PWA metadata, local stylesheet, and the in-page server scripts
# (after i18n.js, before the UI scripts that call /api).
python3 - "$OUT/index.html" <<'PY'
import sys
p = sys.argv[1]
s = open(p, encoding="utf-8").read()
head = """<link rel="manifest" href="manifest.webmanifest">
<meta name="theme-color" content="#1c5da8">
<meta name="description" content="SterOidSoundBoard / DigiLingua: Praat-compatible voice analysis and auditory feedback for speech therapy. Runs entirely in your browser — no data is sent to a server.">
<link rel="icon" type="image/png" sizes="32x32" href="local/icons/favicon-32.png">
<link rel="apple-touch-icon" href="local/icons/apple-touch-icon.png">
<link rel="stylesheet" href="local/local.css">
</head>"""
assert "</head>" in s
s = s.replace("</head>", head, 1)
scripts = "".join(f'<script src="{f}"></script>\n' for f in ["engine.js", "local/store.js", "local/ai.js", "local/audio.js", "local/server.js", "local/boot.js"])
anchor = '<script src="i18n.js"></script>\n'
assert anchor in s
s = s.replace(anchor, anchor + scripts, 1)
open(p, "w", encoding="utf-8").write(s)
PY

# service worker: precache every file of the site; cache name changes with the content
python3 - "$OUT" "$VERSION" <<'PY'
import hashlib, json, os, sys
out, version = sys.argv[1], sys.argv[2]
files, h = [], hashlib.sha256()
for root, _, names in os.walk(out):
    for n in sorted(names):
        rel = os.path.relpath(os.path.join(root, n), out).replace(os.sep, "/")
        if rel in ("sw.js", "_headers") or rel.startswith("."):
            continue
        files.append("/" + rel)
files.sort()
for f in files:
    h.update(f.encode()); h.update(open(os.path.join(out, f[1:]), "rb").read())
sw = os.path.join(out, "sw.js")
s = open(sw, encoding="utf-8").read()
s = s.replace("__SSB_CACHE__", f"{version}-{h.hexdigest()[:12]}").replace("__SSB_FILES__", json.dumps(["/"] + files, indent=2))
open(sw, "w", encoding="utf-8").write(s)
print(f"{len(files)} files, cache ssb-{version}-{h.hexdigest()[:12]}")
PY
echo "browser edition built in $OUT/ ($(du -sh "$OUT" | cut -f1))"
