"use strict";
// Thin wrapper around the WebAssembly engine (crates/web): JSON calls, WAV
// loading, live analyzer and DSP buffers. Works in the page, in a Worker and in
// the AudioWorklet (which has no TextEncoder/TextDecoder, hence the fallbacks).
(function (root) {
  const enc = typeof TextEncoder !== "undefined" ? new TextEncoder() : null;
  const dec = typeof TextDecoder !== "undefined" ? new TextDecoder() : null;
  function utf8(s) {
    if (enc) return enc.encode(s);
    const out = [];
    for (const ch of s) {
      let c = ch.codePointAt(0);
      if (c < 0x80) out.push(c);
      else if (c < 0x800) out.push(0xc0 | (c >> 6), 0x80 | (c & 63));
      else if (c < 0x10000) out.push(0xe0 | (c >> 12), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
      else out.push(0xf0 | (c >> 18), 0x80 | ((c >> 12) & 63), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
    }
    return new Uint8Array(out);
  }
  function text(b) {
    if (dec) return dec.decode(b);
    let s = "", i = 0;
    while (i < b.length) {
      const c = b[i++];
      let cp = c;
      if (c >= 0xf0) cp = ((c & 7) << 18) | ((b[i++] & 63) << 12) | ((b[i++] & 63) << 6) | (b[i++] & 63);
      else if (c >= 0xe0) cp = ((c & 15) << 12) | ((b[i++] & 63) << 6) | (b[i++] & 63);
      else if (c >= 0xc0) cp = ((c & 31) << 6) | (b[i++] & 63);
      s += String.fromCodePoint(cp);
    }
    return s;
  }

  class SsbEngine {
    constructor(instance) {
      this.x = instance.exports;
    }
    get mem() { return this.x.memory.buffer; }
    put(bytes) {
      const p = this.x.ssb_alloc(bytes.length);
      new Uint8Array(this.mem, p, bytes.length).set(bytes);
      return p;
    }
    out() {
      return text(new Uint8Array(this.mem, this.x.ssb_out_ptr(), this.x.ssb_out_len()).slice());
    }
    /** Call a JSON method; throws Error(message) when the engine reports an error. */
    call(method, arg) {
      const m = utf8(method), a = utf8(arg === undefined ? "" : JSON.stringify(arg));
      const mp = this.put(m), ap = this.put(a);
      const rc = this.x.ssb_call(mp, m.length, ap, a.length);
      this.x.ssb_free(mp, m.length); this.x.ssb_free(ap, a.length);
      const s = this.out();
      if (rc !== 0) throw new Error(s);
      return s ? JSON.parse(s) : null;
    }
    /** Decode WAV bytes (as the native app does) for analyze/tracks; returns the sample rate. */
    loadWav(bytes) {
      const p = this.put(bytes);
      const sr = this.x.ssb_load_wav(p, bytes.length);
      this.x.ssb_free(p, bytes.length);
      if (!sr) throw new Error(this.out());
      return sr;
    }
    liveReset(sr) { this.x.ssb_live_reset(sr); }
    livePush(samples) {
      for (let o = 0; o < samples.length; o += 4096) {
        const part = samples.subarray(o, Math.min(samples.length, o + 4096));
        const p = this.x.ssb_scratch(part.length);
        new Float32Array(this.mem, p, part.length).set(part);
        this.x.ssb_live_push(part.length);
      }
    }
    // DSP (AudioWorklet): views are re-created because memory may grow.
    dspInit(sr) { this.x.dsp_init(sr); }
    dspIn(ch, n) { return new Float32Array(this.mem, this.x.dsp_in(ch), n); }
    dspOut(ch, n) { return new Float32Array(this.mem, this.x.dsp_out(ch), n); }
    dspProcess(n) { this.x.dsp_process(n); }
  }

  /** Instantiate from bytes, a Response promise or a compiled WebAssembly.Module. */
  async function load(src) {
    let inst;
    if (src instanceof WebAssembly.Module) inst = await WebAssembly.instantiate(src, {});
    else {
      const r = await WebAssembly.instantiate(src instanceof ArrayBuffer || ArrayBuffer.isView(src) ? src : await (await src).arrayBuffer(), {});
      inst = r.instance;
    }
    return new SsbEngine(inst);
  }

  root.SsbEngine = { load, Engine: SsbEngine, utf8, text };
  if (typeof module !== "undefined") module.exports = root.SsbEngine;
})(typeof globalThis !== "undefined" ? globalThis : this);
