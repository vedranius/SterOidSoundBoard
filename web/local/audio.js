"use strict";
// Audio I/O of the browser edition: microphone (getUserMedia, no echo
// cancellation / noise suppression / AGC, so the voice is not altered), the
// board running in an AudioWorklet, output device choice (setSinkId).
const SsbAudio = (() => {
  const tr = (hr, en) => (I18N.lang === "en" ? en : hr);
  let ctx = null, node = null, src = null, stream = null, run = null;
  let wasmBytes = null;
  const scope = [];
  let peaks = [0, 0, 0, 0], frames = 0, nodes = {}, recCb = null, recEnd = null;
  let chIn = [], chOut = [], taps = {};
  const known = { in: new Map(), out: new Map() }; // channel counts learnt when a device was opened
  const devMap = { in: new Map(), out: new Map() };

  async function devices() {
    let list = [];
    try { list = await navigator.mediaDevices.enumerateDevices(); } catch (_) {}
    const name = (d, i, kind) => d.label || `${kind === "in" ? tr("Ulaz", "Input") : tr("Izlaz", "Output")} ${i + 1}`;
    const pick = (kind, k) => {
      const ds = list.filter((d) => d.kind === k), m = devMap[kind];
      m.clear();
      const names = ds.map((d, i) => { let n = name(d, i, kind); while (m.has(n)) n += " ·"; m.set(n, d.deviceId); return n; });
      const def = ds.find((d) => d.deviceId === "default");
      return [names, def ? names[ds.indexOf(def)] : names[0] || null];
    };
    const [inputs, di] = pick("in", "audioinput"), [outputs, dout] = pick("out", "audiooutput");
    return [{ host: "Web Audio", inputs, outputs, default_input: di, default_output: dout || tr("zadano", "default"),
      input_channels: inputs.map((n) => known.in.get(n) || 2), output_channels: outputs.map((n) => known.out.get(n) || 2) }];
  }

  async function wasm() {
    if (!wasmBytes) wasmBytes = await (await fetch("steroid_web.wasm")).arrayBuffer();
    return wasmBytes.slice(0);
  }

  async function start(cfg, board, opts) {
    await stop();
    if (!navigator.mediaDevices || !window.AudioWorkletNode) throw new Error(tr("Ovaj preglednik ne podržava Web Audio (AudioWorklet). Koristite noviji Chrome, Edge ili Firefox.", "This browser does not support Web Audio (AudioWorklet). Use a recent Chrome, Edge or Firefox."));
    if (!devMap.in.size && !devMap.out.size) await devices();
    const hint = cfg.buffer ? Math.max(cfg.buffer / 48000, 0.001) : "interactive";
    ctx = new AudioContext({ latencyHint: hint });
    try {
      const outId = cfg.output && devMap.out.get(cfg.output);
      if (outId && outId !== "default" && ctx.setSinkId) await ctx.setSinkId(outId).catch(() => {});
      await ctx.audioWorklet.addModule("engine.js");
      await ctx.audioWorklet.addModule("local/worklet.js");
      // microphone first: the worklet needs its channel count
      let inName = null, inCh = 0;
      if (!cfg.no_input) {
        const id = cfg.input && devMap.in.get(cfg.input);
        try {
          stream = await navigator.mediaDevices.getUserMedia({ audio: { deviceId: id ? { exact: id } : undefined, echoCancellation: false, noiseSuppression: false, autoGainControl: false, channelCount: { ideal: cfg.all_channels ? 32 : 2 } } });
        } catch (e) {
          throw new Error(e.name === "NotAllowedError" ? tr("Pristup mikrofonu nije dopušten — dopustite ga u pregledniku (ikona lokota u adresnoj traci).", "Microphone access was denied — allow it in the browser (lock icon in the address bar).")
            : e.name === "NotFoundError" ? tr("Mikrofon nije pronađen.", "No microphone found.") : `${e.name}: ${e.message}`);
        }
        const tk = stream.getAudioTracks()[0], st = tk.getSettings();
        inName = tk.label || cfg.input || null; inCh = Math.min(st.channelCount || 1, 32);
        if (cfg.input) known.in.set(cfg.input, inCh);
      }
      const outCh = cfg.all_channels ? Math.min(Math.max(ctx.destination.maxChannelCount || 2, 1), 32) : 2;
      if (cfg.all_channels) { ctx.destination.channelCount = outCh; ctx.destination.channelCountMode = "explicit"; ctx.destination.channelInterpretation = "discrete"; }
      known.out.set(cfg.output || "", ctx.destination.maxChannelCount || 2);
      if (cfg.output) known.out.set(cfg.output, ctx.destination.maxChannelCount || 2);
      node = new AudioWorkletNode(ctx, "ssb-board", { numberOfInputs: 1, numberOfOutputs: 1, outputChannelCount: [outCh], channelCount: Math.max(inCh, 1), channelCountMode: "explicit",
        channelInterpretation: "discrete", processorOptions: { inCh: Math.max(inCh, 1), outCh, routing: cfg.routing || {} } });
      const ready = new Promise((ok, fail) => {
        node.port.onmessage = (e) => {
          const m = e.data;
          if (m.t === "ready") ok();
          else if (m.t === "peaks") {
            for (let i = 0; i < 4; i++) peaks[i] = Math.max(peaks[i], m.pk[i]);
            frames = m.frames;
            m.chIn.forEach((v, i) => (chIn[i] = Math.max(chIn[i] || 0, v)));
            m.chOut.forEach((v, i) => (chOut[i] = Math.max(chOut[i] || 0, v)));
            for (const [id, d] of Object.entries(m.taps || {})) (taps[id] ||= []).push(d);
          }
          else if (m.t === "scope") { scope.push(m.data); if (scope.length > 64) scope.shift(); }
          else if (m.t === "nodes") nodes = m.nodes;
          else if (m.t === "rec") recCb && recCb(m.data);
          else if (m.t === "rec_end") { recEnd && recEnd(); recEnd = null; }
          else if (m.t === "error") { console.warn("worklet", m); if (m.what === "init") fail(new Error(m.error)); }
        };
      });
      node.port.postMessage({ t: "init", module: await wasm(), board }, []);
      await ready;
      node.port.postMessage({ t: "mute", on: !!opts.mute });
      node.port.postMessage({ t: "tap", src: opts.tap || 0 });
      node.port.postMessage({ t: "bypass_all", on: !!opts.bypass });
      if (stream) {
        src = ctx.createMediaStreamSource(stream);
        src.connect(node);
        await devices(); // labels are known once the microphone is allowed
      }
      node.connect(ctx.destination);
      await ctx.resume();
      run = { host: "Web Audio", input: inName, output: cfg.output || tr("zadano", "default"), sample_rate: ctx.sampleRate, buffer: cfg.buffer || null, out_channels: outCh, in_channels: inCh };
      return run;
    } catch (e) {
      await stop();
      throw e;
    }
  }

  async function stop() {
    run = null;
    if (stream) stream.getTracks().forEach((t) => t.stop());
    stream = null;
    try { src && src.disconnect(); node && node.disconnect(); } catch (_) {}
    src = node = null;
    if (ctx) { const c = ctx; ctx = null; await c.close().catch(() => {}); }
    scope.length = 0;
    chIn = []; chOut = []; taps = {};
  }

  const send = (m) => node && node.port.postMessage(m);
  return {
    devices, start, stop, send,
    info: () => run,
    board: (b) => send({ t: "board", board: b }),
    recStart(srcSel, cb) { recCb = cb; send({ t: "rec", on: true, src: srcSel }); },
    recStop() { return new Promise((ok) => { if (!node) { recCb = null; return ok(); } recEnd = () => { recCb = null; ok(); }; send({ t: "rec", on: false }); setTimeout(() => { if (recEnd) { recEnd(); recEnd = null; } }, 1000); }); },
    state() {
      return {
        running: !!run, sr: ctx ? ctx.sampleRate : 48000,
        scope, frames,
        takePeaks() { const p = peaks; peaks = [0, 0, 0, 0]; return p; },
        takeNodes() { const n = nodes; nodes = {}; send({ t: "meters" }); return n; },
        takeChannels() { const r = [chIn, chOut]; chIn = new Array(run ? run.in_channels : 0).fill(0); chOut = new Array(run ? run.out_channels : 0).fill(0); return r; },
        takeTaps() { const t = taps; taps = {}; return t; },
      };
    },
  };
})();
