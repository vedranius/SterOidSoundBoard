"use strict";
// Shared sonagram tools: colour maps, axes, colour-bar legend, FFT, and the
// live sonagram widget (LiveSono) fed by the server's live analyzer.
const Sono = (() => {
  // ------------------------------------------------------------ colour maps
  const STOPS = {
    praat: [[0, [255, 255, 255]], [1, [0, 0, 0]]], // Praat: white = quiet, black = loud
    toplo: [[0, [0, 0, 4]], [0.25, [66, 10, 104]], [0.5, [147, 38, 103]], [0.75, [221, 81, 58]], [0.9, [252, 165, 10]], [1, [252, 255, 164]]],
    viridis: [[0, [68, 1, 84]], [0.25, [59, 82, 139]], [0.5, [33, 145, 140]], [0.75, [94, 201, 98]], [1, [253, 231, 37]]],
    plavo: [[0, [0, 0, 0]], [0.45, [20, 60, 140]], [0.8, [77, 163, 255]], [1, [230, 245, 255]]],
  };
  const NAMES = { praat: "Praat (sivo)", toplo: "Toplo (inferno)", viridis: "Viridis", plavo: "Plavo" };
  const LUTS = {};
  function lut(name) {
    if (LUTS[name]) return LUTS[name];
    const st = STOPS[name] || STOPS.toplo, out = new Uint8ClampedArray(256 * 3);
    for (let i = 0; i < 256; i++) {
      const v = i / 255;
      let k = 0; while (k < st.length - 2 && v > st[k + 1][0]) k++;
      const [a, ca] = st[k], [b, cb] = st[k + 1], f = (v - a) / (b - a || 1);
      for (let c = 0; c < 3; c++) out[i * 3 + c] = ca[c] + (cb[c] - ca[c]) * Math.max(0, Math.min(1, f));
    }
    return (LUTS[name] = out);
  }

  // ------------------------------------------------------------ axes
  function niceStep(range, n) {
    const raw = range / Math.max(1, n), p = Math.pow(10, Math.floor(Math.log10(raw)));
    const m = raw / p;
    return (m < 1.5 ? 1 : m < 3 ? 2 : m < 7 ? 5 : 10) * p;
  }
  function ticks(min, max, n) {
    if (!(max > min)) return [];
    const s = niceStep(max - min, n), out = [];
    for (let v = Math.ceil(min / s) * s; v <= max + 1e-9; v += s) out.push(+v.toFixed(10));
    return out;
  }
  const fmtHz = (f) => (f >= 1000 ? (f / 1000).toFixed(f % 1000 ? 1 : 0) + "k" : Math.round(f) + "");

  /** Vertical colour bar with dB labels. */
  function colorbar(g, x, y, w, h, map, dbMin, dbMax, dpr = 1, label = "dB") {
    const L = lut(map);
    g.save(); g.textAlign = "left";
    for (let i = 0; i < h; i++) {
      const v = Math.round((1 - i / (h - 1)) * 255);
      g.fillStyle = `rgb(${L[v * 3]},${L[v * 3 + 1]},${L[v * 3 + 2]})`;
      g.fillRect(x, y + i, w, 1);
    }
    g.strokeStyle = "#555"; g.lineWidth = 1; g.strokeRect(x + 0.5, y + 0.5, w - 1, h - 1);
    g.fillStyle = "#9aa4b1"; g.font = `${10 * dpr}px system-ui`; g.textBaseline = "middle";
    for (const d of ticks(dbMin, dbMax, 5)) {
      const yy = y + (1 - (d - dbMin) / (dbMax - dbMin)) * h;
      g.fillRect(x + w, yy, 3 * dpr, 1);
      g.fillText(Math.round(d) + "", x + w + 4 * dpr, yy);
    }
    g.textBaseline = "alphabetic";
    g.fillText(label, x, y - 4 * dpr);
    g.restore();
  }

  // ------------------------------------------------------------ FFT (radix-2, in place)
  function fft(re, im) {
    const n = re.length;
    for (let i = 1, j = 0; i < n; i++) {
      let b = n >> 1; for (; j & b; b >>= 1) j ^= b; j ^= b;
      if (i < j) { let t = re[i]; re[i] = re[j]; re[j] = t; t = im[i]; im[i] = im[j]; im[j] = t; }
    }
    for (let len = 2; len <= n; len <<= 1) {
      const a = (-2 * Math.PI) / len, wr = Math.cos(a), wi = Math.sin(a), h = len >> 1;
      for (let i = 0; i < n; i += len) {
        let cr = 1, ci = 0;
        for (let k = 0; k < h; k++) {
          const p = i + k, q = p + h, tr = re[q] * cr - im[q] * ci, ti = re[q] * ci + im[q] * cr;
          re[q] = re[p] - tr; im[q] = im[p] - ti; re[p] += tr; im[p] += ti;
          const t = cr * wr - ci * wi; ci = cr * wi + ci * wr; cr = t;
        }
      }
    }
  }
  /** Praat's Gaussian spectrogram window (physical length = 2 × effective). */
  function gaussWindow(n) {
    const w = new Float32Array(n), imid = 0.5 * (n + 1), edge = Math.exp(-12);
    for (let i = 1; i <= n; i++) w[i - 1] = (Math.exp(-48 * ((i - imid) / (n + 1)) ** 2) - edge) / (1 - edge);
    return w;
  }

  // ------------------------------------------------------------ live sonagram widget
  const WINDOWS = [[0.005, "Širokopojasni (5 ms)"], [0.015, "Srednji (15 ms)"], [0.03, "Uskopojasni (30 ms)"]];
  const RANGES = [4000, 5000, 8000, 12000];

  class LiveSono {
    constructor(host, opts = {}) {
      this.opts = opts;
      this.set = Object.assign({ fmax: 5000, win: 0.005, dyn: 70, auto: true, maxDb: -20, map: "toplo", secs: 8, f0: true, db: false, f0max: 500 },
        (() => { try { return JSON.parse(localStorage.getItem("ssb.sono") || "{}"); } catch (_) { return {}; } })());
      this.frames = []; // {spec: Uint8Array, f0, db}
      this.meta = null; this.paused = false; this.hover = null; this.recentMax = 0;
      this.root = document.createElement("div"); this.root.className = "lsono";
      this.root.innerHTML = `
        <div class="lsono-bar">
          <label>Raspon <select data-k="fmax">${RANGES.map((r) => `<option value="${r}">0–${r / 1000} kHz</option>`).join("")}</select></label>
          <label>Prozor <select data-k="win">${WINDOWS.map(([v, n]) => `<option value="${v}">${n}</option>`).join("")}</select></label>
          <label>Dinamika <input data-k="dyn" type="range" min="30" max="110" step="5"><span class="mono" data-v="dyn"></span></label>
          <label class="chk"><input data-k="auto" type="checkbox"> auto razina</label>
          <label>Maks <input data-k="maxDb" type="range" min="-80" max="10" step="1"><span class="mono" data-v="maxDb"></span></label>
          <label>Boje <select data-k="map">${Object.entries(NAMES).map(([k, n]) => `<option value="${k}">${n}</option>`).join("")}</select></label>
          <label>Prikaz <select data-k="secs"><option value="4">4 s</option><option value="8">8 s</option><option value="15">15 s</option><option value="30">30 s</option></select></label>
          <label class="chk f0c"><input data-k="f0" type="checkbox"> F0</label>
          <label class="chk dbc"><input data-k="db" type="checkbox"> intenzitet</label>
          <span class="grow"></span>
          <button data-act="pause" title="Zamrzni (P)">⏸</button>
          <button data-act="png" title="Spremi sliku">📷</button>
          <button data-act="fs" title="Povećaj">⛶</button>
        </div>
        <div class="lsono-cv"><canvas></canvas></div>
        <div class="lsono-info mono"><span data-i="now"></span><span class="grow"></span><span data-i="hover" class="dim"></span></div>`;
      host.append(this.root);
      this.cv = this.root.querySelector("canvas");
      this.off = document.createElement("canvas");
      this.bindControls();
      new ResizeObserver(() => this.resize()).observe(this.root.querySelector(".lsono-cv"));
      this.cv.addEventListener("pointermove", (e) => { this.hover = this.toData(e); this.schedule(); });
      this.cv.addEventListener("pointerleave", () => { this.hover = null; this.schedule(); });
      this.raf = 0;
    }
    save() { try { localStorage.setItem("ssb.sono", JSON.stringify(this.set)); } catch (_) {} }
    schedule() { if (!this.raf) this.raf = requestAnimationFrame(() => { this.raf = 0; this.compose(); }); }
    bindControls() {
      this.root.querySelectorAll("[data-k]").forEach((el) => {
        const k = el.dataset.k;
        if (el.type === "checkbox") el.checked = !!this.set[k]; else el.value = this.set[k];
        el.oninput = el.onchange = () => {
          this.set[k] = el.type === "checkbox" ? el.checked : el.tagName === "SELECT" && k === "map" ? el.value : +el.value;
          this.save(); this.syncLabels();
          if (k === "win" && this.opts.onWindow) this.opts.onWindow(this.set.win);
          this.full();
        };
      });
      this.root.querySelector('[data-act="pause"]').onclick = () => this.togglePause();
      this.root.querySelector('[data-act="png"]').onclick = () => { const a = document.createElement("a"); a.href = this.cv.toDataURL("image/png"); a.download = `sonagram_${new Date().toISOString().slice(0, 19).replace(/[:T]/g, "-")}.png`; a.click(); };
      this.root.querySelector('[data-act="fs"]').onclick = () => { this.root.classList.toggle("fs"); this.resize(); };
      this.syncLabels();
    }
    togglePause() { this.paused = !this.paused; this.root.querySelector('[data-act="pause"]').textContent = this.paused ? "▶" : "⏸"; this.root.classList.toggle("paused", this.paused); }
    syncLabels() {
      this.root.querySelector('[data-v="dyn"]').textContent = this.set.dyn + " dB";
      this.root.querySelector('[data-v="maxDb"]').textContent = (this.set.auto ? "auto" : this.set.maxDb + " dB");
      this.root.querySelector('[data-k="maxDb"]').disabled = !!this.set.auto;
    }
    get win() { return this.set.win; }
    resize() {
      const box = this.root.querySelector(".lsono-cv"), dpr = window.devicePixelRatio || 1;
      this.dpr = dpr;
      this.cv.width = Math.max(200, box.clientWidth * dpr); this.cv.height = Math.max(120, box.clientHeight * dpr);
      this.full();
    }
    plot() { // plot rectangle inside the canvas (device px)
      const d = this.dpr || 1;
      return { x: 44 * d, y: 8 * d, w: this.cv.width - (44 + 88) * d, h: this.cv.height - (8 + 20) * d };
    }
    push(live) {
      if (!live || !live.frames) return;
      this.meta = live;
      if (this.paused) return;
      for (const f of live.frames) {
        const bin = atob(f.s), a = new Uint8Array(bin.length);
        for (let i = 0; i < bin.length; i++) a[i] = bin.charCodeAt(i);
        let m = 0; for (let i = 2; i < a.length; i++) if (a[i] > m) m = a[i];
        this.recentMax = Math.max(m, this.recentMax * 0.995);
        this.frames.push({ spec: a, f0: f.f0, db: f.db, max: m });
      }
      const keep = Math.ceil(30 / (live.hop || 0.01)) + 10;
      if (this.frames.length > keep) this.frames.splice(0, this.frames.length - keep);
      this.addColumns(live.frames.length);
      this.schedule();
    }
    levels() {
      const m = this.meta; if (!m) return [-90, -20];
      // auto: follow the recent spectral peak, but never scale digital silence up to full colour
      const top = this.set.auto ? Math.max(-70, m.db_min + this.recentMax * m.db_step) : this.set.maxDb;
      return [top - this.set.dyn, top];
    }
    column(fr, H, L, lo, hi) { // → ImageData column (1 px wide, H tall)
      const m = this.meta, col = new Uint8ClampedArray(H * 4), n = fr.spec.length;
      for (let y = 0; y < H; y++) {
        const f = (1 - (y + 0.5) / H) * this.set.fmax, k = f / m.df;
        const i0 = Math.min(n - 1, Math.floor(k)), i1 = Math.min(n - 1, i0 + 1), t = k - i0;
        const b0 = fr.spec[i0], b1 = fr.spec[i1], db = m.db_min + (b0 * (1 - t) + b1 * t) * m.db_step;
        const v = b0 === 0 && b1 === 0 ? 0 : Math.round(Math.max(0, Math.min(1, (db - lo) / (hi - lo))) * 255); // byte 0 = below −120 dB
        col[y * 4] = L[v * 3]; col[y * 4 + 1] = L[v * 3 + 1]; col[y * 4 + 2] = L[v * 3 + 2]; col[y * 4 + 3] = 255;
      }
      return new ImageData(col, 1, H);
    }
    nCols() { return Math.round(this.set.secs / ((this.meta && this.meta.hop) || 0.01)); }
    full() { // re-render the offscreen image from history
      if (!this.meta) { this.compose(); return; }
      const W = this.nCols(), H = Math.max(64, Math.round(this.plot().h));
      this.off.width = W; this.off.height = H;
      const g = this.off.getContext("2d"), L = lut(this.set.map), [lo, hi] = this.levels();
      g.fillStyle = `rgb(${L[0]},${L[1]},${L[2]})`; g.fillRect(0, 0, W, H);
      const fr = this.frames.slice(-W);
      fr.forEach((f, i) => g.putImageData(this.column(f, H, L, lo, hi), W - fr.length + i, 0));
      this.compose();
    }
    addColumns(k) {
      if (!this.off.width || this.off.height !== Math.max(64, Math.round(this.plot().h)) || this.off.width !== this.nCols()) return this.full();
      const g = this.off.getContext("2d"), W = this.off.width, H = this.off.height, L = lut(this.set.map), [lo, hi] = this.levels();
      k = Math.min(k, W);
      g.drawImage(this.off, -k, 0);
      const fr = this.frames.slice(-k);
      fr.forEach((f, i) => g.putImageData(this.column(f, H, L, lo, hi), W - k + i, 0));
    }
    compose() {
      const g = this.cv.getContext("2d"), d = this.dpr || 1, P = this.plot();
      g.fillStyle = "#0b0d10"; g.fillRect(0, 0, this.cv.width, this.cv.height);
      if (this.off.width) { g.imageSmoothingEnabled = false; g.drawImage(this.off, P.x, P.y, P.w, P.h); }
      g.strokeStyle = "#39414b"; g.strokeRect(P.x - 0.5, P.y - 0.5, P.w + 1, P.h + 1);
      g.font = `${10 * d}px system-ui`; g.fillStyle = "#9aa4b1";
      // frequency axis
      g.textAlign = "right"; g.textBaseline = "middle";
      for (const f of ticks(0, this.set.fmax, 6)) { const y = P.y + (1 - f / this.set.fmax) * P.h; g.fillRect(P.x - 4 * d, y, 4 * d, 1); g.fillText(fmtHz(f), P.x - 6 * d, y); }
      g.save(); g.translate(10 * d, P.y + P.h / 2); g.rotate(-Math.PI / 2); g.textAlign = "center"; g.fillText("Hz", 0, 0); g.restore();
      // time axis (seconds before now)
      g.textAlign = "center"; g.textBaseline = "top";
      for (const s of ticks(0, this.set.secs, 8)) { const x = P.x + P.w - (s / this.set.secs) * P.w; g.fillRect(x, P.y + P.h, 1, 4 * d); g.fillText(s ? `−${s}s` : "sada", x, P.y + P.h + 5 * d); }
      // overlays
      const n = this.nCols(), fr = this.frames.slice(-n), off = n - fr.length, xw = P.w / n;
      if (this.set.db) this.curve(g, fr, off, xw, P, (f) => f.db, 30, 100, "#ffd84d");
      if (this.set.f0) this.dots(g, fr, off, xw, P, "#4dd2ff");
      // right side: F0 scale + colour bar
      g.textAlign = "left"; g.textBaseline = "middle";
      if (this.set.f0) { g.fillStyle = "#4dd2ff"; for (const f of ticks(0, this.set.f0max, 5)) { const y = P.y + (1 - f / this.set.f0max) * P.h; g.fillRect(P.x + P.w, y, 4 * d, 1); g.fillText(f + "", P.x + P.w + 5 * d, y); } }
      const [lo, hi] = this.levels();
      colorbar(g, P.x + P.w + 42 * d, P.y + 12 * d, 10 * d, P.h - 12 * d, this.set.map, lo, hi, d, "dB");
      if (this.hover) { g.strokeStyle = "#fff8"; g.beginPath(); g.moveTo(this.hover.px, P.y); g.lineTo(this.hover.px, P.y + P.h); g.moveTo(P.x, this.hover.py); g.lineTo(P.x + P.w, this.hover.py); g.stroke(); }
      this.drawInfo();
    }
    dots(g, fr, off, xw, P, color) {
      g.fillStyle = color; const r = Math.max(1.5, (this.dpr || 1) * 1.5);
      fr.forEach((f, i) => { if (f.f0 > 0 && f.f0 < this.set.f0max) g.fillRect(P.x + (off + i + 0.5) * xw - r / 2, P.y + (1 - f.f0 / this.set.f0max) * P.h - r / 2, r, r); });
    }
    curve(g, fr, off, xw, P, get, lo, hi, color) {
      g.strokeStyle = color; g.lineWidth = 1.5 * (this.dpr || 1); g.beginPath();
      let pen = false;
      fr.forEach((f, i) => { const v = get(f); if (!(v > lo)) { pen = false; return; } const x = P.x + (off + i + 0.5) * xw, y = P.y + (1 - (Math.min(hi, v) - lo) / (hi - lo)) * P.h; pen ? g.lineTo(x, y) : g.moveTo(x, y); pen = true; });
      g.stroke();
    }
    toData(e) {
      const r = this.cv.getBoundingClientRect(), d = this.dpr || 1, P = this.plot();
      const px = (e.clientX - r.left) * d, py = (e.clientY - r.top) * d;
      if (px < P.x || px > P.x + P.w || py < P.y || py > P.y + P.h) return null;
      const n = this.nCols(), i = Math.floor((px - P.x) / P.w * n), fr = this.frames.slice(-n), k = i - (n - fr.length);
      const f = (1 - (py - P.y) / P.h) * this.set.fmax, frame = fr[k];
      let db = null;
      if (frame && this.meta) { const b = Math.min(frame.spec.length - 1, Math.round(f / this.meta.df)); db = this.meta.db_min + frame.spec[b] * this.meta.db_step; }
      return { px, py, ago: (n - i) * ((this.meta && this.meta.hop) || 0.01), f, db, frame };
    }
    drawInfo() {
      const last = this.frames[this.frames.length - 1];
      const now = this.root.querySelector('[data-i="now"]'), hv = this.root.querySelector('[data-i="hover"]');
      now.innerHTML = last ? `F0 <b>${last.f0 > 0 ? last.f0.toFixed(1) + " Hz" : "—"}</b> · intenzitet <b>${last.db > 0 ? last.db.toFixed(1) + " dB" : "—"}</b>${this.paused ? ' · <span class="warnc">ZAMRZNUTO</span>' : ""}` : "čekam signal…";
      const h = this.hover;
      hv.textContent = h ? `−${h.ago.toFixed(2)} s · ${h.f.toFixed(0)} Hz · ${h.db != null ? h.db.toFixed(1) + " dB" : ""}${h.frame && h.frame.f0 > 0 ? " · F0 " + h.frame.f0.toFixed(1) + " Hz" : ""}` : "";
    }
  }
  return { lut, ticks, fmtHz, colorbar, fft, gaussWindow, LiveSono, MAPS: NAMES };
})();
