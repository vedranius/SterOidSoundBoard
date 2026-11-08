"use strict";
// The desktop app's HTTP + WebSocket server, re-implemented inside the page
// (port of crates/app/src/server.rs and state.rs). The UI keeps calling
// `/api/...` and `/ws`; boot.js routes those here. The heavy and the
// clinically relevant work runs in the same Rust code as the desktop app,
// compiled to WebAssembly: Praat analysis, contours, live sonagram, DSP graph,
// TextGrid, disfluency summary, AI prompt and every validation rule.
const SsbServer = (() => {
  const VERSION = "__SSB_VERSION__";
  const en = () => I18N.lang === "en";
  const tr = (hr, e) => (en() ? e : hr);
  let E = null; // engine on the page
  let store = null; // SsbStore backend
  const enc = new TextEncoder(), dec = new TextDecoder();

  // ---------------------------------------------------------------- state
  const S = {
    board: null, live: false, current_preset: null, clinic: null, ai: null, audioCfg: {},
    dirty: false, mute: false, tap: 0, liveSubs: 0, liveWin: 0.005,
    reports: new Map(), tracks: [], tasks: [], recording: null, lastError: null,
  };
  let seq = 0;
  const now = () => Date.now();
  const newId = (p) => `${p}${now()}-${seq++ % 1000}`;
  const pretty = (v) => JSON.stringify(v, null, 2);

  // ---------------------------------------------------------------- events (/ws)
  const sockets = new Set();
  function emit(v) {
    const s = JSON.stringify(v);
    sockets.forEach((ws) => ws._deliver(s));
  }

  // ---------------------------------------------------------------- persistence
  let chain = Promise.resolve();
  /** Writes are serialised so clinic.json is never written twice at once. */
  const queue = (fn) => (chain = chain.then(fn, fn));
  async function readJson(path) {
    const b = await store.read(path);
    if (!b) return null;
    try { return JSON.parse(dec.decode(b)); } catch (_) { return null; }
  }
  const writeJson = (path, v) => queue(() => store.write(path, enc.encode(pretty(v))));
  async function saveClinic() {
    await writeJson("clinic.json", S.clinic);
    emit({ t: "clinic" });
  }
  const safeName = (name) => {
    let s = [...String(name).trim()].filter((c) => /[\p{L}\p{N}]/u.test(c) || " -_.".includes(c)).join("");
    s = s.replace(/^\.+|\.+$/g, "").trim();
    return !s || s.length > 80 ? null : s;
  };

  async function load(backend) {
    store = backend;
    S.clinic = E.call("normalize", { type: "clinic", value: await readJson("clinic.json") });
    S.board = E.call("normalize", { type: "board", value: await readJson("boards/default.json") });
    S.audioCfg = (await readJson("audio.json")) || {};
    S.ai = Object.assign(SsbAi.defaults(), (await readJson("ai.json")) || {});
    S.reports.clear(); S.tracks = [];
  }
  /** Board autosave (like the desktop app's 2 s timer). */
  setInterval(() => {
    if (S.dirty && store) { S.dirty = false; writeJson("boards/default.json", S.board).catch((e) => console.warn("board save", e)); }
  }, 2000);
  addEventListener("pagehide", () => { if (S.dirty && store) { S.dirty = false; writeJson("boards/default.json", S.board); } });

  // ---------------------------------------------------------------- analysis worker
  const worker = { w: null, n: 0, pending: new Map() };
  function wk() {
    if (!worker.w) {
      worker.w = new Worker("local/analysis-worker.js");
      worker.w.onmessage = (e) => { const p = worker.pending.get(e.data.id); if (p) { worker.pending.delete(e.data.id); p(e.data); } };
    }
    return worker.w;
  }
  const wavCache = new Map();
  async function wavBytes(id) {
    if (wavCache.has(id)) return wavCache.get(id);
    const b = await store.read(`recordings/${id}.wav`);
    if (!b) throw new Error("recording file missing");
    wavCache.set(id, b);
    while (wavCache.size > 3) wavCache.delete(wavCache.keys().next().value);
    return b;
  }
  async function work(op, id, arg) {
    const send = (wav) => new Promise((ok) => { const n = ++worker.n; worker.pending.set(n, ok); wk().postMessage({ id: n, op, key: id, wav, arg }); });
    let r = await send(null);
    if (r.need === "wav") { const b = await wavBytes(id); r = await send(b.slice().buffer); }
    if (r.error) throw new Error(r.error);
    return r.ok;
  }

  const sanitized = (s) => E.call("settings_sanitized", s == null ? S.clinic.settings.analysis : s);
  const settingsKey = (id, s) => `${id}|${s.pitch_floor}|${s.pitch_ceiling}|${s.max_formant}|${s.n_formants}|${s.cpps}`;
  const recById = (id) => S.clinic.recordings.find((r) => r.id === id);

  async function wholeReport(id, st) {
    const key = settingsKey(id, st);
    if (S.reports.has(key)) return S.reports.get(key);
    const r = await work("analyze", id, { settings: st });
    if (S.reports.size > 500) S.reports.clear();
    S.reports.set(key, r);
    return r;
  }
  async function analyze(id, start, end, settings) {
    const rec = recById(id);
    if (!rec) throw new Error("no such recording");
    const st = sanitized(settings);
    const whole = (start == null || start <= 0.001) && (end == null || end >= rec.duration - 0.001);
    if (whole) return structuredClone(await wholeReport(id, st));
    return work("analyze", id, { start, end, settings: st });
  }
  async function tracks(id, settings) {
    if (!recById(id)) throw new Error("no such recording");
    const st = sanitized(settings), key = settingsKey(id, st);
    const hit = S.tracks.find((x) => x[0] === key);
    if (hit) return hit[1];
    const t = await work("tracks", id, { settings: st });
    S.tracks = S.tracks.filter((x) => x[0] !== key).concat([[key, t]]).slice(-6);
    return t;
  }

  // ---------------------------------------------------------------- board
  function setBoard(b, preset) {
    const nb = E.call("sanitize_board", b);
    S.board = nb;
    SsbAudio.board(nb);
    S.dirty = true;
    emit({ t: "board", board: S.board });
    S.current_preset = preset ?? null;
    emit({ t: "preset", name: S.current_preset });
  }
  function structural() {
    SsbAudio.board(S.board);
    S.dirty = true;
    emit({ t: "board", board: S.board });
  }
  const node = (id) => S.board.nodes.find((n) => n.id === id);
  function setParam(id, param, value) {
    const n = node(id);
    if (!n) throw new Error("no such node");
    const v = E.call("clamp_param", { kind: n.kind, param, value });
    n.params[param] = v;
    SsbAudio.send({ t: "param", node: id, param, value: v });
    S.dirty = true;
    return v;
  }
  function setBypass(id, on) {
    const n = node(id);
    if (!n) throw new Error("no such node");
    n.bypass = on;
    SsbAudio.send({ t: "bypass", node: id, on });
    S.dirty = true;
  }

  // ---------------------------------------------------------------- AI tasks
  const MAX_TASKS = 30;
  function newTask(kind, recordingId, label) {
    const busy = S.tasks.find((t) => t.status === "running" && t.kind === kind);
    if (busy) throw new Error(kind === "pull" ? tr(`već se preuzima model ${busy.label} — pričekajte da završi`, `model ${busy.label} is already downloading — wait until it finishes`)
      : tr("AI već obrađuje drugi zahtjev — pričekajte ga ili ga zaustavite", "the AI is already working on another request — wait for it or stop it"));
    const t = { id: newId("t"), kind, recording_id: recordingId, label, started: now(), status: "running", log: [], text: "", stats: null, report: null, error: null };
    S.tasks.push(t);
    const running = S.tasks.filter((x) => x.status === "running").length;
    while (S.tasks.length > Math.max(MAX_TASKS, running)) { const i = S.tasks.findIndex((x) => x.status !== "running"); if (i < 0) break; S.tasks.splice(i, 1); }
    const ac = new AbortController(), t0 = performance.now();
    let buf = "", sent = 0, lastFlush = performance.now();
    const flush = () => { if (buf) { emit({ t: "ai_task", id: t.id, ev: "delta", text: buf, at: sent }); sent += [...buf].length; buf = ""; } lastFlush = performance.now(); };
    const p = {
      signal: ac.signal,
      cancelled: () => ac.signal.aborted,
      log(msg) { const ms = Math.round(performance.now() - t0); flush(); if (t.log.length < 1000) { const n = t.log.length; t.log.push([ms, msg]); emit({ t: "ai_task", id: t.id, ev: "log", n, ms, msg }); } },
      delta(text) { if (t.text.length < 400000) t.text += text; buf += text; if (performance.now() - lastFlush > 150) flush(); },
      stats(v) { flush(); t.stats = v; emit({ t: "ai_task", id: t.id, ev: "stats", stats: v }); },
      flush,
    };
    t._ac = ac;
    emit({ t: "ai_task", id: t.id, ev: "start", kind });
    return [t, p];
  }
  function finishTask(t, status, report, error) {
    if (t.status !== "running") return;
    t.status = status; t.report = report || null; t.error = error || null;
    emit({ t: "ai_task", id: t.id, ev: "end", status, report: t.report, error: t.error });
  }
  const taskJson = (t) => { const { _ac, ...v } = t; return { ...v, age_ms: now() - t.started }; };

  async function aiJob(id, b) {
    const cfg = S.ai;
    const rec = recById(id);
    if (!rec) throw new Error("no such recording");
    const pat = rec.patient_id && S.clinic.patients.find((x) => x.id === rec.patient_id);
    if (pat && !pat.ai_consent) throw new Error(tr("pacijent nema zabilježenu suglasnost za AI analizu (Uredi pacijenta)", "the patient has no recorded consent for AI analysis (Edit patient)"));
    const earlier = S.clinic.recordings.filter((r) => r.patient_id && r.patient_id === rec.patient_id && r.created < rec.created).sort((a, c) => a.created - c.created).slice(-6);
    const report = await analyze(id, b.start, b.end, b.settings);
    const st = sanitized(b.settings);
    const a = Math.max(b.start ?? 0, 0), z = Math.min(b.end ?? rec.duration, rec.duration);
    const history = [];
    for (const r of earlier) { try { history.push([E.call("date_str", r.created), r.task, await wholeReport(r.id, st)]); } catch (_) {} }
    let whole = null; try { whole = await wholeReport(id, st); } catch (_) {}
    const summary = rec.annotations.length ? E.call("summary", { rec, nuclei: whole ? whole.syllable_nuclei : null }) : null;
    const image = b.image && cfg.send_image ? SsbAi.parseImage(b.image) : null;
    const patient = pat ? (({ id: _i, created: _c, updated: _u, ...d }) => d)(pat) : null; // PatientIn: never sent anywhere as is
    const age = patient ? E.call("age_at", { birth: patient.birth, at: rec.created }) : null;
    const q = String(b.question || "").slice(0, 4000);
    const { system, user } = E.call("prompt", { patient, age, rec, start: Math.min(a, z), end: Math.max(a, z), report, history, question: q, has_image: !!image,
      annotations: summary, calibration_db: S.clinic.settings.calibration_db, en: en() });
    return { cfg, rec, start: Math.min(a, z), end: Math.max(a, z), system, user, image, question: q };
  }
  function aiStart(job) {
    const [t, p] = newTask("report", job.rec.id, job.cfg.model.trim());
    (async () => {
      try {
        const ans = await SsbAi.ask(job.cfg, job.system, job.user, job.image, SsbAi.MAX_TOKENS, p);
        p.flush();
        if (p.cancelled()) return finishTask(t, "cancelled");
        const rep = { id: newId("a"), recording_id: job.rec.id, patient_id: job.rec.patient_id, created: now(), provider: job.cfg.provider,
          model: ans.model || job.cfg.model, start: job.start, end: job.end, question: job.question, text: ans.text, truncated: ans.truncated };
        if (!recById(rep.recording_id)) return finishTask(t, "error", null, tr("snimka je u međuvremenu obrisana", "the recording has been deleted in the meantime"));
        S.clinic.ai_reports.push(rep);
        await saveClinic();
        p.log(tr("Mišljenje je spremljeno uz snimku.", "The opinion was saved with the recording."));
        finishTask(t, "done", rep);
      } catch (e) {
        p.flush();
        if (p.cancelled()) return finishTask(t, "cancelled");
        p.log(tr("Greška: ", "Error: ") + e.message);
        finishTask(t, "error", null, e.message);
      }
    })();
    return t.id;
  }

  // ---------------------------------------------------------------- recording
  function wav24(data, sr) {
    const frames = data.length / 2, bytes = 44 + frames * 6, b = new Uint8Array(bytes), v = new DataView(b.buffer);
    const s = (o, t) => [...t].forEach((c, i) => v.setUint8(o + i, c.charCodeAt(0)));
    s(0, "RIFF"); v.setUint32(4, bytes - 8, true); s(8, "WAVE"); s(12, "fmt "); v.setUint32(16, 16, true); v.setUint16(20, 1, true); v.setUint16(22, 2, true);
    v.setUint32(24, sr, true); v.setUint32(28, sr * 6, true); v.setUint16(32, 6, true); v.setUint16(34, 24, true); s(36, "data"); v.setUint32(40, frames * 6, true);
    let o = 44;
    for (let i = 0; i < data.length; i++) {
      const x = Math.trunc(Math.max(-1, Math.min(1, data[i])) * 8388607); // as the native recorder (hound, 24-bit)
      b[o++] = x & 255; b[o++] = (x >> 8) & 255; b[o++] = (x >> 16) & 255;
    }
    return b;
  }
  function activeSession() { return S.clinic.sessions.find((s) => s.end == null); }
  function sessionFor(pid) { const a = activeSession(); return a && pid && a.patient_id === pid ? a.id : null; }

  // ---------------------------------------------------------------- meters (20 Hz)
  setInterval(() => {
    if (!sockets.size) return;
    const a = SsbAudio.state();
    let live = null;
    if (a.running) {
      if (Math.abs((S.liveSr || 0) - a.sr) > 0.5) { E.liveReset(a.sr); S.liveSr = a.sr; }
      for (const c of a.scope.splice(0)) E.livePush(c);
      if (S.liveSubs > 0) live = E.call("live_frames", { win: S.liveWin });
    }
    const pk = a.takePeaks();
    emit({ live, rec: S.recording ? S.recording.frames / a.sr : null, rec_dropped: 0, mute: S.mute, t: "meters",
      in: [pk[0], pk[1]], out: [pk[2], pk[3]], load: 0, xruns: 0, frames: a.frames, nodes: a.takeNodes() });
  }, 50);

  // ---------------------------------------------------------------- routes
  class HttpError extends Error { constructor(m, status = 400) { super(m); this.status = status; } }
  const routes = [];
  const R = (method, pattern, fn) => routes.push([method, new RegExp("^" + pattern.replace(/\{(\w+)\}/g, "(?<$1>[^/]+)") + "$"), fn]);

  R("GET", "/api/status", () => ({ version: VERSION, running: SsbAudio.info(), config: S.audioCfg, error: S.lastError, mute: S.mute, preset: S.current_preset, recording: !!S.recording, edition: "web" }));
  R("GET", "/api/devices", () => SsbAudio.devices());
  R("GET", "/api/node-types", () => E.call("node_types"));
  R("POST", "/api/audio/start", async (q) => {
    if (S.recording) await stopRecording().catch(() => {});
    S.audioCfg = q.body || {};
    try {
      const info = await SsbAudio.start(S.audioCfg, S.board, { mute: S.mute, tap: S.tap });
      S.lastError = null;
      queue(() => store.write("audio.json", enc.encode(pretty(S.audioCfg))));
      emit({ t: "audio", running: info });
      return info;
    } catch (e) {
      S.lastError = e.message;
      emit({ t: "audio", running: null, error: e.message });
      throw e;
    }
  });
  R("POST", "/api/audio/stop", async () => { if (S.recording) await stopRecording().catch(() => {}); await SsbAudio.stop(); emit({ t: "audio", running: null }); return null; });
  R("GET", "/api/board", () => S.board);
  R("POST", "/api/nodes", (q) => {
    const id = `n${Math.max(S.board.next_id || 1, 1)}`;
    S.board.next_id = Math.max(S.board.next_id || 1, 1) + 1;
    const d = E.call("new_node", { id, kind: q.body.kind, x: q.body.x || 0, y: q.body.y || 0, role: q.body.role || null });
    S.board.nodes.push(d);
    structural();
    return d;
  });
  R("DELETE", "/api/nodes/{id}", (q) => {
    if (!node(q.id)) throw new HttpError("no such node");
    S.board.nodes = S.board.nodes.filter((n) => n.id !== q.id);
    S.board.connections = S.board.connections.filter((c) => c.from !== q.id && c.to !== q.id);
    structural();
    return null;
  });
  R("POST", "/api/connections", (q) => { E.call("can_connect", { board: S.board, connection: q.body }); S.board.connections.push({ from: q.body.from, to: q.body.to }); structural(); return null; });
  R("DELETE", "/api/connections", (q) => { S.board.connections = S.board.connections.filter((c) => !(c.from === q.body.from && c.to === q.body.to)); structural(); return null; });
  R("POST", "/api/board/replace", (q) => { setBoard(q.body, null); return null; });
  R("GET", "/api/templates", () => E.call("templates"));
  R("POST", "/api/templates/{id}", (q) => { setBoard(E.call("template", q.id), null); return null; });
  R("GET", "/api/presets", async () => (await store.list("presets")).filter((f) => f.endsWith(".json")).map((f) => f.slice(0, -5)).sort((a, b) => a.toLowerCase().localeCompare(b.toLowerCase())));
  R("POST", "/api/presets", async (q) => {
    const n = safeName(q.body.name || "");
    if (!n) throw new HttpError("invalid preset name");
    const b = structuredClone(S.board); b.name = String(q.body.name).trim();
    await writeJson(`presets/${n}.json`, b);
    emit({ t: "presets" });
    return { name: b.name };
  });
  R("POST", "/api/presets/{name}/load", async (q) => {
    const n = safeName(q.name); const b = n && (await readJson(`presets/${n}.json`));
    if (!b) throw new HttpError("no such preset");
    setBoard(b, null); return null;
  });
  R("DELETE", "/api/presets/{name}", async (q) => {
    const n = safeName(q.name);
    if (!n || !(await store.read(`presets/${n}.json`))) throw new HttpError("no such preset");
    await queue(() => store.remove(`presets/${n}.json`)); emit({ t: "presets" }); return null;
  });

  // ---- patients, sessions
  R("GET", "/api/patients", () => [...S.clinic.patients].sort((a, b) => a.name.toLowerCase().localeCompare(b.name.toLowerCase())));
  R("POST", "/api/patients", async (q) => {
    const d = E.call("patient_check", { patient: q.body, en: en() });
    const t = now(), p = { id: newId("p"), ...d, created: t, updated: t };
    S.clinic.patients.push(p); await saveClinic(); return p;
  });
  R("PUT", "/api/patients/{id}", async (q) => {
    const d = E.call("patient_check", { patient: q.body, en: en() });
    const i = S.clinic.patients.findIndex((x) => x.id === q.id);
    if (i < 0) throw new HttpError("no such patient");
    S.clinic.patients[i] = { id: q.id, ...d, created: S.clinic.patients[i].created, updated: now() };
    await saveClinic(); return S.clinic.patients[i];
  });
  R("DELETE", "/api/patients/{id}", async (q) => {
    const c = S.clinic;
    if (!c.patients.some((p) => p.id === q.id)) throw new HttpError("no such patient");
    const gone = c.recordings.filter((r) => r.patient_id === q.id).map((r) => r.id);
    c.patients = c.patients.filter((p) => p.id !== q.id);
    c.sessions = c.sessions.filter((s) => s.patient_id !== q.id);
    c.recordings = c.recordings.filter((r) => r.patient_id !== q.id);
    c.ai_reports = c.ai_reports.filter((a) => a.patient_id !== q.id);
    for (const r of gone) await queue(() => store.remove(`recordings/${r}.wav`));
    await saveClinic(); return null;
  });
  R("GET", "/api/sessions", (q) => S.clinic.sessions.filter((s) => !q.query.patient || s.patient_id === q.query.patient)
    .map((s) => ({ id: s.id, patient_id: s.patient_id, start: s.start, end: s.end ?? null, notes: s.notes, preset: s.preset ?? null, recordings: S.clinic.recordings.filter((r) => r.session_id === s.id).length })).reverse());
  R("POST", "/api/sessions", async (q) => {
    const c = S.clinic;
    if (!c.patients.some((p) => p.id === q.body.patient_id)) throw new HttpError("no such patient");
    const t = now();
    c.sessions.filter((s) => s.end == null).forEach((s) => (s.end = t));
    const s = { id: newId("s"), patient_id: q.body.patient_id, start: t, end: null, notes: "", board: structuredClone(S.board), preset: S.current_preset };
    c.sessions.push(s); await saveClinic();
    return { id: s.id, patient_id: s.patient_id, start: s.start, end: s.end, notes: s.notes };
  });
  const sessionEdit = async (id, notes, stop) => {
    const s = S.clinic.sessions.find((x) => x.id === id);
    if (!s) throw new HttpError("no such session");
    if (notes != null) { if (notes.length > 100000) throw new HttpError("notes too long"); s.notes = notes; }
    if (stop && s.end == null) s.end = now();
    await saveClinic(); return null;
  };
  R("PUT", "/api/sessions/{id}", (q) => sessionEdit(q.id, q.body.notes, false));
  R("POST", "/api/sessions/{id}/stop", (q) => sessionEdit(q.id, (q.body || {}).notes, true));
  R("GET", "/api/sessions/{id}/detail", (q) => {
    const s = S.clinic.sessions.find((x) => x.id === q.id);
    if (!s) throw new HttpError("no such session");
    const nodes = s.board ? s.board.nodes.filter((n) => n.role).map((n) => ({ role: n.role, kind: n.kind, bypass: n.bypass, params: n.params })) : [];
    return { id: s.id, patient_id: s.patient_id, start: s.start, end: s.end ?? null, notes: s.notes, preset: s.preset ?? null, has_board: !!s.board, nodes, recordings: S.clinic.recordings.filter((r) => r.session_id === s.id) };
  });
  R("POST", "/api/sessions/{id}/apply", (q) => {
    const s = S.clinic.sessions.find((x) => x.id === q.id);
    if (!s) throw new HttpError("no such session");
    if (!s.board) throw new HttpError("sesija nema spremljene postavke");
    setBoard(structuredClone(s.board), s.preset ?? null); return null;
  });

  // ---- recording
  async function stopRecording() {
    const r = S.recording;
    if (!r) throw new HttpError("not recording");
    await SsbAudio.recStop();
    S.recording = null;
    emit({ t: "rec", on: false });
    const total = r.chunks.reduce((n, c) => n + c.length, 0), data = new Float32Array(total);
    let o = 0; for (const c of r.chunks) { data.set(c, o); o += c.length; }
    const bytes = wav24(data, r.meta.sample_rate);
    await queue(() => store.write(`recordings/${r.meta.id}.wav`, bytes));
    r.meta.duration = total / 2 / r.meta.sample_rate;
    S.clinic.recordings.push(r.meta); await saveClinic();
    return r.meta;
  }
  R("POST", "/api/record/start", async (q) => {
    if (S.recording) throw new HttpError("already recording");
    const b = q.body || {}, pid = b.patient_id || null;
    if (pid && !S.clinic.patients.some((x) => x.id === pid)) throw new HttpError("no such patient");
    const info = SsbAudio.info();
    if (!info) throw new HttpError(tr("zvuk nije pokrenut", "audio is not running"));
    const src = b.source === "out" ? 1 : 0;
    const meta = { id: newId("r"), patient_id: pid, session_id: sessionFor(pid), label: String(b.label || "").slice(0, 200), created: now(), duration: 0,
      sample_rate: info.sample_rate, source: src ? "out" : "in", task: String(b.task || "").slice(0, 200), notes: "", annotations: [], syllables: null, analyses: [] };
    S.recording = { meta, chunks: [], frames: 0 };
    SsbAudio.recStart(src, (chunk) => { if (S.recording) { S.recording.chunks.push(chunk); S.recording.frames += chunk.length / 2; } });
    emit({ t: "rec", on: true });
    return null;
  });
  R("POST", "/api/record/stop", () => stopRecording());
  R("GET", "/api/recordings", (q) => S.clinic.recordings.filter((r) => !q.query.patient || r.patient_id === q.query.patient).sort((a, b) => b.created - a.created));
  R("PUT", "/api/recordings/{id}", async (q) => {
    const r = recById(q.id);
    if (!r) throw new HttpError("no such recording");
    const e = q.body || {};
    if (e.label != null) r.label = [...String(e.label)].slice(0, 200).join("");
    if (e.task != null) r.task = [...String(e.task)].slice(0, 200).join("");
    if (e.notes != null) r.notes = [...String(e.notes)].slice(0, 20000).join("");
    if (e.annotations != null) r.annotations = E.call("annotations_check", { annotations: e.annotations, duration: r.duration, en: en() });
    if (e.syllables != null) r.syllables = e.syllables > 0 ? Math.min(e.syllables, 1000000) : null;
    await saveClinic(); return null;
  });
  R("DELETE", "/api/recordings/{id}", async (q) => {
    const c = S.clinic;
    if (!recById(q.id)) throw new HttpError("no such recording");
    c.recordings = c.recordings.filter((r) => r.id !== q.id);
    c.ai_reports = c.ai_reports.filter((a) => a.recording_id !== q.id);
    wavCache.delete(q.id);
    await queue(() => store.remove(`recordings/${q.id}.wav`));
    await saveClinic(); return null;
  });
  R("GET", "/api/recordings/{id}/wav", async (q) => {
    const r = recById(q.id);
    if (!r) return new Response("not found", { status: 404 });
    const p = r.patient_id && S.clinic.patients.find((x) => x.id === r.patient_id);
    const who = [...(p ? p.name : "snimka")].map((ch) => (/[\p{L}\p{N}]/u.test(ch) ? ch : "_")).join("");
    return new Response(await wavBytes(q.id), { headers: { "content-type": "audio/wav", "content-disposition": `inline; filename="${who}_${r.id}.wav"` } });
  });
  R("POST", "/api/recordings/upload", async (q) => {
    const pid = q.query.patient || null;
    if (pid && !S.clinic.patients.some((x) => x.id === pid)) throw new HttpError("no such patient");
    const bytes = new Uint8Array(await q.raw.arrayBuffer());
    let sr;
    try { E.loadWav(bytes); sr = E.call("wav_info"); } catch (e) { throw new HttpError(tr(`datoteka nije ispravan WAV: ${e.message}`, `the file is not a valid WAV: ${e.message}`)); }
    if (!sr.frames || sr.sample_rate < 8000) throw new HttpError(tr(`snimka je prazna ili ima prenisku frekvenciju uzorkovanja (${sr.sample_rate} Hz)`, `the recording is empty or its sample rate is too low (${sr.sample_rate} Hz)`));
    const meta = { id: newId("r"), patient_id: pid, session_id: sessionFor(pid), label: String(q.query.label || "").slice(0, 200), created: now(), duration: sr.frames / sr.sample_rate,
      sample_rate: sr.sample_rate, source: "upload", task: String(q.query.task || "").slice(0, 200), notes: "", annotations: [], syllables: null, analyses: [] };
    await queue(() => store.write(`recordings/${meta.id}.wav`, bytes));
    S.clinic.recordings.push(meta); await saveClinic();
    return meta;
  });
  R("POST", "/api/recordings/{id}/analyze", async (q) => {
    const b = q.body || {};
    let rep = await analyze(q.id, b.start, b.end, b.settings);
    if (en()) rep = E.call("localize", { report: rep, en: true });
    return rep;
  });
  R("POST", "/api/recordings/{id}/tracks", (q) => tracks(q.id, (q.body || {}).settings));
  R("GET", "/api/recordings/{id}/textgrid", (q) => {
    const r = recById(q.id);
    if (!r) return new Response("not found", { status: 404 });
    return new Response(E.call("textgrid", { rec: r, en: q.query.lang === "en" }), { headers: { "content-type": "text/plain; charset=utf-8", "content-disposition": `attachment; filename="${r.id}.TextGrid"` } });
  });
  R("POST", "/api/recordings/{id}/analyses", async (q) => {
    const b = q.body || {};
    let rep = await analyze(q.id, b.start, b.end, b.settings);
    if (en()) rep = E.call("localize", { report: rep, en: true });
    delete rep.pitch;
    const a = { id: newId("v"), created: now(), start: b.start, end: b.end, label: String(b.label || "").slice(0, 200), report: rep };
    const r = recById(q.id);
    if (!r) throw new HttpError("no such recording");
    if (r.analyses.length >= 200) throw new HttpError(tr("previše spremljenih analiza za ovu snimku", "too many saved analyses for this recording"));
    r.analyses.push(a); await saveClinic(); return a;
  });
  R("DELETE", "/api/recordings/{id}/analyses/{aid}", async (q) => {
    const r = recById(q.id);
    if (!r) throw new HttpError("no such recording");
    const n = r.analyses.length; r.analyses = r.analyses.filter((a) => a.id !== q.aid);
    if (r.analyses.length === n) throw new HttpError(tr("nema te analize", "no such analysis"));
    await saveClinic(); return null;
  });
  R("GET", "/api/recordings/{id}/summary", async (q) => {
    const rec = recById(q.id);
    if (!rec) throw new HttpError("no such recording");
    let nuclei = null;
    try { nuclei = (await wholeReport(q.id, sanitized(null))).syllable_nuclei; } catch (_) {}
    return E.call("summary", { rec, nuclei });
  });
  R("GET", "/api/patients/{id}/progress", async (q) => {
    if (!S.clinic.patients.some((p) => p.id === q.id)) throw new HttpError("no such patient");
    const recs = S.clinic.recordings.filter((r) => r.patient_id === q.id).sort((a, b) => a.created - b.created), st = sanitized(null), rows = [];
    for (const r of recs) {
      let rep = null; try { rep = await wholeReport(r.id, st); } catch (_) {}
      const sum = E.call("summary", { rec: r, nuclei: rep ? rep.syllable_nuclei : null });
      const g = (f) => (rep ? f(rep) ?? null : null);
      rows.push({ id: r.id, created: r.created, task: r.task, label: r.label, session_id: r.session_id, duration: r.duration,
        f0_mean: g((x) => x.f0_mean), f0_sd: g((x) => x.f0_sd), f0_range_st: g((x) => x.f0_range_st), jitter_local: g((x) => x.jitter_local), shimmer_local: g((x) => x.shimmer_local),
        hnr_db: g((x) => x.hnr_db), cpps: g((x) => x.cpps), intensity_mean_db: g((x) => x.intensity_mean_db), max_voiced_s: g((x) => x.max_voiced_s),
        speech_rate: g((x) => x.speech_rate), articulation_rate: g((x) => x.articulation_rate), pauses: g((x) => x.pauses), f1: g((x) => x.formants[0]), f2: g((x) => x.formants[1]),
        disfluencies: sum.total, sld: sum.sld, pct_ss: sum.pct_ss });
    }
    return { settings: st, rows };
  });
  R("GET", "/api/clinic/settings", () => S.clinic.settings);
  R("PUT", "/api/clinic/settings", async (q) => { S.clinic.settings = E.call("normalize", { type: "clinic_settings", value: q.body }); await saveClinic(); return S.clinic.settings; });
  R("GET", "/api/clinic/annotation-kinds", () => E.call("annotation_kinds"));
  const customPresets = async () => {
    const out = [];
    for (const f of await store.list("clinic_presets")) if (f.endsWith(".json")) { const p = await readJson(`clinic_presets/${f}`); if (p && p.id) out.push(p); }
    return out.sort((a, b) => a.name.toLowerCase().localeCompare(b.name.toLowerCase()));
  };
  const presetFile = (id) => { if (!id || !/^[A-Za-z0-9-]+$/.test(id)) throw new HttpError("invalid preset id"); return `clinic_presets/${id}.json`; };
  R("GET", "/api/clinic/presets", async () => [
    ...E.call("clinical_presets").map((p) => ({ id: `f:${p.id}`, name: p.name, description: p.description, category: p.category, factory: true })),
    ...(await customPresets()).map((p) => ({ id: `c:${p.id}`, name: p.name, description: p.description || "", category: p.category || "Vlastiti", factory: false, created: p.created })),
  ]);
  R("POST", "/api/clinic/presets", async (q) => {
    const name = String(q.body.name || "").trim();
    if (!name || name.length > 120) throw new HttpError(tr("naziv preseta je obavezan (do 120 znakova)", "a preset name is required (up to 120 characters)"));
    const p = { id: newId("cp"), name, description: String(q.body.description || "").slice(0, 2000), category: String(q.body.category || "").slice(0, 100), created: now(), board: structuredClone(S.board) };
    await writeJson(presetFile(p.id), p);
    S.current_preset = p.name;
    emit({ t: "clinic_presets" });
    return { id: `c:${p.id}` };
  });
  R("POST", "/api/clinic/presets/{id}/load", async (q) => {
    const id = decodeURIComponent(q.id);
    if (id.startsWith("f:")) {
      const p = E.call("clinical_presets").find((x) => x.id === id.slice(2));
      if (!p) throw new HttpError("no such preset");
      setBoard(p.board, p.name); return null;
    }
    if (!id.startsWith("c:")) throw new HttpError("no such preset");
    const p = await readJson(presetFile(id.slice(2)));
    if (!p) throw new HttpError("no such preset");
    setBoard(p.board, p.name); return null;
  });
  R("DELETE", "/api/clinic/presets/{id}", async (q) => {
    const id = decodeURIComponent(q.id);
    if (!id.startsWith("c:")) throw new HttpError(tr("tvornički preseti se ne mogu brisati", "factory presets cannot be deleted"));
    const f = presetFile(id.slice(2));
    if (!(await store.read(f))) throw new HttpError("no such preset");
    await queue(() => store.remove(f)); emit({ t: "clinic_presets" }); return null;
  });

  // ---- AI
  R("GET", "/api/ai/config", () => SsbAi.pub(S.ai));
  R("PUT", "/api/ai/config", async (q) => {
    const b = q.body || {};
    if (!["anthropic", "openai", "ollama"].includes(b.provider)) throw new HttpError("nepoznat AI servis");
    const base = String(b.base_url || "").trim();
    if (base && !/^https?:\/\//.test(base)) throw new HttpError("adresa mora počinjati s http:// ili https://");
    const c = S.ai, changed = c.provider !== b.provider;
    c.provider = b.provider; c.base_url = base.slice(0, 500); c.model = String(b.model || "").trim().slice(0, 200);
    c.send_image = b.send_image !== false; c.ollama_ctx = !b.ollama_ctx ? 0 : Math.min(Math.max(b.ollama_ctx, 2048), 131072);
    if (b.clear_key || changed) c.api_key = ""; // a key never silently follows a switch to another service
    const k = String(b.api_key || "").trim();
    if (k) c.api_key = k.slice(0, 1000);
    await writeJson("ai.json", c);
    return SsbAi.pub(c);
  });
  R("POST", "/api/ai/test", async () => { const a = await SsbAi.test(S.ai); return { ok: true, model: a.model, text: [...a.text].slice(0, 200).join("") }; });
  R("POST", "/api/recordings/{id}/ai/preview", async (q) => {
    const j = await aiJob(q.id, q.body || {});
    return { provider: SsbAi.providerName(j.cfg), url: SsbAi.base(j.cfg), model: j.cfg.model, image: !!j.image, system: j.system, user: j.user };
  });
  R("POST", "/api/recordings/{id}/ai", async (q) => ({ task: aiStart(await aiJob(q.id, q.body || {})) }));
  R("GET", "/api/recordings/{id}/ai", (q) => S.clinic.ai_reports.filter((a) => a.recording_id === q.id).sort((a, b) => b.created - a.created));
  R("GET", "/api/ai/tasks", (q) => S.tasks.filter((t) => !q.query.recording || t.recording_id === q.query.recording).map(taskJson));
  R("GET", "/api/ai/tasks/{id}", (q) => { const t = S.tasks.find((x) => x.id === q.id); if (!t) throw new HttpError("nema tog zadatka"); return taskJson(t); });
  R("POST", "/api/ai/tasks/{id}/cancel", (q) => {
    const t = S.tasks.find((x) => x.id === q.id);
    if (!t) throw new HttpError(tr("nema tog zadatka", "no such task"));
    const msg = en() ? "Stopped at the user's request." : "Zaustavljeno na zahtjev korisnika.", ms = now() - t.started, n = t.log.length;
    if (t.status === "running") t.log.push([ms, msg]);
    t._ac && t._ac.abort();
    emit({ t: "ai_task", id: t.id, ev: "log", n, ms, msg });
    finishTask(t, "cancelled");
    return null;
  });
  const ollamaRootFor = (url) => SsbAi.ollamaRoot(url != null ? url : S.ai.provider === "ollama" ? S.ai.base_url : "");
  R("GET", "/api/ai/ollama", (q) => {
    const u = q.query.url && q.query.url.trim();
    if (u && !/^https?:\/\//.test(u)) throw new HttpError("adresa mora počinjati s http:// ili https://");
    return SsbAi.ollamaStatus(ollamaRootFor(u || null));
  });
  const checkModel = (m) => { m = String(m || "").trim(); if (!m || m.length > 200 || !/^[A-Za-z0-9._:/-]+$/.test(m)) throw new HttpError("neispravan naziv modela"); return m; };
  R("POST", "/api/ai/ollama/pull", (q) => {
    const model = checkModel(q.body.model), root = ollamaRootFor(null);
    const [t, p] = newTask("pull", null, model);
    SsbAi.ollamaPull(root, model, p).then(() => finishTask(t, "done"), (e) => { if (p.cancelled()) return finishTask(t, "cancelled"); p.log(tr("Greška: ", "Error: ") + e.message); finishTask(t, "error", null, e.message); });
    return { task: t.id };
  });
  R("POST", "/api/ai/ollama/delete", async (q) => { await SsbAi.ollamaDelete(ollamaRootFor(null), checkModel(q.body.model)); return null; });
  R("DELETE", "/api/ai-reports/{id}", async (q) => {
    const n = S.clinic.ai_reports.length; S.clinic.ai_reports = S.clinic.ai_reports.filter((a) => a.id !== q.id);
    if (S.clinic.ai_reports.length === n) throw new HttpError("no such AI report");
    await saveClinic(); return null;
  });

  /** Handle one `/api/...` request; returns a Response like the real server. */
  async function handle(method, url, body, raw) {
    const u = new URL(url, location.href);
    for (const [m, re, fn] of routes) {
      if (m !== method) continue;
      const hit = re.exec(u.pathname);
      if (!hit) continue;
      const q = { ...Object.fromEntries(Object.entries(hit.groups || {}).map(([k, v]) => [k, decodeURIComponent(v)])), query: Object.fromEntries(u.searchParams), body, raw };
      try {
        const out = await fn(q);
        if (out instanceof Response) return out;
        return out === null || out === undefined ? new Response(null, { status: 204 }) : new Response(JSON.stringify(out), { headers: { "content-type": "application/json" } });
      } catch (e) {
        console.warn(method, u.pathname, e);
        return new Response(JSON.stringify({ error: String(e.message || e) }), { status: e.status || 400, headers: { "content-type": "application/json" } });
      }
    }
    return new Response(JSON.stringify({ error: "not found" }), { status: 404, headers: { "content-type": "application/json" } });
  }

  // ---------------------------------------------------------------- /ws
  let clientSeq = 1;
  /** Stands in for the WebSocket to the desktop server. */
  class LocalSocket extends EventTarget {
    constructor() {
      super();
      this.readyState = 0; this.me = clientSeq++; this.live = false;
      this.onopen = this.onmessage = this.onclose = this.onerror = null;
      setTimeout(() => {
        this.readyState = 1; sockets.add(this);
        const ev = new Event("open"); this.onopen && this.onopen(ev); this.dispatchEvent(ev);
        this._deliver(JSON.stringify({ t: "hello", client: this.me }));
      });
    }
    _deliver(s) { if (this.readyState !== 1) return; const ev = new MessageEvent("message", { data: s }); this.onmessage && this.onmessage(ev); this.dispatchEvent(ev); }
    send(s) {
      let m; try { m = JSON.parse(s); } catch (_) { return; }
      try {
        if (m.t === "param") { const v = setParam(m.node, m.param, m.value); emit({ t: "param", node: m.node, param: m.param, value: v, src: this.me }); }
        else if (m.t === "bypass") { setBypass(m.node, !!m.on); emit({ t: "bypass", node: m.node, on: !!m.on, src: this.me }); }
        else if (m.t === "move") { const n = node(m.node); if (n) { n.x = m.x; n.y = m.y; S.dirty = true; } emit({ t: "move", node: m.node, x: m.x, y: m.y, src: this.me }); }
        else if (m.t === "mute") { S.mute = !!m.on; SsbAudio.send({ t: "mute", on: S.mute }); emit({ t: "mute", on: S.mute }); }
        else if (m.t === "tap") { S.tap = Math.min(m.src | 0, 1); SsbAudio.send({ t: "tap", src: S.tap }); }
        else if (m.t === "live") {
          if (typeof m.win === "number" && isFinite(m.win)) S.liveWin = Math.min(Math.max(m.win, 0.002), 0.05);
          if (!!m.on !== this.live) { this.live = !!m.on; S.liveSubs += this.live ? 1 : -1; }
        }
      } catch (_) {}
    }
    close() { if (this.readyState === 3) return; if (this.live) S.liveSubs--; sockets.delete(this); this.readyState = 3; const ev = new CloseEvent("close"); this.onclose && this.onclose(ev); this.dispatchEvent(ev); }
  }

  async function init(engine, backend) {
    E = engine;
    E.liveReset(48000);
    await load(backend);
  }
  return { init, load, handle, LocalSocket, state: S, get store() { return store; }, saveNow: () => writeJson("boards/default.json", S.board), VERSION };
})();
