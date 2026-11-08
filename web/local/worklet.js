"use strict";
// AudioWorklet: runs the board (the same Rust DSP graph as the desktop app,
// compiled to WebAssembly) on the browser's audio thread. Does what the native
// output callback does: safety limiter (in the engine), mute, peak meters,
// a mono tap for the live sonagram and a stereo tap for the recorder.
class SsbProcessor extends AudioWorkletProcessor {
  constructor() {
    super();
    this.eng = null;
    this.mute = false;
    this.tapOut = false;
    this.recOn = false;
    this.recOut = false;
    this.pk = [0, 0, 0, 0]; // in L/R, out L/R
    this.blocks = 0;
    this.scope = new Float32Array(2048); this.scopeN = 0;
    this.rec = new Float32Array(8192); this.recN = 0; // interleaved stereo
    this.port.onmessage = (e) => this.msg(e.data);
  }

  async msg(m) {
    try {
      if (m.t === "init") {
        this.eng = await globalThis.SsbEngine.load(m.module); // engine.js was added to the worklet first
        this.eng.dspInit(sampleRate);
        if (m.board) this.eng.call("dsp_board", m.board);
        this.port.postMessage({ t: "ready" });
      } else if (!this.eng) return;
      else if (m.t === "board") this.eng.call("dsp_board", m.board);
      else if (m.t === "param") this.eng.call("dsp_param", m);
      else if (m.t === "bypass") this.eng.call("dsp_bypass", m);
      else if (m.t === "mute") this.mute = !!m.on;
      else if (m.t === "tap") this.tapOut = m.src === 1;
      else if (m.t === "rec") { this.flushRec(); this.recOn = !!m.on; this.recOut = m.src === 1; if (!m.on) this.port.postMessage({ t: "rec_end" }); }
      else if (m.t === "meters") this.port.postMessage({ t: "nodes", nodes: this.eng.call("dsp_meters") });
    } catch (e) {
      this.port.postMessage({ t: "error", error: String(e && e.message || e), what: m.t });
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
    const il = inp[0], ir = inp[1] || inp[0];
    const dl = this.eng.dspIn(0, n), dr = this.eng.dspIn(1, n);
    if (il) { dl.set(il); dr.set(ir); } else { dl.fill(0); dr.fill(0); }
    this.eng.dspProcess(n);
    const ol = this.eng.dspOut(0, n), or = this.eng.dspOut(1, n);
    const pk = this.pk;
    for (let i = 0; i < n; i++) {
      const a = il ? il[i] : 0, b = ir ? ir[i] : 0;
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
    }
    const L = out[0], R = out[1] || out[0];
    if (this.mute) { L.fill(0); R.fill(0); }
    else { L.set(ol); if (out[1]) R.set(or); }
    for (let i = 0; i < n; i++) {
      if (Math.abs(L[i]) > pk[2]) pk[2] = Math.abs(L[i]);
      if (Math.abs(R[i]) > pk[3]) pk[3] = Math.abs(R[i]);
    }
    if (++this.blocks >= 8) { // ≈ 20 ms at 48 kHz
      this.port.postMessage({ t: "peaks", pk: pk.slice(), frames: n });
      pk.fill(0);
      this.blocks = 0;
    }
    return true;
  }
}
registerProcessor("ssb-board", SsbProcessor);
