"use strict";
// AudioWorklet: runs the board (the same Rust DSP graph as the desktop app,
// compiled to WebAssembly) on the browser's audio thread. Does what the native
// audio callbacks do: input matrix (every input channel → board L/R), safety
// limiter (in the engine), board bypass, mute, output matrix (board L/R and the
// dry input → every output channel), peak meters per channel, a mono tap for
// the live sonagram, a stereo tap for the recorder, and the display nodes' taps.
class SsbProcessor extends AudioWorkletProcessor {
  constructor(opt) {
    super();
    this.eng = null;
    this.mute = false;
    this.bypass = false;
    this.tapOut = false;
    this.recOn = false;
    this.recOut = false;
    this.inCh = (opt.processorOptions && opt.processorOptions.inCh) || 2;
    this.outCh = (opt.processorOptions && opt.processorOptions.outCh) || 2;
    this.setRouting((opt.processorOptions && opt.processorOptions.routing) || {});
    this.pk = [0, 0, 0, 0]; // board in L/R, out L/R
    this.chIn = new Float32Array(32); this.chOut = new Float32Array(32);
    this.blocks = 0;
    this.taps = [];
    this.scope = new Float32Array(2048); this.scopeN = 0;
    this.rec = new Float32Array(8192); this.recN = 0; // interleaved stereo
    this.port.onmessage = (e) => this.msg(e.data);
  }

  /** Same defaults as crates/engine/src/routing.rs. */
  setRouting(r) {
    const ins = r.inputs || [], outs = r.outputs || [];
    this.gin = []; this.gout = [];
    for (let c = 0; c < this.inCh; c++) this.gin.push(ins.length ? (ins[c] || [0, 0]) : c === 0 ? (this.inCh === 1 ? [1, 1] : [1, 0]) : c === 1 ? [0, 1] : [0, 0]);
    for (let c = 0; c < this.outCh; c++) this.gout.push(outs.length ? (outs[c] || [0, 0, 0, 0]) : c === 0 ? [1, 0, 0, 0] : c === 1 ? [0, 1, 0, 0] : [0, 0, 0, 0]);
  }

  async msg(m) {
    try {
      if (m.t === "init") {
        this.eng = await globalThis.SsbEngine.load(m.module); // engine.js was added to the worklet first
        this.eng.dspInit(sampleRate);
        if (m.board) { this.eng.call("dsp_board", m.board); this.taps = this.eng.call("dsp_taps"); }
        this.port.postMessage({ t: "ready" });
      } else if (m.t === "routing") this.setRouting(m.routing || {});
      else if (!this.eng) return;
      else if (m.t === "board") { this.eng.call("dsp_board", m.board); this.taps = this.eng.call("dsp_taps"); }
      else if (m.t === "param") this.eng.call("dsp_param", m);
      else if (m.t === "bypass") this.eng.call("dsp_bypass", m);
      else if (m.t === "bypass_all") this.bypass = !!m.on;
      else if (m.t === "mute") this.mute = !!m.on;
      else if (m.t === "tap") this.tapOut = m.src === 1;
      else if (m.t === "rec") { this.flushRec(); this.recOn = !!m.on; this.recOut = m.src === 1; if (!m.on) this.port.postMessage({ t: "rec_end" }); }
      else if (m.t === "meters") this.port.postMessage({ t: "nodes", nodes: this.eng.call("dsp_meters") });
    } catch (e) {
      this.port.postMessage({ t: "error", error: String((e && e.message) || e), what: m.t });
    }
  }

  flushRec() {
    if (this.recN) { this.port.postMessage({ t: "rec", data: this.rec.slice(0, this.recN) }); this.recN = 0; }
  }

  process(inputs, outputs) {
    const out = outputs[0];
    if (!this.eng) { for (const c of out) c.fill(0); return true; }
    const n = out[0].length;
    const inp = inputs[0] || [];
    const dl = this.eng.dspIn(0, n), dr = this.eng.dspIn(1, n);
    // input matrix
    dl.fill(0); dr.fill(0);
    const nin = Math.min(inp.length, this.gin.length);
    for (let c = 0; c < nin; c++) {
      const x = inp[c], g = this.gin[c];
      let p = 0;
      for (let i = 0; i < n; i++) { const v = x[i]; if (Math.abs(v) > p) p = Math.abs(v); dl[i] += v * g[0]; dr[i] += v * g[1]; }
      if (p > this.chIn[c]) this.chIn[c] = p;
    }
    if (nin === 1 && inp.length === 1 && this.inCh > 1) dr.set(dl); // a mono stream on a stereo setting
    // dry copy without allocating on the audio thread
    if (!this.dry || this.dry[0].length < n) this.dry = [new Float32Array(n), new Float32Array(n)];
    const il = this.dry[0].subarray(0, n), ir = this.dry[1].subarray(0, n);
    il.set(dl); ir.set(dr);
    if (this.bypass) { this.eng.dspOut(0, n).set(il); this.eng.dspOut(1, n).set(ir); }
    else this.eng.dspProcess(n);
    const ol = this.eng.dspOut(0, n), or = this.eng.dspOut(1, n);
    const pk = this.pk;
    for (let i = 0; i < n; i++) {
      const a = il[i], b = ir[i];
      if (Math.abs(a) > pk[0]) pk[0] = Math.abs(a);
      if (Math.abs(b) > pk[1]) pk[1] = Math.abs(b);
      const l = ol[i], r = or[i];
      // taps: live sonagram (mono) and recorder (stereo), never blocking
      const sl = this.tapOut ? l : a, sr = this.tapOut ? r : b;
      this.scope[this.scopeN++] = (sl + sr) * 0.5;
      if (this.scopeN === this.scope.length) { this.port.postMessage({ t: "scope", data: this.scope.slice() }); this.scopeN = 0; }
      if (this.recOn) {
        this.rec[this.recN++] = this.recOut ? l : a;
        this.rec[this.recN++] = this.recOut ? r : b;
        if (this.recN === this.rec.length) this.flushRec();
      }
      if (Math.abs(l) > pk[2]) pk[2] = Math.abs(l);
      if (Math.abs(r) > pk[3]) pk[3] = Math.abs(r);
    }
    // output matrix
    for (let c = 0; c < out.length; c++) {
      const y = out[c], g = this.gout[c] || [0, 0, 0, 0];
      let p = 0;
      if (this.mute || !(g[0] || g[1] || g[2] || g[3])) { y.fill(0); continue; }
      for (let i = 0; i < n; i++) {
        let v = ol[i] * g[0] + or[i] * g[1] + Math.max(-1, Math.min(1, il[i])) * g[2] + Math.max(-1, Math.min(1, ir[i])) * g[3];
        v = v > 1 ? 1 : v < -1 ? -1 : v;
        y[i] = v;
        if (Math.abs(v) > p) p = Math.abs(v);
      }
      if (p > this.chOut[c]) this.chOut[c] = p;
    }
    if (this.mute) { pk[2] = 0; pk[3] = 0; }
    if (++this.blocks >= 8) { // ≈ 20 ms at 48 kHz
      const taps = {};
      this.taps.forEach((t, i) => { const d = this.eng.dspTapRead(i); if (d) taps[t.id] = d; });
      this.port.postMessage({ t: "peaks", pk: pk.slice(), frames: n, chIn: Array.from(this.chIn.subarray(0, this.inCh)), chOut: Array.from(this.chOut.subarray(0, out.length)), taps });
      pk.fill(0); this.chIn.fill(0); this.chOut.fill(0);
      this.blocks = 0;
    }
    return true;
  }
}
registerProcessor("ssb-board", SsbProcessor);
