"use strict";
// Praat-style sound editor for DigiLingua recordings: waveform (+ pulses),
// spectrogram (+ pitch, intensity, formants), annotation tier and time axis
// on one shared, zoomable time axis; cursor readouts; spectral slice / LTAS.
// Analysis contours come from the server's Praat-compatible engine.
const Editor = (() => {
  const $e = (root, s) => root.querySelector(s);
  const clamp = (v, a, b) => Math.max(a, Math.min(b, v));
  const DEF = {
    fmax: 5000, win: 0.005, dyn: 70, auto: true, maxDb: 0, pre: 6, map: "praat", comp: 0, shape: "gauss",
    pitchUnit: "hz", pitchStyle: "line", fShow: 5, fMaxBw: 0, fDot: 2,
    show: { spec: true, pitch: true, intensity: false, formants: true, pulses: false, slice: true },
    ivMin: 50, ivMax: 100,
  };
  const KIND_COLORS = { blok: "#e5484d", produljenje: "#f76b15", ponavljanje_glasa: "#e93d82", ponavljanje_sloga: "#d6409f", ponavljanje_rijeci: "#ab4aba",
    umetak: "#3e63dd", revizija: "#0090ff", tvrdi_pocetak: "#12a594", prekid_glasa: "#ffc53d", sapat: "#8e8c99", pratece: "#7c66dc", ostalo: "#6e7b8a" };
  let KINDS = [];
  async function kinds() { if (!KINDS.length) KINDS = await api("GET", "/api/clinic/annotation-kinds"); return KINDS; }
  const kindLabel = (k) => (KINDS.find((x) => x.id === k) || { label: k }).label;
  const uid = () => "a" + Date.now().toString(36) + Math.random().toString(36).slice(2, 6);

  class SoundEditor {
    constructor(host, opts) {
      this.opts = opts; this.rec = opts.rec;
      this.set = Object.assign(JSON.parse(JSON.stringify(DEF)), (() => { try { return JSON.parse(localStorage.getItem("ssb.editor") || "{}"); } catch (_) { return {}; } })());
      this.set.show = Object.assign({}, DEF.show, this.set.show);
      this.analysis = Object.assign({ pitch_floor: 75, pitch_ceiling: 600, max_formant: 5500, n_formants: 5, cpps: true }, opts.analysis || {});
      this.anns = JSON.parse(JSON.stringify(this.rec.annotations || []));
      this.selAnn = null; this.tracks = null; this.play = null;
      this.root = document.createElement("div"); this.root.className = "sed";
      this.root.innerHTML = `
        <div class="sed-bar row">
          <button data-a="playsel" class="pri" title="Reproduciraj odabir (razmaknica)">▶ Odabir</button>
          <button data-a="playvis" title="Reproduciraj vidljivo">▶ Vidljivo</button>
          <button data-a="stop" title="Zaustavi">■</button>
          <span class="sep"></span>
          <button data-a="zsel" title="Zumiraj na odabir">⤢ Odabir</button>
          <button data-a="zin" title="Povećaj (+)">＋</button><button data-a="zout" title="Smanji (−)">－</button>
          <button data-a="zall" title="Cijela snimka">Sve</button>
          <button data-a="left" title="Pomakni lijevo (←)">◀</button><button data-a="right" title="Pomakni desno (→)">▶</button>
          <span class="sep"></span>
          <label class="chk"><input type="checkbox" data-s="spec"> Spektrogram</label>
          <label class="chk c-pitch"><input type="checkbox" data-s="pitch"> Visina</label>
          <label class="chk c-int"><input type="checkbox" data-s="intensity"> Intenzitet</label>
          <label class="chk c-form"><input type="checkbox" data-s="formants"> Formanti</label>
          <label class="chk c-pulse"><input type="checkbox" data-s="pulses"> Pulsevi</label>
          <label class="chk"><input type="checkbox" data-s="slice"> Presjek</label>
          <span class="grow"></span>
          <button data-a="ann" title="Označi odabir (A)">＋ Oznaka</button>
          <button data-a="report" class="pri" title="Analiza odabira (Praat voice report)">Analiziraj odabir</button>
          <button data-a="settings" title="Postavke prikaza i analize">⚙</button>
        </div>
        <div class="sed-main">
          <div class="sed-cv"><canvas class="sed-canvas"></canvas><div class="sed-busy hidden">Računam konture (Praat)…</div></div>
          <div class="sed-side"><div class="h row"><span data-i="slt">Spektralni presjek</span><span class="grow"></span>
            <label class="chk small"><input type="checkbox" data-ltas> LTAS odabira</label></div><canvas class="sed-slice"></canvas><div class="sed-sliceinfo mono small"></div></div>
        </div>
        <div class="sed-info mono small"><span data-i="cur"></span><span class="grow"></span><span data-i="mouse" class="dim"></span></div>`;
      host.innerHTML = ""; host.append(this.root);
      this.cv = $e(this.root, ".sed-canvas"); this.sl = $e(this.root, ".sed-slice");
      this.bind();
      new ResizeObserver(() => this.resize()).observe($e(this.root, ".sed-cv"));
    }
    save() { try { localStorage.setItem("ssb.editor", JSON.stringify(this.set)); } catch (_) {} }

    // ---------------------------------------------------------------- loading
    async load() {
      await kinds();
      this.ctx = this.ctx || new (window.AudioContext || window.webkitAudioContext)();
      const data = await (await fetch(`/api/recordings/${this.rec.id}/wav`)).arrayBuffer();
      this.buf = await this.ctx.decodeAudioData(data);
      const b = this.buf;
      if (b.numberOfChannels < 2) this.x = b.getChannelData(0);
      else {
        const e = (d) => { let s = 0; for (let i = 0; i < d.length; i += 16) s += d[i] * d[i]; return s; };
        this.x = e(b.getChannelData(0)) >= e(b.getChannelData(1)) ? b.getChannelData(0) : b.getChannelData(1);
      }
      this.sr = b.sampleRate; this.dur = b.duration;
      this.view = [0, this.dur]; this.sel = [0, this.dur]; this.cursor = 0;
      this.resize();
      await this.loadTracks();
    }
    async loadTracks() {
      $e(this.root, ".sed-busy").classList.remove("hidden");
      try { this.tracks = await api("POST", `/api/recordings/${this.rec.id}/tracks`, { settings: this.analysis }); }
      catch (e) { toast("Konture: " + e.message); this.tracks = null; }
      $e(this.root, ".sed-busy").classList.add("hidden");
      this.specKey = null; this.draw();
    }

    // ---------------------------------------------------------------- layout
    resize() {
      const box = $e(this.root, ".sed-cv"), d = (this.dpr = window.devicePixelRatio || 1);
      this.cv.width = Math.max(300, box.clientWidth * d); this.cv.height = Math.max(300, box.clientHeight * d);
      const sb = this.sl.parentElement; this.sl.width = Math.max(150, sb.clientWidth * d); this.sl.height = 220 * d;
      this.specKey = null; this.draw();
    }
    L() { // regions in device px
      const d = this.dpr, W = this.cv.width, H = this.cv.height;
      const left = 50 * d, right = 96 * d, x = left, w = W - left - right;
      const waveH = Math.round(H * 0.22), tierH = 34 * d, axisH = 22 * d, gap = 6 * d;
      const specH = H - waveH - tierH - axisH - 3 * gap - 4 * d;
      const wave = { x, y: 4 * d, w, h: waveH };
      const spec = { x, y: wave.y + waveH + gap, w, h: specH };
      const tier = { x, y: spec.y + specH + gap, w, h: tierH };
      const axis = { x, y: tier.y + tierH + gap, w, h: axisH };
      return { wave, spec, tier, axis, d };
    }
    tx(t, r) { return r.x + ((t - this.view[0]) / (this.view[1] - this.view[0])) * r.w; }
    xt(px, r) { return this.view[0] + ((px - r.x) / r.w) * (this.view[1] - this.view[0]); }

    // ---------------------------------------------------------------- drawing
    draw() {
      if (!this.buf || this.drawing) return;
      this.drawing = requestAnimationFrame(() => { this.drawing = 0; this.paint(); });
    }
    paint() {
      const g = this.cv.getContext("2d"), R = this.L(), d = R.d;
      this.C = Sono.pal(); g.fillStyle = this.C.bg; g.fillRect(0, 0, this.cv.width, this.cv.height);
      g.font = `${10 * d}px system-ui`;
      this.paintWave(g, R.wave, d);
      this.paintSpec(g, R.spec, d);
      this.paintTier(g, R.tier, d);
      this.paintAxis(g, R.axis, d);
      // selection, cursor, play head across all panes
      const top = R.wave.y, bot = R.tier.y + R.tier.h;
      const [a, b] = this.sel;
      if (b > a) { g.fillStyle = this.C.sel; const xa = this.tx(a, R.wave), xb = this.tx(b, R.wave); g.fillRect(xa, top, xb - xa, bot - top); g.fillStyle = this.C.acc; g.fillRect(xa, top, d, bot - top); g.fillRect(xb, top, d, bot - top); }
      g.fillStyle = "#e5484d"; const xc = this.tx(this.cursor, R.wave); if (xc >= R.wave.x && xc <= R.wave.x + R.wave.w) g.fillRect(xc, top, d, bot - top);
      if (this.play) { const t = this.play.t0 + (this.ctx.currentTime - this.play.start); g.fillStyle = "#3fbf6f"; g.fillRect(this.tx(t, R.wave), top, 2 * d, bot - top); }
      if (this.mouse && this.mouse.px != null) { g.strokeStyle = this.C.cross; g.beginPath(); g.moveTo(this.mouse.px, top); g.lineTo(this.mouse.px, bot); g.stroke();
        if (this.mouse.py > R.spec.y && this.mouse.py < R.spec.y + R.spec.h) { g.beginPath(); g.moveTo(R.spec.x, this.mouse.py); g.lineTo(R.spec.x + R.spec.w, this.mouse.py); g.stroke(); } }
      this.info();
      this.paintSlice();
    }
    paintWave(g, r, d) {
      g.fillStyle = this.C.waveBg; g.fillRect(r.x, r.y, r.w, r.h);
      const [t0, t1] = this.view, x = this.x, sr = this.sr;
      const i0 = Math.max(0, Math.floor(t0 * sr)), i1 = Math.min(x.length, Math.ceil(t1 * sr));
      let peak = 1e-4; for (let i = i0; i < i1; i += Math.max(1, ((i1 - i0) / 20000) | 0)) peak = Math.max(peak, Math.abs(x[i]));
      const mid = r.y + r.h / 2, sc = (r.h / 2 - 2) / peak;
      g.fillStyle = this.C.wave; const W = Math.round(r.w), spp = (i1 - i0) / W;
      if (spp > 1.5) {
        for (let px = 0; px < W; px++) {
          let mn = 1, mx = -1; const a = i0 + Math.floor(px * spp), b = Math.min(i1, i0 + Math.floor((px + 1) * spp));
          for (let i = a; i < b; i++) { const v = x[i]; if (v < mn) mn = v; if (v > mx) mx = v; }
          if (mx >= mn) g.fillRect(r.x + px, mid - mx * sc, 1, Math.max(1, (mx - mn) * sc));
        }
      } else {
        g.strokeStyle = this.C.wave; g.lineWidth = d; g.beginPath();
        for (let i = i0; i < i1; i++) { const px = this.tx((i + 0.5) / sr, r), py = mid - x[i] * sc; i === i0 ? g.moveTo(px, py) : g.lineTo(px, py); }
        g.stroke();
      }
      g.fillStyle = this.C.grid; g.fillRect(r.x, mid, r.w, 1);
      if (this.set.show.pulses && this.tracks) {
        g.fillStyle = "#4dd2ffcc";
        for (const p of this.tracks.pulses) if (p >= t0 && p <= t1) g.fillRect(this.tx(p, r), r.y, 1, r.h);
      }
      g.fillStyle = this.C.text; g.textAlign = "right"; g.textBaseline = "middle";
      g.fillText(peak.toFixed(peak < 0.1 ? 3 : 2), r.x - 4 * d, r.y + 6 * d); g.fillText("0", r.x - 4 * d, mid); g.fillText((-peak).toFixed(peak < 0.1 ? 3 : 2), r.x - 4 * d, r.y + r.h - 6 * d);
    }
    computeSpec(r) {
      const key = [this.view[0], this.view[1], Math.round(r.w), Math.round(r.h), this.set.fmax, this.set.win, this.set.pre, this.set.shape, this.set.comp].join("|");
      if (key === this.specKey) return;
      this.specKey = key;
      const cols = Math.min(1600, Math.round(r.w)), rows = Math.round(r.h), sr = this.sr, x = this.x;
      // Praat: physical window = 2 × effective length for the Gaussian; Hann uses the effective length
      const n = Math.max(16, Math.round((this.set.shape === "hann" ? 1 : 2) * this.set.win * sr));
      const win = this.set.shape === "hann" ? Float32Array.from({ length: n }, (_, i) => 0.5 - 0.5 * Math.cos((2 * Math.PI * (i + 1)) / (n + 1))) : Sono.gaussWindow(n);
      let N = 256; while (N < n) N *= 2; if (this.set.win >= 0.02 && N < 2048) N = 2048;
      const re = new Float32Array(N), im = new Float32Array(N), df = sr / N;
      let wsum = 0; for (const v of win) wsum += v;
      const out = new Float32Array(cols * rows), kmax = Math.min(N / 2 - 1, Math.ceil(this.set.fmax / df) + 1);
      const colDb = new Float32Array(kmax + 1), colMax = new Float32Array(cols).fill(-300);
      let top = -300;
      for (let c = 0; c < cols; c++) {
        const t = this.view[0] + ((c + 0.5) / cols) * (this.view[1] - this.view[0]);
        const s0 = Math.round(t * sr - n / 2);
        let mean = 0; for (let i = 0; i < n; i++) { const k = s0 + i; mean += k >= 0 && k < x.length ? x[k] : 0; } mean /= n;
        re.fill(0); im.fill(0);
        for (let i = 0; i < n; i++) { const k = s0 + i; re[i] = ((k >= 0 && k < x.length ? x[k] : 0) - mean) * win[i]; }
        Sono.fft(re, im);
        for (let k = 0; k <= kmax; k++) {
          const f = k * df, p = (re[k] * re[k] + im[k] * im[k]) * (4 / (wsum * wsum));
          let db = 10 * Math.log10(p + 1e-20);
          if (this.set.pre && f > 0) db += this.set.pre * Math.log2(f / 1000);
          colDb[k] = db;
        }
        for (let y = 0; y < rows; y++) {
          const f = (1 - (y + 0.5) / rows) * this.set.fmax, kk = f / df, k0 = Math.floor(kk), fr = kk - k0;
          const v = colDb[k0] * (1 - fr) + colDb[Math.min(kmax, k0 + 1)] * fr;
          out[y * cols + c] = v; if (v > top) top = v; if (v > colMax[c]) colMax[c] = v;
        }
      }
      // Praat's dynamic compression: raise quiet columns towards the loudest one
      if (this.set.comp > 0) for (let c = 0; c < cols; c++) { const add = this.set.comp * (top - colMax[c]); for (let y = 0; y < rows; y++) out[y * cols + c] += add; }
      this.spec = { cols, rows, db: out, top };
    }
    levels() { const top = this.set.auto ? (this.spec ? this.spec.top : 0) : this.set.maxDb; return [top - this.set.dyn, top]; }
    paintSpec(g, r, d) {
      if (!this.set.show.spec) { g.fillStyle = this.C.lane; g.fillRect(r.x, r.y, r.w, r.h); }
      else {
        this.computeSpec(r);
        const s = this.spec, [lo, hi] = this.levels(), L = Sono.lut(this.set.map), img = new ImageData(s.cols, s.rows);
        for (let i = 0; i < s.db.length; i++) { const v = Math.round(clamp((s.db[i] - lo) / (hi - lo), 0, 1) * 255); img.data[i * 4] = L[v * 3]; img.data[i * 4 + 1] = L[v * 3 + 1]; img.data[i * 4 + 2] = L[v * 3 + 2]; img.data[i * 4 + 3] = 255; }
        const tmp = this.tmp || (this.tmp = document.createElement("canvas")); tmp.width = s.cols; tmp.height = s.rows; tmp.getContext("2d").putImageData(img, 0, 0);
        g.imageSmoothingEnabled = true; g.drawImage(tmp, r.x, r.y, r.w, r.h);
        Sono.colorbar(g, r.x + r.w + 56 * d, r.y + 14 * d, 10 * d, r.h - 14 * d, this.set.map, lo, hi, d, "dB");
      }
      g.strokeStyle = this.C.grid; g.strokeRect(r.x - 0.5, r.y - 0.5, r.w + 1, r.h + 1);
      g.fillStyle = this.C.text; g.textAlign = "right"; g.textBaseline = "middle";
      for (const f of Sono.ticks(0, this.set.fmax, 6)) { const y = r.y + (1 - f / this.set.fmax) * r.h; g.fillRect(r.x - 4 * d, y, 4 * d, 1); g.fillText(Sono.fmtHz(f), r.x - 6 * d, y); }
      g.save(); g.translate(10 * d, r.y + r.h / 2); g.rotate(-Math.PI / 2); g.textAlign = "center"; g.fillText("frekvencija (Hz)", 0, 0); g.restore();
      const T = this.tracks; if (!T) return;
      const [t0, t1] = this.view, sh = this.set.show;
      if (sh.formants) {
        g.fillStyle = "#e03131"; const rr = Math.max(1.5, this.set.fDot * d), nmax = 1 + 2 * this.set.fShow, bwMax = this.set.fMaxBw;
        for (const fr of T.formants) { if (fr[0] < t0 || fr[0] > t1) continue; const px = this.tx(fr[0], r);
          for (let k = 1; k < Math.min(fr.length, nmax); k += 2) { const f = fr[k]; if (f > 0 && f < this.set.fmax && !(bwMax > 0 && fr[k + 1] > bwMax)) g.fillRect(px - rr / 2, r.y + (1 - f / this.set.fmax) * r.h - rr / 2, rr, rr); } }
      }
      if (sh.intensity) {
        const lo = this.set.ivMin, hi = this.set.ivMax;
        g.strokeStyle = "#d49b00"; g.lineWidth = 1.6 * d; g.beginPath(); let pen = false;
        for (const [t, db] of T.intensity) { if (t < t0 || t > t1) { pen = false; continue; } const y = r.y + (1 - (clamp(db, lo, hi) - lo) / (hi - lo)) * r.h, px = this.tx(t, r); pen ? g.lineTo(px, y) : g.moveTo(px, y); pen = true; }
        g.stroke();
        g.fillStyle = "#d49b00"; g.textAlign = "left";
        for (const v of Sono.ticks(lo, hi, 4)) { const y = r.y + (1 - (v - lo) / (hi - lo)) * r.h; g.fillRect(r.x + r.w, y, 3 * d, 1); g.fillText(v + " dB", r.x + r.w + 4 * d, y); }
      }
      if (sh.pitch) {
        // pitch axis in Hz (linear) or semitones re 100 Hz (logarithmic), as Praat's Pitch settings
        const st = this.set.pitchUnit === "st", u = (f) => (st ? 12 * Math.log2(f / 100) : f);
        const lo = u(this.analysis.pitch_floor), hi = u(this.analysis.pitch_ceiling);
        const Y = (f) => r.y + (1 - (clamp(u(f), lo, hi) - lo) / (hi - lo)) * r.h;
        g.strokeStyle = "#1f8fff"; g.fillStyle = "#1f8fff";
        if (this.set.pitchStyle === "dots") {
          const rr = Math.max(2, 2.4 * d);
          for (const [t, f] of T.pitch) if (t >= t0 && t <= t1 && f > 0) { g.beginPath(); g.arc(this.tx(t, r), Y(f), rr, 0, 7); g.fill(); }
        } else {
          g.lineWidth = 2.5 * d; g.beginPath(); let pen = false;
          for (const [t, f] of T.pitch) {
            if (t < t0 - 0.02 || t > t1 + 0.02 || !(f > 0)) { pen = false; continue; }
            const y = Y(f), px = this.tx(t, r); pen ? g.lineTo(px, y) : g.moveTo(px, y); pen = true;
          }
          g.stroke(); g.strokeStyle = this.C.bg; g.lineWidth = 0.8 * d; g.stroke();
        }
        g.fillStyle = "#1f8fff"; g.textAlign = "left";
        for (const v of Sono.ticks(lo, hi, 4)) { const y = r.y + (1 - (v - lo) / (hi - lo)) * r.h; g.fillRect(r.x + r.w, y, 3 * d, 1); g.fillText(st ? v + " st" : v + " Hz", r.x + r.w + 4 * d, y + (sh.intensity ? 10 * d : 0)); }
      }
    }
    paintTier(g, r, d) {
      g.fillStyle = this.C.tier; g.fillRect(r.x, r.y, r.w, r.h);
      g.strokeStyle = this.C.grid; g.strokeRect(r.x - 0.5, r.y - 0.5, r.w + 1, r.h + 1);
      g.fillStyle = this.C.text; g.textAlign = "right"; g.textBaseline = "middle"; g.fillText("oznake", r.x - 6 * d, r.y + r.h / 2);
      g.textAlign = "left";
      for (const a of this.anns) {
        if (a.end < this.view[0] || a.start > this.view[1]) continue;
        const xa = Math.max(r.x, this.tx(a.start, r)), xb = Math.min(r.x + r.w, this.tx(a.end, r));
        const col = KIND_COLORS[a.kind] || "#6e7b8a";
        g.fillStyle = col + (this.selAnn === a.id ? "ff" : "aa"); g.fillRect(xa, r.y + 3 * d, Math.max(2, xb - xa), r.h - 6 * d);
        if (this.selAnn === a.id) { g.strokeStyle = "#fff"; g.lineWidth = 2 * d; g.strokeRect(xa, r.y + 3 * d, Math.max(2, xb - xa), r.h - 6 * d); }
        const label = kindLabel(a.kind) + (a.text ? ": " + a.text : "");
        g.save(); g.beginPath(); g.rect(xa + 2 * d, r.y, Math.max(0, xb - xa - 4 * d), r.h); g.clip(); g.fillStyle = "#fff"; g.fillText(label, xa + 4 * d, r.y + r.h / 2); g.restore();
      }
    }
    paintAxis(g, r, d) {
      const [t0, t1] = this.view;
      g.fillStyle = this.C.text; g.textAlign = "center"; g.textBaseline = "top";
      const tk = Sono.ticks(t0, t1, Math.max(4, Math.round(r.w / (90 * d)))), dec = Math.max(0, Math.min(4, Math.ceil(-Math.log10((t1 - t0) / 10))));
      for (const t of tk) { const x = this.tx(t, r); g.fillRect(x, r.y, 1, 4 * d); g.fillText(t.toFixed(dec) + " s", x, r.y + 6 * d); }
    }

    // ---------------------------------------------------------------- readouts
    trackAt(arr, t) { // nearest frame
      if (!arr || !arr.length) return null;
      let lo = 0, hi = arr.length - 1;
      while (hi - lo > 1) { const m = (lo + hi) >> 1; if (arr[m][0] < t) lo = m; else hi = m; }
      return Math.abs(arr[lo][0] - t) < Math.abs(arr[hi][0] - t) ? arr[lo] : arr[hi];
    }
    pitchAt(t) { const p = this.trackAt(this.tracks && this.tracks.pitch, t); return p && Math.abs(p[0] - t) < 0.02 && p[1] > 0 ? p[1] : null; }
    info() {
      const [a, b] = this.sel;
      $e(this.root, '[data-i="cur"]').textContent =
        `kursor ${this.cursor.toFixed(3)} s · odabir ${a.toFixed(3)}–${b.toFixed(3)} s (${(b - a).toFixed(3)} s) · prikaz ${this.view[0].toFixed(2)}–${this.view[1].toFixed(2)} s`;
      const m = this.mouse, el = $e(this.root, '[data-i="mouse"]');
      if (!m || m.t == null) { el.textContent = ""; return; }
      const parts = [`${m.t.toFixed(3)} s`];
      if (m.f != null) parts.push(`${m.f.toFixed(0)} Hz${m.db != null ? " · " + m.db.toFixed(1) + " dB" : ""}`);
      const f0 = this.pitchAt(m.t); if (f0) parts.push(`F0 ${f0.toFixed(1)} Hz`);
      const it = this.trackAt(this.tracks && this.tracks.intensity, m.t); if (it) parts.push(`int. ${it[1].toFixed(1)} dB`);
      const fr = this.trackAt(this.tracks && this.tracks.formants, m.t);
      if (fr && Math.abs(fr[0] - m.t) < 0.02) parts.push([1, 3, 5, 7].map((k, i) => fr[k] > 0 ? `F${i + 1} ${Math.round(fr[k])}` : "").filter(Boolean).join(" "));
      el.textContent = parts.join(" · ");
    }
    paintSlice() {
      const side = $e(this.root, ".sed-side");
      side.classList.toggle("hidden", !this.set.show.slice);
      if (!this.set.show.slice || !this.buf) return;
      const g = this.sl.getContext("2d"), d = this.dpr, W = this.sl.width, H = this.sl.height;
      g.fillStyle = this.C.lane; g.fillRect(0, 0, W, H);
      const ltas = $e(this.root, "[data-ltas]").checked, sr = this.sr, x = this.x;
      const n = Math.round(Math.max(0.04, 2 * this.set.win) * sr), win = Sono.gaussWindow(n);
      let N = 1024; while (N < n) N *= 2;
      const re = new Float32Array(N), im = new Float32Array(N), acc = new Float32Array(N / 2), df = sr / N;
      let wsum = 0; for (const v of win) wsum += v;
      const frameAt = (t) => { re.fill(0); im.fill(0); const s0 = Math.round(t * sr - n / 2); for (let i = 0; i < n; i++) { const k = s0 + i; re[i] = (k >= 0 && k < x.length ? x[k] : 0) * win[i]; } Sono.fft(re, im); for (let k = 0; k < N / 2; k++) acc[k] += (re[k] * re[k] + im[k] * im[k]) * 4 / (wsum * wsum); };
      let count = 0;
      if (ltas && this.sel[1] - this.sel[0] > 0.05) { for (let t = this.sel[0]; t <= this.sel[1]; t += 0.01) { frameAt(t); count++; if (count > 3000) break; } }
      else { frameAt(this.cursor); count = 1; }
      const kmax = Math.min(N / 2 - 1, Math.ceil(this.set.fmax / df));
      const db = new Float32Array(kmax + 1); let top = -300;
      for (let k = 0; k <= kmax; k++) { db[k] = 10 * Math.log10(acc[k] / count + 1e-20); if (k > 0 && db[k] > top) top = db[k]; }
      const lo = top - 80, P = { x: 34 * d, y: 6 * d, w: W - 40 * d, h: H - 26 * d };
      g.strokeStyle = this.C.grid; g.strokeRect(P.x, P.y, P.w, P.h);
      g.fillStyle = this.C.text; g.font = `${9 * d}px system-ui`; g.textAlign = "right"; g.textBaseline = "middle";
      for (const v of Sono.ticks(lo, top, 4)) { const y = P.y + (1 - (v - lo) / (top - lo)) * P.h; g.fillRect(P.x - 3 * d, y, 3 * d, 1); g.fillText(Math.round(v) + "", P.x - 4 * d, y); }
      g.textAlign = "center"; g.textBaseline = "top";
      for (const f of Sono.ticks(0, this.set.fmax, 4)) { const px = P.x + (f / this.set.fmax) * P.w; g.fillRect(px, P.y + P.h, 1, 3 * d); g.fillText(Sono.fmtHz(f), px, P.y + P.h + 4 * d); }
      g.strokeStyle = this.C.wave; g.lineWidth = 1.2 * d; g.beginPath();
      for (let k = 0; k <= kmax; k++) { const px = P.x + (k * df / this.set.fmax) * P.w, py = P.y + (1 - (clamp(db[k], lo, top) - lo) / (top - lo)) * P.h; k ? g.lineTo(px, py) : g.moveTo(px, py); }
      g.stroke();
      const info = [];
      if (!ltas) {
        const fr = this.trackAt(this.tracks && this.tracks.formants, this.cursor);
        if (fr && Math.abs(fr[0] - this.cursor) < 0.02) {
          g.fillStyle = "#ff3b3b";
          for (let k = 1, i = 1; k < fr.length && i <= 4; k += 2, i++) if (fr[k] > 0 && fr[k] < this.set.fmax) { const px = P.x + (fr[k] / this.set.fmax) * P.w; g.fillRect(px, P.y, d, P.h); info.push(`F${i} ${Math.round(fr[k])} Hz (B ${Math.round(fr[k + 1])})`); }
        }
        const f0 = this.pitchAt(this.cursor); if (f0) info.unshift(`F0 ${f0.toFixed(1)} Hz`);
      } else info.push(`LTAS ${count} okvira · ${(this.sel[1] - this.sel[0]).toFixed(2)} s`);
      $e(this.root, '[data-i="slt"]').textContent = ltas ? "LTAS odabira" : `Spektralni presjek @ ${this.cursor.toFixed(3)} s`;
      $e(this.root, ".sed-sliceinfo").textContent = info.join(" · ");
    }

    // ---------------------------------------------------------------- interaction
    bind() {
      const R = () => this.L();
      this.root.querySelectorAll("[data-s]").forEach((c) => { c.checked = !!this.set.show[c.dataset.s]; c.onchange = () => { this.set.show[c.dataset.s] = c.checked; this.save(); this.specKey = null; this.draw(); }; });
      $e(this.root, "[data-ltas]").onchange = () => this.draw();
      const act = (a) => {
        const [v0, v1] = this.view, w = v1 - v0, c = (v0 + v1) / 2;
        if (a === "playsel") this.startPlay(this.sel[0], this.sel[1] > this.sel[0] ? this.sel[1] : this.view[1]);
        else if (a === "playvis") this.startPlay(v0, v1);
        else if (a === "stop") this.stopPlay();
        else if (a === "zsel" && this.sel[1] - this.sel[0] > 0.005) this.setView(this.sel[0], this.sel[1]);
        else if (a === "zin") { const m = this.cursor >= v0 && this.cursor <= v1 ? this.cursor : c; this.setView(m - w / 4, m + w / 4); }
        else if (a === "zout") this.setView(c - w, c + w);
        else if (a === "zall") this.setView(0, this.dur);
        else if (a === "left") this.setView(v0 - w / 2, v1 - w / 2);
        else if (a === "right") this.setView(v0 + w / 2, v1 + w / 2);
        else if (a === "ann") this.addAnnotation();
        else if (a === "report") this.opts.onAnalyze && this.opts.onAnalyze(this.sel[1] - this.sel[0] > 0.1 ? this.sel : [0, this.dur], this.analysis);
        else if (a === "settings") this.openSettings();
      };
      this.root.querySelectorAll("[data-a]").forEach((b) => (b.onclick = () => act(b.dataset.a)));
      this.act = act;
      const cv = this.cv;
      const pos = (e) => { const r = cv.getBoundingClientRect(); return { px: (e.clientX - r.left) * this.dpr, py: (e.clientY - r.top) * this.dpr }; };
      cv.addEventListener("pointerdown", (e) => {
        if (!this.buf) return;
        const { px, py } = pos(e), L = R();
        if (px < L.wave.x || px > L.wave.x + L.wave.w) return;
        cv.setPointerCapture(e.pointerId);
        const t = clamp(this.xt(px, L.wave), 0, this.dur);
        if (py >= L.tier.y && py <= L.tier.y + L.tier.h) {
          const hit = this.anns.find((a) => t >= a.start && t <= a.end);
          this.selAnn = hit ? hit.id : null;
          if (hit && e.detail >= 2) this.editAnnotation(hit);
          if (hit) { this.sel = [hit.start, hit.end]; this.cursor = hit.start; this.draw(); return; }
        }
        this.drag = { t0: t }; this.cursor = t; this.sel = [t, t]; this.draw();
      });
      cv.addEventListener("pointermove", (e) => {
        if (!this.buf) return;
        const { px, py } = pos(e), L = R();
        const inX = px >= L.wave.x && px <= L.wave.x + L.wave.w;
        const t = inX ? clamp(this.xt(px, L.wave), 0, this.dur) : null;
        let f = null, db = null;
        if (inX && py >= L.spec.y && py <= L.spec.y + L.spec.h) {
          f = (1 - (py - L.spec.y) / L.spec.h) * this.set.fmax;
          if (this.spec && this.set.show.spec) { const c = Math.floor(((px - L.spec.x) / L.spec.w) * this.spec.cols), rr = Math.floor(((py - L.spec.y) / L.spec.h) * this.spec.rows); db = this.spec.db[clamp(rr, 0, this.spec.rows - 1) * this.spec.cols + clamp(c, 0, this.spec.cols - 1)]; }
        }
        this.mouse = inX ? { px, py, t, f, db } : null;
        if (this.drag && t != null) { this.sel = [Math.min(this.drag.t0, t), Math.max(this.drag.t0, t)]; }
        this.draw();
      });
      const up = () => { if (!this.drag) return; this.drag = null; if (this.sel[1] - this.sel[0] < 0.003) this.sel = [this.cursor, this.cursor]; else this.cursor = this.sel[0]; this.draw(); };
      cv.addEventListener("pointerup", up); cv.addEventListener("pointercancel", up);
      cv.addEventListener("pointerleave", () => { this.mouse = null; this.draw(); });
      cv.addEventListener("wheel", (e) => {
        if (!this.buf) return; e.preventDefault();
        const L = R(), { px } = pos(e), [v0, v1] = this.view, w = v1 - v0;
        if (e.shiftKey || Math.abs(e.deltaX) > Math.abs(e.deltaY)) { const dt = ((e.shiftKey ? e.deltaY : e.deltaX) / 500) * w; this.setView(v0 + dt, v1 + dt); return; }
        const m = clamp(this.xt(px, L.wave), v0, v1), k = Math.exp(e.deltaY / 400);
        this.setView(m - (m - v0) * k, m + (v1 - m) * k);
      }, { passive: false });
      this.keys = (e) => {
        if (!this.buf || !this.root.isConnected || /INPUT|TEXTAREA|SELECT/.test(document.activeElement.tagName)) return;
        const k = e.key;
        if (k === " " || k === "Tab") { e.preventDefault(); this.play ? this.stopPlay() : act("playsel"); }
        else if (k === "+" || k === "=") act("zin"); else if (k === "-") act("zout");
        else if (k === "ArrowLeft") act("left"); else if (k === "ArrowRight") act("right");
        else if (k === "a" || k === "A") act("ann");
        else if ((k === "Delete" || k === "Backspace") && this.selAnn) { this.anns = this.anns.filter((a) => a.id !== this.selAnn); this.selAnn = null; this.saveAnns(); this.draw(); }
      };
      document.addEventListener("keydown", this.keys);
    }
    destroy() { this.stopPlay(); document.removeEventListener("keydown", this.keys); }
    setView(a, b) {
      const w = clamp(b - a, 0.002, this.dur);
      a = clamp(a, 0, this.dur - w); this.view = [a, a + w]; this.draw();
    }
    startPlay(a, b) {
      this.stopPlay(); if (!(b > a)) return;
      const s = this.ctx.createBufferSource(); s.buffer = this.buf; s.connect(this.ctx.destination);
      this.ctx.resume(); s.start(0, a, b - a);
      this.play = { src: s, t0: a, start: this.ctx.currentTime };
      s.onended = () => { this.play = null; this.draw(); };
      const tick = () => { if (!this.play) return; this.draw(); requestAnimationFrame(tick); }; tick();
    }
    stopPlay() { if (this.play) { try { this.play.src.stop(); } catch (_) {} this.play = null; this.draw(); } }

    // ---------------------------------------------------------------- annotations
    saveAnns() {
      clearTimeout(this.annT);
      this.annT = setTimeout(async () => {
        try { await api("PUT", "/api/recordings/" + this.rec.id, { annotations: this.anns }); this.rec.annotations = this.anns; this.opts.onAnnotations && this.opts.onAnnotations(this.anns); }
        catch (e) { toast(e.message); }
      }, 300);
    }
    annDialog(a, title) {
      return new Promise((resolve) => {
        const dlg = document.createElement("div"); dlg.className = "modal"; dlg.style.zIndex = 70;
        dlg.innerHTML = `<form class="modal-in card2 small-dlg"><h3>${title}</h3>
          <label>Vrsta <select name="kind">${KINDS.map((k) => `<option value="${k.id}">${k.label}${k.sld ? " (SLD)" : ""}</option>`).join("")}</select></label>
          <label>Napomena <input name="text" maxlength="500" placeholder="npr. glas /p/, 3 ponavljanja"></label>
          <div class="dim small">${a.start.toFixed(3)}–${a.end.toFixed(3)} s (${(a.end - a.start).toFixed(3)} s)</div>
          <div class="row"><button class="pri" type="submit">Spremi</button><button type="button" data-x>Odustani</button><span class="grow"></span>${a.id ? '<button type="button" class="danger" data-del>Obriši</button>' : ""}</div></form>`;
        document.body.append(dlg);
        const f = dlg.querySelector("form"); f.kind.value = a.kind || "blok"; f.text.value = a.text || ""; f.kind.focus();
        const close = (v) => { dlg.remove(); resolve(v); };
        f.onsubmit = (e) => { e.preventDefault(); close({ kind: f.kind.value, text: f.text.value.trim() }); };
        dlg.querySelector("[data-x]").onclick = () => close(null);
        const del = dlg.querySelector("[data-del]"); if (del) del.onclick = () => close("delete");
      });
    }
    async addAnnotation() {
      const [a, b] = this.sel;
      if (!(b - a > 0.01)) return toast("Prvo označite odsječak mišem (povucite u prikazu)");
      const v = await this.annDialog({ start: a, end: b }, "Nova oznaka");
      if (!v || v === "delete") return;
      const ann = { id: uid(), start: +a.toFixed(4), end: +b.toFixed(4), kind: v.kind, text: v.text };
      this.anns.push(ann); this.anns.sort((x, y) => x.start - y.start); this.selAnn = ann.id; this.saveAnns(); this.draw();
    }
    async editAnnotation(a) {
      const v = await this.annDialog(a, "Uredi oznaku");
      if (!v) return;
      if (v === "delete") this.anns = this.anns.filter((x) => x.id !== a.id);
      else Object.assign(a, v);
      this.saveAnns(); this.draw();
    }

    // ---------------------------------------------------------------- settings
    /** Praat-style settings (spectrogram, pitch, formants, intensity) as a form in `host`. */
    settingsForm(host, done) {
      const s = this.set, an = this.analysis, sr = this.sr || 44100;
      host.innerHTML = `<form class="set-form">
        <fieldset><legend>Spektrogram</legend>
          <label>Raspon prikaza do (Hz) <input name="fmax" type="number" min="1000" max="${Math.floor(sr / 2)}" step="500"></label>
          <label>Duljina prozora (s) <select name="win"><option value="0.003">0,003</option><option value="0.005">0,005 — širokopojasni</option><option value="0.01">0,010</option><option value="0.015">0,015</option><option value="0.03">0,030 — uskopojasni</option><option value="0.05">0,050</option></select></label>
          <label>Oblik prozora <select name="shape"><option value="gauss">Gaussov (Praat)</option><option value="hann">Hann</option></select></label>
          <label>Dinamički raspon (dB) <input name="dyn" type="number" min="20" max="150" step="5"></label>
          <label>Autoskaliranje <input name="auto" type="checkbox"></label>
          <label>Maksimum (dB) <input name="maxDb" type="number" step="1"></label>
          <label>Pre-emphasis (dB/okt) <input name="pre" type="number" min="0" max="12" step="1"></label>
          <label>Dinamička kompresija (0–1) <input name="comp" type="number" min="0" max="1" step="0.1"></label>
          <label>Boje <select name="map">${Object.entries(Sono.MAPS).map(([k, n]) => `<option value="${k}">${n}</option>`).join("")}</select></label>
        </fieldset>
        <fieldset><legend>Visina tona (Pitch)</legend>
          <div class="row small"><button type="button" data-p="60,300">Muški 60–300</button><button type="button" data-p="100,500">Ženski 100–500</button><button type="button" data-p="150,700">Dječji 150–700</button><button type="button" data-p="75,600">Standard 75–600</button></div>
          <label>Donja granica (Hz) <input name="pf" type="number" min="30" max="500"></label>
          <label>Gornja granica (Hz) <input name="pc" type="number" min="100" max="1500"></label>
          <label>Jedinica prikaza <select name="pitchUnit"><option value="hz">Hz</option><option value="st">polutonovi (re 100 Hz)</option></select></label>
          <label>Crtanje <select name="pitchStyle"><option value="line">linija</option><option value="dots">točke (speckles)</option></select></label>
          <div class="dim small">Granice mijenjaju i mjerenje (Praat „pitch floor/ceiling“).</div>
        </fieldset>
        <fieldset><legend>Formanti (Burg)</legend>
          <div class="row small"><button type="button" data-f="5000">Muški 5000</button><button type="button" data-f="5500">Ženski 5500</button><button type="button" data-f="8000">Dječji 8000</button></div>
          <label>Maksimalni formant (Hz) <input name="mf" type="number" min="2000" max="8000" step="100"></label>
          <label>Broj formanata <input name="nf" type="number" min="3" max="7" step="0.5"></label>
          <label>Prikaži formanata <select name="fShow"><option>3</option><option>4</option><option>5</option><option>6</option></select></label>
          <label>Samo širina pojasa do (Hz, 0 = sve) <input name="fMaxBw" type="number" min="0" max="2000" step="50"></label>
          <label>Veličina točke <input name="fDot" type="number" min="1" max="6" step="0.5"></label>
        </fieldset>
        <fieldset><legend>Intenzitet (prikaz)</legend>
          <label>Od (dB) <input name="ivMin" type="number" step="5"></label>
          <label>Do (dB) <input name="ivMax" type="number" step="5"></label>
          <label>CPPS u analizi <input name="cpps" type="checkbox"></label>
        </fieldset>
        <div class="row" style="grid-column:1/-1"><button class="pri" type="submit">Primijeni</button>${done ? '<button type="button" data-x>Odustani</button>' : ""}<button type="button" data-reset>Zadano</button>
          <span class="dim small">Postavke prikaza pamte se u ovom pregledniku; granice visine i formanata vrijede za ovu snimku (zadane: ⚙ Postavke).</span></div></form>`;
      const f = host.querySelector("form");
      const fill = () => {
        f.fmax.value = s.fmax; f.win.value = s.win; f.shape.value = s.shape; f.dyn.value = s.dyn; f.auto.checked = s.auto; f.maxDb.value = Math.round(s.auto ? this.levels()[1] : s.maxDb);
        f.pre.value = s.pre; f.comp.value = s.comp; f.map.value = s.map; f.pitchUnit.value = s.pitchUnit; f.pitchStyle.value = s.pitchStyle;
        f.pf.value = this.analysis.pitch_floor; f.pc.value = this.analysis.pitch_ceiling; f.mf.value = this.analysis.max_formant; f.nf.value = this.analysis.n_formants;
        f.fShow.value = s.fShow; f.fMaxBw.value = s.fMaxBw; f.fDot.value = s.fDot; f.ivMin.value = s.ivMin; f.ivMax.value = s.ivMax; f.cpps.checked = this.analysis.cpps !== false;
      };
      fill();
      host.querySelectorAll("[data-p]").forEach((b) => (b.onclick = () => { const [a, c] = b.dataset.p.split(","); f.pf.value = a; f.pc.value = c; }));
      host.querySelectorAll("[data-f]").forEach((b) => (b.onclick = () => { f.mf.value = b.dataset.f; }));
      const x = host.querySelector("[data-x]"); if (x) x.onclick = () => done && done();
      host.querySelector("[data-reset]").onclick = () => { const keep = { show: s.show }; Object.assign(s, JSON.parse(JSON.stringify(DEF)), keep); fill(); };
      f.onsubmit = async (e) => {
        e.preventDefault();
        Object.assign(s, { fmax: clamp(+f.fmax.value, 1000, sr / 2), win: +f.win.value, shape: f.shape.value, dyn: +f.dyn.value, auto: f.auto.checked, maxDb: +f.maxDb.value, pre: +f.pre.value,
          comp: clamp(+f.comp.value || 0, 0, 1), map: f.map.value, pitchUnit: f.pitchUnit.value, pitchStyle: f.pitchStyle.value, fShow: +f.fShow.value, fMaxBw: +f.fMaxBw.value || 0, fDot: +f.fDot.value || 2,
          ivMin: +f.ivMin.value, ivMax: +f.ivMax.value });
        const na = { ...this.analysis, pitch_floor: +f.pf.value, pitch_ceiling: +f.pc.value, max_formant: +f.mf.value, n_formants: +f.nf.value, cpps: f.cpps.checked };
        const changed = ["pitch_floor", "pitch_ceiling", "max_formant", "n_formants", "cpps"].some((k) => na[k] !== this.analysis[k]);
        this.analysis = na; this.save(); this.specKey = null; this.draw(); if (done) done();
        if (changed) { await this.loadTracks(); this.opts.onSettings && this.opts.onSettings(na); }
      };
    }
    openSettings() {
      const dlg = document.createElement("div"); dlg.className = "modal"; dlg.style.zIndex = 70;
      dlg.innerHTML = `<div class="modal-in card2 set-dlg"><h3>Postavke prikaza i analize</h3><div></div></div>`;
      document.body.append(dlg);
      this.settingsForm(dlg.querySelector(".set-dlg > div"), () => dlg.remove());
    }
    /** Re-render (after a theme change). */
    redraw() { this.specKey = null; this.draw(); }
    selectAnnotation(id) { const a = this.anns.find((x) => x.id === id); if (!a) return; this.selAnn = id; this.sel = [a.start, a.end]; this.cursor = a.start; const w = this.view[1] - this.view[0]; if (a.start < this.view[0] || a.end > this.view[1]) this.setView(a.start - w * 0.2, a.start + w * 0.8); this.draw(); }
    editAnnotationById(id) { const a = this.anns.find((x) => x.id === id); if (a) this.editAnnotation(a); }
    deleteAnnotation(id) { this.anns = this.anns.filter((x) => x.id !== id); if (this.selAnn === id) this.selAnn = null; this.saveAnns(); this.draw(); }
    /** JPEG snapshot of the editor view (for the AI and printed reports), at most `maxW` px wide. */
    snapshot(q = 0.85, maxW = 0) {
      if (!maxW || this.cv.width <= maxW) return this.cv.toDataURL("image/jpeg", q);
      const c = document.createElement("canvas"); c.width = maxW; c.height = Math.round(this.cv.height * maxW / this.cv.width);
      const g = c.getContext("2d"); g.imageSmoothingQuality = "high"; g.drawImage(this.cv, 0, 0, c.width, c.height);
      return c.toDataURL("image/jpeg", q);
    }
  }
  return { SoundEditor, kinds, kindLabel, KIND_COLORS };
})();
