"use strict";
// Web Worker for the heavy analysis (whole-recording Praat analysis, editor
// contours): keeps the page responsive. Messages: {id, op, key, wav?, arg}.
// `key` names the recording; its WAV bytes are only sent when the worker
// does not hold that recording yet (it replies {need: "wav"} then).
importScripts("../engine.js");
let eng = null, loaded = null;
const ready = (async () => { eng = await SsbEngine.load(fetch("../steroid_web.wasm")); })();

onmessage = async (e) => {
  const { id, op, key, wav, arg } = e.data;
  try {
    await ready;
    if (key && loaded !== key) {
      if (!wav) return postMessage({ id, need: "wav" });
      eng.loadWav(new Uint8Array(wav));
      loaded = key;
    }
    postMessage({ id, ok: eng.call(op, arg) });
  } catch (err) {
    postMessage({ id, error: String((err && err.message) || err) });
  }
};
