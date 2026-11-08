// Compares the WebAssembly engine with the native build on WAV files. Same Rust
// code; only the math library differs (Rust's libm in WebAssembly vs. the
// system's), so values may differ in the last float digits: every measure and
// contour point must agree within 1e-5 relative (or 1e-9 absolute).
//   cargo build --release -p steroid-web --target wasm32-unknown-unknown
//   cargo build --release -p steroid-web --example analyze
//   node tools/check-wasm.js file1.wav [file2.wav …]
const fs = require("fs"), path = require("path"), { execFileSync } = require("child_process");
const root = path.join(__dirname, "..");
const E = require(path.join(root, "web/engine.js"));
(async () => {
  const eng = await E.load(fs.readFileSync(path.join(root, "target/wasm32-unknown-unknown/release/steroid_web.wasm")));
  let bad = 0;
  let worst = 0;
  const cmp = (a, b, where, out) => {
    if (typeof a === "number" && typeof b === "number") {
      const d = Math.abs(a - b), rel = d / Math.max(Math.abs(a), Math.abs(b), 1e-30);
      if (d > 1e-9) worst = Math.max(worst, rel);
      if (d > 1e-9 && rel > 1e-5) out.push(`${where}: ${a} ≠ ${b}`);
    } else if (Array.isArray(a) && Array.isArray(b) && a.length === b.length) a.forEach((v, i) => cmp(v, b[i], `${where}[${i}]`, out));
    else if (a && b && typeof a === "object" && typeof b === "object") new Set([...Object.keys(a), ...Object.keys(b)]).forEach((k) => cmp(a[k], b[k], `${where}.${k}`, out));
    else if (JSON.stringify(a) !== JSON.stringify(b)) out.push(`${where}: ${JSON.stringify(a)} ≠ ${JSON.stringify(b)}`);
  };
  for (const f of process.argv.slice(2)) {
    eng.loadWav(fs.readFileSync(f));
    const wasm = [eng.call("analyze"), eng.call("tracks")];
    const nat = execFileSync(path.join(root, "target/release/examples/analyze"), [f], { maxBuffer: 1 << 28 }).toString().trim().split("\n").map((l) => JSON.parse(l));
    const diffs = [];
    cmp(wasm[0], nat[0], "report", diffs);
    cmp(wasm[1], nat[1], "tracks", diffs);
    const r = wasm[0];
    console.log(`${diffs.length ? "DIFFERENT" : "ok"}  ${path.basename(f)}  F0 ${r.f0_median?.toFixed(3)}  jitter ${r.jitter_local?.toFixed(3)} %  shimmer ${r.shimmer_local?.toFixed(3)} %  HNR ${r.hnr_db?.toFixed(2)} dB  CPPS ${r.cpps?.toFixed(2)} dB`);
    if (diffs.length) { bad++; diffs.slice(0, 10).forEach((d) => console.log("   ", d)); }
  }
  console.log(`largest relative difference wasm vs native: ${worst.toExponential(2)}`);
  process.exit(bad ? 1 : 0);
})();
