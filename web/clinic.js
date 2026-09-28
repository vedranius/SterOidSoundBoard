"use strict";
// DigiLingua — clinical view over the same board/engine as the pedalboard.
// Controls bind to nodes by `role` (mic, eq, daf, faf, interrupt, noise, ears).
(() => {
const ROLES = ["mic", "eq", "daf", "faf", "interrupt", "noise", "ears"];
const FREQS = [20, 25, 31.5, 40, 50, 63, 80, 100, 125, 160, 200, 250, 315, 400, 500, 630, 800, 1000, 1250, 1600, 2000, 2500, 3150, 4000, 5000, 6300, 8000, 10000, 12500, 16000, 20000];
const EQ_MIN = -48, EQ_MAX = 24;
const fk = (f) => (f >= 1000 ? f / 1000 + "k" : String(f));
// Quick EQ curves (31 values, dB); the full clinical presets live on the server.
const EQ_QUICK = {
  "Ravno": FREQS.map(() => 0),
  "Govor": FREQS.map((f) => (f < 100 ? -18 : f < 200 ? -8 : f >= 1000 && f <= 4000 ? 6 : f > 8000 ? -6 : 0)),
  "Telefon": FREQS.map((f) => (f < 300 || f > 3400 ? -40 : 0)),
  "Bas": FREQS.map((f) => (f <= 125 ? 8 : f <= 250 ? 4 : 0)),
  "Visoki": FREQS.map((f) => (f >= 8000 ? 8 : f >= 4000 ? 4 : 0)),
};
const TASKS = ["Produženi vokal /a/", "Produženi vokal /i/", "Produženi vokal /u/", "Čitanje standardnog teksta", "Spontani govor", "Brojanje / automatizirani govor", "Ponavljanje rečenica", "Glasovni raspon / glisando", "Pjevanje", "Kalibracija", "Ostalo"];
function fillTasks(sel, cur) {
  sel.innerHTML = ""; sel.add(new Option("— zadatak —", ""));
  TASKS.forEach((t) => sel.add(new Option(t, t)));
  if (cur && !TASKS.includes(cur)) sel.add(new Option(cur, cur));
  sel.value = cur || "";
}
fillTasks($("#clRecTask"), TASKS[0]);

const st = {
  patients: [], allSessions: [], patient: null, sessions: [], active: null, recs: [], tab: "rehab",
  recording: false, mutedByTab: false, visible: false, presets: [], currentPreset: null, presetDirty: false,
  settings: { clinic_name: "", clinician: "", analysis: {}, calibration_db: null }, earPrev: {},
};
const node = (role) => BOARD.nodes.find((n) => n.role === role);
const spec = (role, id) => { const n = node(role); return n && TYPES[n.kind].params.find((p) => p.id === id); };
const pval = (role, id) => { const n = node(role); if (!n) return 0; const s = spec(role, id); return n.params[id] ?? (s ? s.default : 0); };
const setP = (role, id, v) => { const n = node(role); if (n) setParam(n.id, id, v); };
const clamp = (v, a, b) => Math.max(a, Math.min(b, v));
const fmtTime = (ms) => new Date(ms).toLocaleString("hr-HR", { dateStyle: "short", timeStyle: "short" });
const fmtDate = (ms) => new Date(ms).toLocaleDateString("hr-HR");
const fmtDur = (s) => { s = Math.max(0, Math.round(s)); const h = Math.floor(s / 3600), m = Math.floor(s / 60) % 60, x = s % 60; return (h ? h + ":" : "") + String(m).padStart(2, "0") + ":" + String(x).padStart(2, "0"); };
const esc = (t) => String(t ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
const num = (v, d) => (v == null || !isFinite(v) ? "—" : v.toFixed(d).replace(".", ","));
const fold = (t) => String(t || "").toLowerCase().replace(/đ/g, "d").normalize("NFD").replace(/[̀-ͯ]/g, "");
const initials = (name) => String(name || "?").trim().split(/\s+/).slice(0, 2).map((w) => w[0]).join("").toUpperCase();
const lsGet = (k, d) => { try { return localStorage.getItem(k) ?? d; } catch (_) { return d; } };
const lsSet = (k, v) => { try { localStorage.setItem(k, v); } catch (_) {} };
const isOpen = (id) => !$(id).classList.contains("hidden");

// ============================================================ clinic settings
async function loadSettings() {
  try { st.settings = await api("GET", "/api/clinic/settings"); } catch (_) {}
  return st.settings;
}
const calibrated = () => st.settings && st.settings.calibration_db != null;

// ============================================================ patients
async function loadPatients(keep) {
  const [pts, all] = await Promise.all([api("GET", "/api/patients"), api("GET", "/api/sessions")]);
  st.patients = pts; st.allSessions = all;
  const want = keep ?? st.patient?.id ?? lsGet("ssb.patient", "");
  st.patient = st.patients.find((p) => p.id === want) || null;
  renderPList();
  await loadPatientData();
}
let clinicReloadT = 0;
function reloadSoon() { clearTimeout(clinicReloadT); clinicReloadT = setTimeout(() => loadPatients().catch((e) => toast(e.message)), 250); }

function renderPList() {
  const q = fold($("#clSearch").value.trim()), list = $("#clPList");
  const last = {};
  st.allSessions.forEach((s) => { if (!last[s.patient_id] || s.start > last[s.patient_id]) last[s.patient_id] = s.start; });
  const live = new Set(st.allSessions.filter((s) => !s.end).map((s) => s.patient_id));
  const match = st.patients.filter((p) => !q || fold([p.name, p.code, p.diagnosis, p.complaint].join(" ")).includes(q));
  $("#clPCount").textContent = st.patients.length ? `(${st.patients.length})` : "";
  list.innerHTML = "";
  if (!match.length) list.innerHTML = `<div class="dim small">${st.patients.length ? "Nema rezultata." : "Nema pacijenata — dodajte ih gumbom + Novi."}</div>`;
  match.forEach((p) => {
    const d = el("div", "pitem" + (st.patient?.id === p.id ? " sel" : ""));
    d.tabIndex = 0; d.setAttribute("role", "option"); d.dataset.id = p.id;
    const age = ageOf(p.birth);
    const meta = [p.code, age != null ? age + " god." : null, p.sex || null].filter(Boolean).map(esc).join(" · ");
    d.innerHTML = `<span class="avatar">${esc(initials(p.name))}</span><div class="grow"><div class="pn">${esc(p.name)}</div>` +
      (meta ? `<div class="small dim">${meta}</div>` : "") +
      `<div class="small dim">${last[p.id] ? "Zadnja sesija: " + fmtDate(last[p.id]) : "Bez sesija"}</div></div>` +
      (live.has(p.id) ? '<span class="live-dot" title="Sesija u tijeku"></span>' : "");
    d.onclick = () => selectPatient(p.id);
    d.onkeydown = (e) => { if (e.key === "Enter") selectPatient(p.id); };
    list.append(d);
  });
}
$("#clSearch").oninput = renderPList;
$("#clSearch").onkeydown = (e) => {
  if (e.key === "Enter") { const f = $("#clPList .pitem"); if (f) selectPatient(f.dataset.id); }
  if (e.key === "ArrowDown") { e.preventDefault(); $("#clPList .pitem")?.focus(); }
};
$("#clPList").addEventListener("keydown", (e) => {
  if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
  e.preventDefault();
  const a = document.activeElement, n = e.key === "ArrowDown" ? a.nextElementSibling : a.previousElementSibling;
  if (n && n.classList.contains("pitem")) n.focus(); else if (e.key === "ArrowUp") $("#clSearch").focus();
});
async function selectPatient(id) {
  st.patient = st.patients.find((p) => p.id === id) || null;
  lsSet("ssb.patient", st.patient ? st.patient.id : "");
  renderPList();
  try { await loadPatientData(); } catch (e) { toast(e.message); }
}

const P_FIELDS = ["name", "code", "birth", "sex", "language", "smoking", "diagnosis", "complaint", "history", "medical", "medications", "voice_use", "hearing", "goals", "notes"];
function ageOf(birth, at = Date.now()) {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(birth || ""); if (!m) return null;
  const d = new Date(at); let a = d.getFullYear() - +m[1];
  if (d.getMonth() + 1 < +m[2] || (d.getMonth() + 1 === +m[2] && d.getDate() < +m[3])) a--;
  return a >= 0 && a <= 130 ? a : null;
}
function openForm(p) {
  const f = $("#clPForm"); f.dataset.id = p ? p.id : "";
  P_FIELDS.forEach((k) => (f[k].value = p?.[k] || ""));
  f.ai_consent.checked = !!p?.ai_consent;
  $("#pTitle").textContent = p ? "Pacijent — uredi" : "Novi pacijent";
  $("#clPDel").classList.toggle("hidden", !p);
  showAge(); $("#pModal").classList.remove("hidden"); f.name.focus();
}
function closeForm() { $("#pModal").classList.add("hidden"); }
function showAge() { const a = ageOf($("#clPForm").birth.value); $("#pAge").textContent = a == null ? "" : `${a} god.`; }
$("#clPForm").birth.oninput = showAge;
$("#clNewP").onclick = () => openForm(null);
$("#clEditP").onclick = () => (st.patient ? openForm(st.patient) : toast("Odaberite pacijenta"));
$("#clPCancel").onclick = closeForm;
$("#clPForm").onsubmit = async (e) => {
  e.preventDefault(); const f = e.target;
  const body = Object.fromEntries(P_FIELDS.map((k) => [k, f[k].value.trim()]));
  body.ai_consent = f.ai_consent.checked;
  try {
    const p = f.dataset.id ? await api("PUT", "/api/patients/" + f.dataset.id, body) : await api("POST", "/api/patients", body);
    closeForm(); lsSet("ssb.patient", p.id);
    await loadPatients(p.id);
  } catch (er) { toast(er.message); }
};
$("#clPDel").onclick = async () => {
  const p = st.patient; if (!p) return;
  if (!confirm(`Trajno obrisati pacijenta "${p.name}" sa svim sesijama, snimkama i AI mišljenjima?`)) return;
  try { await api("DELETE", "/api/patients/" + p.id); closeForm(); await loadPatients(""); } catch (er) { toast(er.message); }
};
function renderPCard() {
  const p = st.patient;
  $("#clPCard").classList.toggle("hidden", !p);
  if (!p) return;
  $("#clPName").textContent = p.name + (p.code ? ` (${p.code})` : "");
  const age = ageOf(p.birth);
  const bits = [age != null ? age + " god." : null, p.sex || null, p.diagnosis || null].filter(Boolean).map(esc).join(" · ");
  $("#clPSummary").innerHTML = (bits ? `<div>${bits}</div>` : "") + (p.complaint ? `<div class="clip" title="${esc(p.complaint)}">${esc(p.complaint)}</div>` : "") +
    `<div>${p.ai_consent ? "✓ suglasnost za AI" : "✗ nema suglasnosti za AI"}</div>`;
}

async function loadPatientData() {
  const q = st.patient ? "?patient=" + encodeURIComponent(st.patient.id) : "";
  const [sessions, recs] = await Promise.all([
    st.patient ? api("GET", "/api/sessions" + q) : Promise.resolve([]),
    st.patient ? api("GET", "/api/recordings" + q) : Promise.resolve([]),
  ]);
  st.sessions = sessions; st.recs = recs;
  st.active = st.allSessions.find((s) => !s.end) || null;
  renderPCard(); renderSession(); renderRecs();
  if (st.tab === "napredak") loadProgress();
}

// ============================================================ sessions
let timerH = 0;
function renderSession() {
  const a = st.active, mine = a && st.patient && a.patient_id === st.patient.id;
  const other = a && !mine ? st.patients.find((p) => p.id === a.patient_id) : null;
  $("#clSessState").textContent = mine ? "Sesija u tijeku" : other ? `Aktivna sesija: ${other.name}` : "Nema aktivne sesije";
  $("#clSessPreset").textContent = mine && a.preset ? "Preset na početku: " + a.preset : "";
  $("#clSessBtn").textContent = mine ? "Završi sesiju" : "Započni sesiju";
  $("#clSessBtn").classList.toggle("danger", !!mine);
  const notes = $("#clSessNotes"); notes.classList.toggle("hidden", !mine);
  if (mine && document.activeElement !== notes) notes.value = a.notes || "";
  clearInterval(timerH); $("#clTimer").textContent = "";
  if (mine) { const tick = () => ($("#clTimer").textContent = fmtDur((Date.now() - a.start) / 1000)); tick(); timerH = setInterval(tick, 1000); }
  $("#clSessCount").textContent = st.sessions.length ? `(${st.sessions.length})` : "";
  const list = $("#clSessions"); list.innerHTML = "";
  if (!st.patient) list.innerHTML = '<div class="dim small">Odaberite pacijenta.</div>';
  else if (!st.sessions.length) list.innerHTML = '<div class="dim small">Nema sesija.</div>';
  st.sessions.forEach((s) => {
    const d = el("div", "cl-item click");
    d.innerHTML = `<div><b>${fmtTime(s.start)}</b> <span class="dim">${s.end ? fmtDur((s.end - s.start) / 1000) : "u tijeku"}</span></div>` +
      `<div class="small dim">${[s.preset ? esc(s.preset) : null, s.recordings ? s.recordings + " snim." : null].filter(Boolean).join(" · ")}</div>` +
      (s.notes ? `<div class="small dim clip">${esc(s.notes)}</div>` : "");
    d.title = "Detalji sesije"; d.onclick = () => openSession(s.id);
    list.append(d);
  });
}
$("#clSessBtn").onclick = async () => {
  if (!st.patient) return toast("Odaberite pacijenta");
  const a = st.active, mine = a && a.patient_id === st.patient.id;
  try {
    if (mine) await api("POST", `/api/sessions/${a.id}/stop`, { notes: $("#clSessNotes").value });
    else await api("POST", "/api/sessions", { patient_id: st.patient.id });
    await loadPatients();
  } catch (e) { toast(e.message); }
};
$("#clSessNotes").onchange = () => { if (st.active) api("PUT", "/api/sessions/" + st.active.id, { notes: $("#clSessNotes").value }).catch((e) => toast(e.message)); };

// Human-readable summary of a node's settings (session detail, chain chips).
function describeNode(role, kind, bypass, params) {
  const m = MODULES.find((x) => x.role === role); const info = TYPES[kind]; if (!info) return "";
  if (role === "eq") {
    const g = FREQS.map((_, i) => params[info.params[i].id] ?? 0);
    return eqMini(g) + (g.every((v) => v === 0) ? " ravno" : "");
  }
  if (!m) return "";
  return m.ctl.map(([id, label, a, , , unit, scale]) => {
    const v = params[id] ?? (info.params.find((p) => p.id === id) || {}).default ?? 0;
    if (Array.isArray(a)) return `${label}: ${a[Math.round(v)] ?? v}`;
    return `${label} ${+(v / (scale || 1)).toFixed(1)} ${unit}`;
  }).join(" · ");
}
function eqMini(g) {
  const y = (v) => ((EQ_MAX - v) / (EQ_MAX - EQ_MIN)) * 30;
  return `<svg class="eqmini" viewBox="0 0 124 30" preserveAspectRatio="none"><line x1="0" x2="124" y1="${y(0)}" y2="${y(0)}"/><polyline points="${g.map((v, i) => i * 4 + 2 + "," + y(v).toFixed(1)).join(" ")}"/></svg>`;
}
let sessOpen = null;
async function openSession(id) {
  try {
    const s = await api("GET", `/api/sessions/${id}/detail`); sessOpen = s;
    const p = st.patients.find((x) => x.id === s.patient_id);
    $("#sTitle").textContent = `Sesija ${fmtTime(s.start)}${p ? " — " + p.name : ""}`;
    const NAMES = { mic: "Mikrofon", eq: "EQ", daf: "DAF", faf: "FAF", interrupt: "Diskontinuitet", noise: "Šum", ears: "Uši" };
    const nodes = ROLES.map((r) => s.nodes.find((n) => n.role === r)).filter(Boolean);
    $("#sBody").innerHTML = `<div class="kv small"><span class="dim">Početak</span><b>${fmtTime(s.start)}</b><span class="dim">Kraj</span><b>${s.end ? fmtTime(s.end) : "u tijeku"}</b>
        <span class="dim">Trajanje</span><b>${s.end ? fmtDur((s.end - s.start) / 1000) : "—"}</b><span class="dim">Preset</span><b>${esc(s.preset || "—")}</b></div>
      <h4>Postavke na početku sesije</h4>` +
      (s.has_board ? `<table class="stable small"><tbody>${nodes.map((n) => `<tr class="${n.bypass && !["mic", "ears", "eq"].includes(n.role) ? "dim" : ""}"><td><b>${NAMES[n.role]}</b></td><td>${!["mic", "ears", "eq"].includes(n.role) ? (n.bypass ? "isključen" : "uključen") : ""}</td><td>${describeNode(n.role, n.kind, n.bypass, n.params)}</td></tr>`).join("")}</tbody></table>`
        : '<div class="dim small">Za ovu sesiju nisu spremljene postavke (starija verzija).</div>') +
      `<h4>Bilješke</h4><div class="small pre">${esc(s.notes || "—")}</div><h4>Snimke (${s.recordings.length})</h4>` +
      (s.recordings.length ? "" : '<div class="dim small">Nema snimaka u ovoj sesiji.</div>');
    const box = el("div", "cl-list");
    s.recordings.forEach((r) => { const d = el("div", "cl-item click", `<b>${esc(r.task || r.label || "Snimka")}</b> <span class="small dim">${fmtTime(r.created)} · ${r.duration.toFixed(1)} s</span>`); d.onclick = () => { closeSession(); openAnalysis(r); }; box.append(d); });
    $("#sBody").append(box);
    $("#sApply").disabled = !s.has_board;
    $("#sModal").classList.remove("hidden");
  } catch (e) { toast(e.message); }
}
function closeSession() { $("#sModal").classList.add("hidden"); }
$("#sClose").onclick = closeSession;
$("#sApply").onclick = async () => {
  if (!sessOpen || !confirm("Učitati postavke ove sesije? Trenutne postavke zvuka bit će zamijenjene.")) return;
  try { await api("POST", `/api/sessions/${sessOpen.id}/apply`); closeSession(); } catch (e) { toast(e.message); }
};

// ============================================================ recordings
function recTitle(r) { return r.task || r.label || "Snimka"; }
function renderRecs() {
  const list = $("#clRecs"); list.innerHTML = "";
  $("#clRecCount").textContent = st.recs.length ? `(${st.recs.length})` : "";
  if (!st.patient) list.innerHTML = '<div class="dim small">Odaberite pacijenta.</div>';
  else if (!st.recs.length) list.innerHTML = '<div class="dim small">Nema snimaka. Snimajte na tabu "Analiza glasa".</div>';
  st.recs.forEach((r) => {
    const d = el("div", "cl-item row");
    const info = el("div", "grow click");
    const nA = (r.annotations || []).length;
    info.innerHTML = `<b>${esc(recTitle(r))}</b>${r.task && r.label ? ` <span class="dim">${esc(r.label)}</span>` : ""}<div class="small dim">${fmtTime(r.created)} · ${r.duration.toFixed(1)} s · ${r.source === "out" ? "obrađeno" : "suhi mikrofon"}${nA ? ` · ${nA} ozn.` : ""}</div>`;
    info.onclick = () => openAnalysis(r);
    const del = el("button", "small", "✕"); del.title = "Obriši snimku";
    del.onclick = () => deleteRec(r);
    d.append(info, del); list.append(d);
  });
  renderRecTable();
}
async function deleteRec(r) {
  if (!confirm(`Obrisati snimku "${recTitle(r)}" (${fmtTime(r.created)}) s oznakama i AI mišljenjima?`)) return;
  try { await api("DELETE", "/api/recordings/" + r.id); } catch (e) { toast(e.message); }
}
function renderRecTable() {
  const box = $("#clRecTable");
  if (!st.patient) { box.innerHTML = '<div class="dim small">Odaberite pacijenta.</div>'; return; }
  if (!st.recs.length) { box.innerHTML = '<div class="dim small">Nema snimaka.</div>'; return; }
  const sess = Object.fromEntries(st.sessions.map((s) => [s.id, s]));
  box.innerHTML = `<table><thead><tr><th>Datum</th><th>Zadatak</th><th>Oznaka</th><th>Trajanje</th><th>Izvor</th><th>Sesija</th><th>Oznake</th><th></th></tr></thead><tbody>${st.recs.map((r) =>
    `<tr data-id="${r.id}"><td>${fmtTime(r.created)}</td><td>${esc(r.task || "—")}</td><td>${esc(r.label || "")}</td><td class="mono">${r.duration.toFixed(1)} s</td><td>${r.source === "out" ? "obrađeno" : "mikrofon"}</td><td>${r.session_id && sess[r.session_id] ? fmtDate(sess[r.session_id].start) : "—"}</td><td>${(r.annotations || []).length || ""}</td>
     <td class="acts"><button class="small pri" data-a="open">Analiza</button><a class="btn small" href="/api/recordings/${r.id}/wav" download>WAV</a><a class="btn small" href="/api/recordings/${r.id}/textgrid" download title="Praat TextGrid">TG</a><button class="small" data-a="del">✕</button></td></tr>`).join("")}</tbody></table>`;
  box.querySelectorAll("tr[data-id]").forEach((tr) => {
    const r = st.recs.find((x) => x.id === tr.dataset.id);
    tr.querySelector('[data-a="open"]').onclick = () => openAnalysis(r);
    tr.querySelector('[data-a="del"]').onclick = () => deleteRec(r);
  });
}

// ============================================================ tabs, mute
function setTab(t) {
  st.tab = t;
  document.querySelectorAll("#clTabs button").forEach((b) => b.classList.toggle("sel", b.dataset.tab === t));
  $("#clRehab").classList.toggle("hidden", t !== "rehab");
  $("#clAnaliza").classList.toggle("hidden", t !== "analiza");
  $("#clNapredak").classList.toggle("hidden", t !== "napredak");
  // progress review needs the room; devices and the live view stay on the working tabs
  document.querySelectorAll(".cl-status, .cl-live").forEach((e) => e.classList.toggle("hidden", t === "napredak"));
  lsSet("ssb.cltab", t);
  if (t === "napredak") loadProgress();
  liveSub(); applyTabMute();
}
function applyTabMute() {
  const want = st.visible && st.tab === "analiza" && $("#clAutoMute").checked;
  if (want && !st.mutedByTab) { st.mutedByTab = true; send({ t: "mute", on: true }); }
  else if (!want && st.mutedByTab) { st.mutedByTab = false; send({ t: "mute", on: false }); }
}
document.querySelectorAll("#clTabs button").forEach((b) => (b.onclick = () => setTab(b.dataset.tab)));
$("#clAutoMute").onchange = applyTabMute;
$("#clTap").onchange = () => { send({ t: "tap", src: +$("#clTap").value }); if (sono) { sono.frames = []; sono.full(); } };

// ============================================================ audio devices + status
const AUD = { devices: [], cfg: {}, running: null, error: null, frames: 0 };
async function loadDevices() {
  try {
    const [devs, s] = await Promise.all([api("GET", "/api/devices"), api("GET", "/api/status")]);
    AUD.devices = devs; AUD.cfg = s.config || {}; AUD.running = s.running; AUD.error = s.error;
    st.currentPreset = s.preset || null; renderPresetBadge();
  } catch (e) { toast(e.message); return; }
  const hs = $("#clHost"); hs.innerHTML = "";
  AUD.devices.forEach((h) => hs.add(new Option(h.host, h.host)));
  hs.value = AUD.cfg.host || AUD.running?.host || AUD.devices[0]?.host || "";
  fillDevs();
  $("#clBuf").value = AUD.cfg.buffer ? String(AUD.cfg.buffer) : "";
  $("#clNoIn").checked = !!AUD.cfg.no_input;
  showAudio();
}
function fillDevs() {
  const h = AUD.devices.find((d) => d.host === $("#clHost").value) || AUD.devices[0]; if (!h) return;
  const fill = (sel, list, cur, def) => {
    sel.innerHTML = ""; sel.add(new Option(`zadano (${def || "nema"})`, ""));
    list.forEach((n) => sel.add(new Option(n, n)));
    sel.value = list.includes(cur) ? cur : "";
  };
  fill($("#clIn"), h.inputs, AUD.cfg.input, h.default_input);
  fill($("#clOut"), h.outputs, AUD.cfg.output, h.default_output);
}
const audioCfg = () => ({ host: $("#clHost").value || null, input: $("#clIn").value || null, output: $("#clOut").value || null,
  buffer: $("#clBuf").value ? +$("#clBuf").value : null, no_input: $("#clNoIn").checked });
async function startAudio() {
  if (st.recording && !confirm("Ponovno pokretanje zvuka zaustavlja snimanje. Nastaviti?")) return;
  $("#clAudioBtn").disabled = true;
  try { AUD.cfg = audioCfg(); AUD.running = await api("POST", "/api/audio/start", AUD.cfg); AUD.error = null; }
  catch (e) { AUD.error = e.message; AUD.running = null; }
  finally { $("#clAudioBtn").disabled = false; }
  showAudio();
}
async function stopAudio() {
  if (st.recording && !confirm("Zaustavljanje zvuka zaustavlja snimanje. Nastaviti?")) return;
  try { await api("POST", "/api/audio/stop"); } catch (e) { toast(e.message); }
}
const toggleAudio = () => (AUD.running ? stopAudio() : startAudio());
$("#clAudioBtn").onclick = toggleAudio;
$("#clHost").onchange = () => { fillDevs(); if (AUD.running) startAudio(); };
["#clIn", "#clOut", "#clBuf", "#clNoIn"].forEach((s) => ($(s).onchange = () => { AUD.cfg = audioCfg(); if (AUD.running) startAudio(); }));
$("#clDevRefresh").onclick = () => loadDevices().then(() => toastInfo("Popis uređaja osvježen"));
function showAudio() {
  const r = AUD.running, led = $("#clLed");
  led.className = "led " + (AUD.error ? "bad" : r ? "ok" : "");
  $("#clLedTxt").textContent = AUD.error ? "Greška" : r ? "Radi" : "Zaustavljeno";
  $("#clAudioErr").textContent = AUD.error || "";
  const b = $("#clAudioBtn");
  b.textContent = r ? "■ Zaustavi zvuk" : "▶ Pokreni zvuk";
  b.classList.toggle("danger", !!r); b.classList.toggle("pri", !r);
  $("#clSr").textContent = r ? `${(r.sample_rate / 1000).toFixed(1)} kHz` : "—";
  showStats();
}
function showStats(m) {
  const r = AUD.running;
  if (m) { AUD.frames = m.frames; $("#clDsp").textContent = Math.round(m.load * 100) + " %"; $("#clXr").textContent = m.xruns; $("#clDsp").classList.toggle("warnc", m.load > 0.8); }
  const fr = AUD.frames || r?.buffer || 0;
  $("#clFrames").textContent = r && fr ? `${fr} okv. (${(fr / r.sample_rate * 1000).toFixed(1)} ms)` : "—";
  // output callback + up to two queued input callbacks + the input device's own buffer
  $("#clLat").textContent = r && fr ? `≈ ${(3 * fr / r.sample_rate * 1000).toFixed(0)} ms` : "—";
  if (!r) { $("#clDsp").textContent = "—"; $("#clXr").textContent = "—"; }
}
function toastInfo(msg) { const t = $("#toast"); t.classList.add("info"); toast(msg); clearTimeout(t._i); t._i = setTimeout(() => t.classList.remove("info"), 3200); }

// ============================================================ live sonagram + VU
let sono = null;
function ensureSono() { if (!sono) sono = new Sono.LiveSono($("#clSonoHost"), { onWindow: () => liveSub() }); }
function liveSub() {
  const on = st.visible && !document.hidden && !isOpen("#anModal") && st.tab !== "napredak";
  send({ t: "live", on, win: sono ? sono.win : 0.005 });
}
document.addEventListener("visibilitychange", liveSub);
const VU = { in: { hold: -90, t: 0, clip: 0 }, outL: { hold: -90, t: 0, clip: 0 }, outR: { hold: -90, t: 0, clip: 0 } };
function vuRow(key, pk) {
  const row = document.querySelector(`#clVu2 [data-src="${key}"]`); if (!row) return;
  const db = 20 * Math.log10(pk + 1e-9), now = performance.now(), h = VU[key];
  if (db >= h.hold || now - h.t > 2000) { h.hold = db; h.t = now; }
  if (pk >= 0.999) h.clip = now;
  const pct = (d) => clamp(((d + 60) / 60) * 100, 0, 100);
  row.querySelector("i").style.left = pct(db) + "%";
  row.querySelector("s").style.left = `calc(${pct(h.hold)}% - 2px)`;
  row.querySelector(".db").textContent = h.hold > -90 ? h.hold.toFixed(1) : "—";
  row.classList.toggle("clip", now - h.clip < 1500);
}
function vu(m) {
  vuRow("in", Math.max(m.in[0], m.in[1]));
  vuRow("outL", m.out[0]); vuRow("outR", m.out[1]);
}
function syncEars() {
  document.querySelectorAll("#clVu2 .ear").forEach((b) => {
    const muted = !!node("ears") && pval("ears", b.dataset.ear) <= 0;
    b.textContent = muted ? "🔇" : "🔊"; b.classList.toggle("warn", muted);
    b.title = (muted ? "Uključi " : "Utišaj ") + (b.dataset.ear === "left" ? "lijevo uho" : "desno uho");
    b.disabled = !node("ears");
  });
}
document.querySelectorAll("#clVu2 .ear").forEach((b) => (b.onclick = () => {
  const ear = b.dataset.ear, v = pval("ears", ear);
  if (v > 0) { st.earPrev[ear] = v; setP("ears", ear, 0); } else setP("ears", ear, st.earPrev[ear] || 100);
  syncModules(); syncEars();
}));

// ============================================================ signal chain chips
const CHAIN = [["mic", "Mikrofon"], ["eq", "EQ"], ["daf", "DAF"], ["faf", "FAF"], ["interrupt", "Diskont."], ["noise", "Šum"], ["ears", "Uši"]];
function chainInfo(role) {
  const n = node(role); if (!n) return "nema";
  const r = (v) => Math.round(v);
  switch (role) {
    case "mic": return ["stereo", "L → oba", "D → oba", "mono"][r(pval("mic", "source"))] || "";
    case "eq": { const k = FREQS.filter((_, i) => pval("eq", TYPES.eq31.params[i].id) !== 0).length; return k ? k + " pojasa" : "ravno"; }
    case "daf": return r(pval("daf", "time")) + " ms";
    case "faf": { const s = pval("faf", "semi"); return (s > 0 ? "+" : "") + s + " st"; }
    case "interrupt": return `${r(pval("interrupt", "period"))}/${r(pval("interrupt", "gap"))} ms`;
    case "noise": return r(pval("noise", "level")) + " dB";
    case "ears": return `${r(pval("ears", "left"))}/${r(pval("ears", "right"))} %`;
  }
  return "";
}
function renderChain() {
  const box = $("#clChain"); box.innerHTML = "";
  CHAIN.forEach(([role, label], i) => {
    const n = node(role), fixed = role === "mic" || role === "ears";
    const c = el("button", "chip " + (!n ? "missing" : fixed || !n.bypass ? "on" : "off") + (fixed ? " fixed" : ""));
    c.innerHTML = `<b>${label}</b><small>${esc(chainInfo(role))}</small>`;
    if (n && !fixed) { c.title = n.bypass ? "Uključi" : "Isključi"; c.onclick = () => { setBypass(n.id, !n.bypass); syncAll(); }; }
    else c.tabIndex = -1;
    box.append(c);
    if (i < CHAIN.length - 1) box.append(el("span", "arrow", "›"));
  });
}

// ============================================================ EQ (sliders + curve)
const eqSvg = $("#clEq");
const eqY = (g) => ((EQ_MAX - g) / (EQ_MAX - EQ_MIN)) * 144;
const eqX = (i) => i * 10 + 5;
const band = (i) => pval("eq", TYPES.eq31.params[i].id);
function setBand(i, g) {
  if (!node("eq")) return;
  g = clamp(Math.round(g * 2) / 2, EQ_MIN, EQ_MAX); if (Math.abs(g) < 0.5) g = 0;
  setP("eq", TYPES.eq31.params[i].id, g);
  $("#clEqVal").textContent = `${fk(FREQS[i])} Hz: ${g > 0 ? "+" : ""}${g} dB`;
  drawEq(); renderChainSoon();
}
function resetEq() { if (!node("eq")) return; FREQS.forEach((_, i) => setP("eq", TYPES.eq31.params[i].id, 0)); $("#clEqVal").textContent = "sve 0 dB"; drawEq(); renderChainSoon(); }
function drawEq() {
  const n = node("eq"), gains = FREQS.map((_, i) => (n ? band(i) : 0));
  let h = "";
  for (let g = EQ_MIN; g <= EQ_MAX; g += 12) h += `<line x1="0" x2="310" y1="${eqY(g)}" y2="${eqY(g)}" class="${g === 0 ? "zero" : "grid"}"/><text x="2" y="${eqY(g) - 1.5}" class="lbl">${g > 0 ? "+" + g : g}</text>`;
  gains.forEach((g, i) => { h += `<rect x="${eqX(i) - 3}" width="6" y="${Math.min(eqY(g), eqY(0))}" height="${Math.abs(eqY(g) - eqY(0))}" class="bar"/>`; });
  h += `<polyline points="${gains.map((g, i) => eqX(i) + "," + eqY(g)).join(" ")}" class="curve"/>`;
  gains.forEach((g, i) => { h += `<circle cx="${eqX(i)}" cy="${eqY(g)}" r="2.2" class="dot"/>`; });
  eqSvg.innerHTML = h;
  const off = !n || n.bypass;
  eqSvg.classList.toggle("off", off); $("#clEqSliders").classList.toggle("off", off);
  const pct = (g) => ((EQ_MAX - g) / (EQ_MAX - EQ_MIN)) * 100, z = pct(0);
  document.querySelectorAll("#clEqSliders .eqb").forEach((c, i) => {
    const g = gains[i], p = pct(g);
    c.querySelector(".fill").style.cssText = `top:${Math.min(p, z)}%;height:${Math.abs(p - z)}%`;
    c.querySelector(".th").style.top = p + "%";
    c.querySelector(".v").textContent = (g > 0 ? "+" : "") + g;
    c.querySelector(".tr").setAttribute("aria-valuenow", g);
    c.classList.toggle("nz", g !== 0);
  });
}
function buildEqSliders() {
  const box = $("#clEqSliders"); box.innerHTML = "";
  const z = ((EQ_MAX) / (EQ_MAX - EQ_MIN)) * 100;
  FREQS.forEach((f, i) => {
    const grp = i < 9 ? 0 : i < 16 ? 1 : i < 23 ? 2 : 3;
    const c = el("div", "eqb g" + grp);
    c.innerHTML = `<span class="v mono">0</span><div class="tr" tabindex="0" role="slider" aria-label="${fk(f)} Hz" aria-valuemin="${EQ_MIN}" aria-valuemax="${EQ_MAX}">` +
      `<div class="zero" style="top:${z}%"></div><div class="fill"></div><div class="th"></div></div><span class="f mono">${fk(f)}</span>`;
    const tr = c.querySelector(".tr");
    const fromY = (e) => { const r = tr.getBoundingClientRect(); setBand(i, EQ_MAX - ((e.clientY - r.top) / r.height) * (EQ_MAX - EQ_MIN)); };
    tr.addEventListener("pointerdown", (e) => { if (!node("eq")) return; e.preventDefault(); tr.setPointerCapture(e.pointerId); tr.focus(); fromY(e); });
    tr.addEventListener("pointermove", (e) => { if (tr.hasPointerCapture(e.pointerId)) fromY(e); });
    tr.addEventListener("dblclick", () => setBand(i, 0));
    tr.addEventListener("keydown", (e) => {
      const s = e.shiftKey ? 3 : 0.5;
      const d = { ArrowUp: s, ArrowRight: s, ArrowDown: -s, ArrowLeft: -s, PageUp: 6, PageDown: -6 }[e.key];
      if (d != null) { e.preventDefault(); e.stopPropagation(); setBand(i, band(i) + d); }
      else if (e.key === "Home" || e.key === "0") { e.preventDefault(); e.stopPropagation(); setBand(i, 0); }
    });
    tr.addEventListener("focus", () => ($("#clEqVal").textContent = `${fk(f)} Hz: ${band(i) > 0 ? "+" : ""}${band(i)} dB`));
    box.append(c);
  });
}
function eqPointer(e) {
  if (!(e.buttons & 1) || !node("eq")) return;
  e.preventDefault();
  const r = eqSvg.getBoundingClientRect();
  const i = clamp(Math.floor(((e.clientX - r.left) / r.width) * 31), 0, 30);
  setBand(i, EQ_MAX - ((e.clientY - r.top) / r.height) * (EQ_MAX - EQ_MIN));
}
eqSvg.addEventListener("pointerdown", (e) => { eqSvg.setPointerCapture(e.pointerId); eqPointer(e); });
eqSvg.addEventListener("pointermove", eqPointer);
$("#clEqLabels").innerHTML = FREQS.map((f) => `<span>${fk(f)}</span>`).join("");
function setEqView(v) {
  document.querySelectorAll("#clEqView button").forEach((b) => b.classList.toggle("sel", b.dataset.v === v));
  $("#clEqSliders").classList.toggle("hidden", v !== "sliders"); $("#clEqCurve").classList.toggle("hidden", v !== "curve");
  lsSet("ssb.eqview", v);
}
document.querySelectorAll("#clEqView button").forEach((b) => (b.onclick = () => setEqView(b.dataset.v)));
(() => { const q = $("#clEqQuick"); q.add(new Option("Brze krivulje…", "")); Object.keys(EQ_QUICK).forEach((k) => q.add(new Option(k, k)));
  q.onchange = () => { const c = EQ_QUICK[q.value]; q.value = ""; if (!c || !node("eq")) return; c.forEach((g, i) => setP("eq", TYPES.eq31.params[i].id, g)); drawEq(); renderChainSoon(); }; })();
$("#clEqReset").onclick = resetEq;

// ============================================================ clinical presets
async function loadPresets() {
  try { st.presets = await api("GET", "/api/clinic/presets"); } catch (e) { toast(e.message); }
  renderPresets();
}
function renderPresetBadge() {
  const b = $("#clPresetBadge");
  b.classList.toggle("hidden", !st.currentPreset);
  b.textContent = st.currentPreset ? "Preset: " + st.currentPreset + (st.presetDirty ? " • izmijenjeno" : "") : "";
  b.classList.toggle("dirty", st.presetDirty);
}
function renderPresets() {
  const box = $("#clPresetList"); box.innerHTML = "";
  const cats = [];
  st.presets.forEach((p) => { if (!cats.includes(p.category)) cats.push(p.category); });
  cats.forEach((c) => {
    box.append(el("div", "pcat", esc(c)));
    st.presets.filter((p) => p.category === c).forEach((p) => {
      const d = el("div", "pr" + (st.currentPreset === p.name ? " sel" : ""));
      d.innerHTML = `<div class="grow"><b>${esc(p.name)}</b>${p.factory ? ' <span class="tag">tvornički</span>' : ""}<div class="small dim">${esc(p.description || "")}</div></div>`;
      d.title = "Učitaj preset"; d.tabIndex = 0;
      d.onclick = () => loadPreset(p);
      d.onkeydown = (e) => { if (e.key === "Enter") loadPreset(p); };
      if (!p.factory) {
        const x = el("button", "small", "✕"); x.title = "Obriši preset";
        x.onclick = async (e) => { e.stopPropagation(); if (!confirm(`Obrisati preset "${p.name}"?`)) return; try { await api("DELETE", "/api/clinic/presets/" + encodeURIComponent(p.id)); } catch (er) { toast(er.message); } };
        d.append(x);
      }
      box.append(d);
    });
  });
}
async function loadPreset(p) {
  if (BOARD.nodes.length && ROLES.some((r) => !node(r)) && !confirm("Preset zamjenjuje trenutni board DigiLingua lancem. Nastaviti?")) return;
  try { await api("POST", `/api/clinic/presets/${encodeURIComponent(p.id)}/load`); toastInfo("Učitano: " + p.name); } catch (e) { toast(e.message); }
}
$("#clPresetSave").onclick = () => {
  if (!ROLES.every((r) => node(r))) return toast("Učitajte DigiLingua lanac prije spremanja preseta");
  const dlg = el("div", "modal"); dlg.style.zIndex = 70;
  dlg.innerHTML = `<form class="modal-in card2 small-dlg"><h3>Spremi klinički preset</h3>
    <label>Naziv * <input name="name" required maxlength="120" value="${esc(st.currentPreset && st.presetDirty ? st.currentPreset + " (prilagođeno)" : "")}"></label>
    <label>Kategorija <input name="category" maxlength="100" list="clPresetCats" placeholder="npr. Mucanje, Glas, Sluh"></label>
    <label>Opis <textarea name="description" rows="3" maxlength="2000" placeholder="npr. DAF 120 ms za pacijente s blokovima; smanjivati kroz terapiju"></textarea></label>
    <datalist id="clPresetCats">${[...new Set(st.presets.filter((p) => !p.factory).map((p) => p.category))].map((c) => `<option value="${esc(c)}">`).join("")}</datalist>
    <div class="dim small">Spremaju se sve postavke: EQ, DAF, FAF, šum, diskontinuitet, glasnoća po uhu i uključeni moduli.</div>
    <div class="row"><button class="pri" type="submit">Spremi</button><button type="button" data-x>Odustani</button></div></form>`;
  document.body.append(dlg);
  const f = dlg.querySelector("form"); f.name.focus();
  dlg.querySelector("[data-x]").onclick = () => dlg.remove();
  f.onsubmit = async (e) => {
    e.preventDefault();
    try {
      await api("POST", "/api/clinic/presets", { name: f.name.value.trim(), category: f.category.value.trim(), description: f.description.value.trim() });
      dlg.remove(); toastInfo("Preset spremljen");
      st.currentPreset = f.name.value.trim(); st.presetDirty = false; renderPresetBadge();
    }
    catch (er) { toast(er.message); }
  };
};

// ============================================================ modules (DAF, FAF, noise, interrupter, mic/ears)
// Each control: [param id, label, min, max, step, unit, scale]
const MODULES = [
  { role: "daf", title: "DAF — odgođena slušna povratna veza", hint: "Pacijent čuje samo odgođeni glas (bez jeke kad je suhi glas 0 %).",
    ctl: [["time", "Kašnjenje", 0, 5000, 10, "ms"], ["dry", "Vlastiti (suhi) glas", 0, 100, 1, "%", 0.01], ["wet", "Odgođeni glas", 0, 100, 1, "%", 0.01]] },
  { role: "faf", title: "FAF — frekvencijski promijenjena povratna veza", hint: "Pomak visine glasa u polutonovima (±12 = oktava).",
    ctl: [["semi", "Pomak", -12, 12, 0.5, "st"], ["mix", "Udio pomaknutog", 0, 100, 1, "%", 0.01]] },
  { role: "noise", title: "Maskirajući šum", hint: "Razina je RMS u dBFS. Uskopojasni šum: 1/3 oktave oko središnje frekvencije.",
    ctl: [["color", "Vrsta", ["Bijeli", "Ružičasti", "Smeđi", "Uskopojasni"]], ["level", "Razina", -80, 0, 1, "dB"], ["freq", "Središnja frekv.", 100, 8000, 10, "Hz"], ["route", "Uho", ["Oba", "Lijevo", "Desno"]]] },
  { role: "interrupt", title: "Diskontinuitet (prekidanje)", hint: "Periodično utišavanje signala — trajanje pauze na kraju svakog perioda.",
    ctl: [["period", "Period", 50, 5000, 10, "ms"], ["gap", "Pauza", 10, 2000, 10, "ms"], ["depth", "Dubina", 0, 100, 1, "%", 0.01]] },
  { role: "mic", title: "Mikrofon", hint: "Mono mikrofon na lijevom ulazu → oba uha.", always: true,
    ctl: [["source", "Ulaz", ["Stereo", "Lijevi → oba", "Desni → oba", "Mono zbroj"]]] },
  { role: "ears", title: "Slušalice — glasnoća po uhu", hint: "0–200 %. Utišavanje jednog uha: gumb 🔊 kod mjerača razine.", always: true, link: true,
    ctl: [["left", "Lijevo uho", 0, 200, 1, "%"], ["right", "Desno uho", 0, 200, 1, "%"]] },
];
function buildModules() {
  const box = $("#clModules"); box.innerHTML = "";
  MODULES.forEach((m) => {
    const c = el("div", "card2 mod"); c.dataset.role = m.role;
    const h = el("div", "h row"); h.append(el("span", "grow", m.title));
    if (!m.always) {
      const t = el("button", "tog", "ISKLJ."); t.onclick = () => { const n = node(m.role); if (n) { setBypass(n.id, !n.bypass); syncAll(); } };
      h.append(t);
    }
    c.append(h, el("div", "dim small", m.hint));
    m.ctl.forEach(([id, label, a, b, step, unit, scale]) => {
      const row = el("label", "ctl");
      row.append(el("span", null, label));
      if (Array.isArray(a)) {
        const s = el("select"); a.forEach((o, i) => s.add(new Option(o, i))); s.dataset.p = id;
        s.onchange = () => { setP(m.role, id, +s.value); syncModules(); renderChainSoon(); };
        row.append(s);
      } else {
        const r = el("input"); r.type = "range"; r.min = a; r.max = b; r.step = step; r.dataset.p = id; r.dataset.scale = scale || 1;
        const v = el("input", "num mono"); v.type = "number"; v.min = a; v.max = b; v.step = step;
        const apply = (x) => {
          x = clamp(+x, a, b); r.value = x; v.value = x;
          setP(m.role, id, x * (scale || 1));
          if (m.link && $("#clLink")?.checked) { const other = id === "left" ? "right" : "left"; setP(m.role, other, x); syncModules(); }
          if (m.role === "ears") syncEars();
          renderChainSoon();
        };
        r.oninput = () => apply(r.value); v.onchange = () => apply(v.value);
        r.ondblclick = () => { const s = spec(m.role, id); if (s) apply(s.default / (scale || 1)); };
        row.append(r, v, el("span", "dim small", unit));
      }
      c.append(row);
    });
    if (m.link) { const l = el("label", "chk small"); l.innerHTML = '<input type="checkbox" id="clLink" checked> poveži L/D'; c.append(l); }
    box.append(c);
  });
}
function syncModules() {
  document.querySelectorAll("#clModules .mod").forEach((c) => {
    const n = node(c.dataset.role);
    c.classList.toggle("missing", !n);
    const t = c.querySelector(".tog");
    if (t) { t.textContent = n && !n.bypass ? "UKLJ." : "ISKLJ."; t.classList.toggle("on", !!n && !n.bypass); }
    c.classList.toggle("byp", !!n && n.bypass);
    c.querySelectorAll("[data-p]").forEach((inp) => {
      if (!n || document.activeElement === inp) return;
      const v = pval(c.dataset.role, inp.dataset.p);
      if (inp.tagName === "SELECT") inp.value = Math.round(v);
      else { const x = +(v / (+inp.dataset.scale || 1)).toFixed(2); inp.value = x; inp.nextSibling.value = x; }
    });
  });
  const color = Math.round(pval("noise", "color"));
  const fr = document.querySelector('#clModules [data-role="noise"] [data-p="freq"]');
  if (fr) fr.parentElement.classList.toggle("hidden", color !== 3);
}
let chainRaf = 0;
function renderChainSoon() { if (!chainRaf) chainRaf = requestAnimationFrame(() => { chainRaf = 0; renderChain(); syncEars(); }); }
function syncAll() {
  const missing = ROLES.filter((r) => !node(r));
  $("#clMissing").classList.toggle("hidden", !missing.length);
  drawEq(); syncModules(); renderChainSoon();
}
$("#clLoadChain").onclick = async () => {
  if (BOARD.nodes.length && !confirm("Zamijeniti trenutni board DigiLingua lancem?")) return;
  try { await api("POST", "/api/templates/digilingua"); } catch (e) { toast(e.message); }
};

// ============================================================ recording
$("#clRecBtn").onclick = async () => {
  try {
    if (st.recording) { const r = await api("POST", "/api/record/stop"); toastInfo(`Spremljeno: ${r.duration.toFixed(1)} s`); }
    else {
      if (!AUD.running) return toast("Zvuk nije pokrenut — kliknite ▶ Pokreni zvuk");
      if (!st.patient && !confirm("Nije odabran pacijent — snimiti bez pacijenta?")) return;
      await api("POST", "/api/record/start", { patient_id: st.patient?.id || null, source: $("#clRecSrc").value, label: $("#clRecLabel").value.trim(), task: $("#clRecTask").value });
    }
  } catch (e) { toast(e.message); }
};
function showRec(on, secs) {
  st.recording = on;
  const b = $("#clRecBtn"); b.classList.toggle("live", on);
  b.textContent = on ? `ZAVRŠI SNIMANJE  ${fmtDur(secs || 0)}` : "ZAPOČNI SNIMANJE";
}

// ============================================================ measures (shared by the analysis view, print, export)
// Row: [label, value text, unit, reference, ok?]; a single-element row is a group heading.
function measureRows(r) {
  const f = (v, d) => (v == null || !isFinite(v) ? null : num(v, d));
  const chk = (v, ok) => (v == null ? undefined : ok);
  const cal = calibrated() ? st.settings.calibration_db : null;
  const iu = cal != null ? "dB SPL" : "dB (rel.)";
  const iv = (v) => (v == null ? null : num(v + (cal || 0), 1));
  const fm = r.formants || [];
  return [
    ["Visina (Pitch)"],
    ["Medijan F0", f(r.f0_median, 2), "Hz"], ["Prosjek F0", f(r.f0_mean, 2), "Hz"], ["Standardna devijacija F0", f(r.f0_sd, 2), "Hz"],
    ["Minimum / maksimum F0", r.f0_min != null ? `${num(r.f0_min, 1)} / ${num(r.f0_max, 1)}` : null, "Hz"], ["Raspon F0", f(r.f0_range_st, 1), "st"],
    ["Pulsevi (Pulses)"],
    ["Broj pulseva", r.pulses, ""], ["Broj perioda", r.periods, ""], ["Prosječni period", f(r.mean_period_ms, 3), "ms"], ["SD perioda", f(r.sd_period_ms, 3), "ms"],
    ["Zvučnost (Voicing)"],
    ["Udio nezvučnih okvira", f(r.unvoiced_fraction, 1), "%"], ["Broj prekida zvučnosti", r.voice_breaks, ""], ["Stupanj prekida zvučnosti", f(r.voice_break_degree, 1), "%"],
    ["Jitter"],
    ["Jitter (local)", f(r.jitter_local, 3), "%", "< 1,04", chk(r.jitter_local, r.jitter_local <= 1.04)],
    ["Jitter (local, absolute)", f(r.jitter_abs_us, 2), "µs", "< 83,2", chk(r.jitter_abs_us, r.jitter_abs_us <= 83.2)],
    ["Jitter (rap)", f(r.jitter_rap, 3), "%", "< 0,68", chk(r.jitter_rap, r.jitter_rap <= 0.68)],
    ["Jitter (ppq5)", f(r.jitter_ppq5, 3), "%", "< 0,84", chk(r.jitter_ppq5, r.jitter_ppq5 <= 0.84)],
    ["Jitter (ddp)", f(r.jitter_ddp, 3), "%"],
    ["Shimmer"],
    ["Shimmer (local)", f(r.shimmer_local, 3), "%", "< 3,81", chk(r.shimmer_local, r.shimmer_local <= 3.81)],
    ["Shimmer (local, dB)", f(r.shimmer_db, 3), "dB", "< 0,35", chk(r.shimmer_db, r.shimmer_db <= 0.35)],
    ["Shimmer (apq3)", f(r.shimmer_apq3, 3), "%"], ["Shimmer (apq5)", f(r.shimmer_apq5, 3), "%"],
    ["Shimmer (apq11)", f(r.shimmer_apq11, 3), "%", "< 3,07", chk(r.shimmer_apq11, r.shimmer_apq11 <= 3.07)],
    ["Shimmer (dda)", f(r.shimmer_dda, 3), "%"],
    ["Harmoničnost (Harmonicity)"],
    ["Srednja autokorelacija", f(r.mean_autocorrelation, 4), ""],
    ["Omjer šum/harmonici (NHR)", f(r.nhr, 4), "", "< 0,19", chk(r.nhr, r.nhr <= 0.19)],
    ["Omjer harmonici/šum (HNR)", f(r.hnr_db, 2), "dB", "> 20", chk(r.hnr_db, r.hnr_db >= 20)],
    ["Kepstralna analiza"],
    ["CPPS (AVQI postavke)", f(r.cpps, 2), "dB", r.cpps_note ? "*" : ""],
    ["Formanti (medijan zvučnih okvira)"],
    ...[0, 1, 2, 3].map((k) => [`F${k + 1}`, f(fm[k], 0), "Hz"]),
    ["Intenzitet"],
    ["Prosjek", iv(r.intensity_mean_db), iu], ["Minimum / maksimum", r.intensity_min_db != null ? `${iv(r.intensity_min_db)} / ${iv(r.intensity_max_db)}` : null, iu],
    ["Standardna devijacija", f(r.intensity_sd_db, 2), "dB"], ["Razina prema digitalnom maksimumu", f(r.intensity_dbfs, 1), "dBFS"],
    ["Vremenske mjere"],
    ["Najduža fonacija (MPT)", f(r.max_voiced_s, 2), "s"], ["Pauze ≥ 250 ms", r.pauses, ""], ["Prosječna pauza", f(r.pause_mean_s, 2), "s"], ["Udio pauza", f(r.pause_ratio, 1), "%"],
    ["Slogovne jezgre", r.syllable_nuclei, ""], ["Brzina govora", f(r.speech_rate, 2), "slog/s"], ["Brzina artikulacije", f(r.articulation_rate, 2), "slog/s"],
  ];
}
function measuresHtml(r) {
  return `<table><thead><tr><th>Mjera</th><th class="r">Vrijednost</th><th></th><th class="r">Orijent. granica</th></tr></thead><tbody>${measureRows(r).map((x) => x.length === 1
    ? `<tr class="grp"><td colspan="4">${esc(x[0])}</td></tr>`
    : `<tr class="${x[4] === false ? "bad" : x[4] === true ? "ok" : ""}"><td>${esc(x[0])}</td><td class="r mono">${x[1] ?? "—"}</td><td class="dim">${esc(x[2])}</td><td class="r dim">${esc(x[3] || "")}</td></tr>`).join("")}</tbody></table>
    <div class="dim small">Algoritmi su port Praata (Boersma &amp; Weenink), provjereni prema Praat 7.0. Granice su orijentacijske (MDVP/literatura) i ovise o zadatku, mikrofonu i prostoriji.${r.cpps_note ? " * " + esc(r.cpps_note) : ""}</div>`;
}

// ============================================================ analysis modal (Praat-style editor)
const an = { rec: null, editor: null, report: null, sel: null, settings: null, ai: null, summary: null };
async function openAnalysis(r) {
  closeAnalysis(true);
  an.rec = r; an.report = null; an.ai = null; an.sel = null; an.summary = null;
  await loadSettings();
  an.settings = Object.assign({}, st.settings.analysis);
  $("#anModal").classList.remove("hidden"); liveSub();
  const sess = r.session_id && st.sessions.find((s) => s.id === r.session_id);
  $("#anTitle").textContent = `${st.patient && r.patient_id === st.patient.id ? st.patient.name + " — " : ""}${recTitle(r)} · ${fmtTime(r.created)}${sess ? " · sesija " + fmtDate(sess.start) : ""}`;
  $("#anWav").href = `/api/recordings/${r.id}/wav`; $("#anWav").download = "";
  $("#anTg").href = `/api/recordings/${r.id}/textgrid`; $("#anTg").download = "";
  $("#anReport").value = "Učitavanje…"; $("#anTiles").innerHTML = ""; $("#anMeasures").innerHTML = ""; $("#anSummary").innerHTML = ""; $("#anSelInfo").textContent = "";
  fillTasks($("#anTask"), r.task); $("#anNotes").value = r.notes || ""; $("#anSyl").value = r.syllables || "";
  $("#anAiOut").innerHTML = ""; $("#anAiPrompt").classList.add("hidden"); $("#anAiQ").value = "";
  aiStatus(); loadAiHistory(); loadSummary(); aiRestore(r);
  an.editor = new Editor.SoundEditor($("#anEditor"), {
    rec: r, analysis: an.settings,
    onAnalyze: (sel, settings) => { an.settings = settings; runAnalysis(sel); },
    onAnnotations: (anns) => { r.annotations = anns; loadSummary(); renderRecs(); },
    onSettings: (na) => { an.settings = na; runAnalysis(an.sel); },
  });
  try { await an.editor.load(); await runAnalysis([0, an.editor.dur]); }
  catch (e) { $("#anReport").value = "Greška: " + e.message; }
}
function closeAnalysis(silent) {
  if (an.editor) { an.editor.destroy(); an.editor = null; }
  aiRun = null; clearInterval(aiTick); // the task keeps running on the server; reopening follows it again
  $("#anEditor").innerHTML = "";
  if (!silent) { $("#anModal").classList.add("hidden"); $("#anModal").classList.remove("max"); liveSub(); }
}
$("#anClose").onclick = () => closeAnalysis();
$("#anMax").onclick = () => { $("#anModal").classList.toggle("max"); };
// Two passes: the quick Praat measures first, then the same report with CPPS
// (its robust trend line is O(n²) per frame, as in Praat) replaces it.
let anSeq = 0;
async function runAnalysis(sel) {
  if (!an.rec) return;
  sel = sel || [0, an.editor ? an.editor.dur : an.rec.duration];
  const seq = ++anSeq, rec = an.rec, settings = { ...an.settings };
  $("#anReport").value = "Analiza…";
  $("#anMeasures").innerHTML = '<div class="dim small">Računam Praat voice report i formante…</div>';
  $("#anTiles").classList.add("busy");
  const show = (rep, pending) => {
    if (seq !== anSeq || an.rec !== rec) return false;
    an.report = rep; an.sel = [sel[0], sel[1]];
    $("#anReport").value = rep.report;
    $("#anSelInfo").textContent = `odsječak ${num(sel[0], 2)}–${num(sel[1], 2)} s · visina ${settings.pitch_floor}–${settings.pitch_ceiling} Hz${pending ? " · CPPS se računa…" : ""}`;
    tiles(rep, pending); $("#anMeasures").innerHTML = measuresHtml(rep);
    $("#anTiles").classList.remove("busy");
    return true;
  };
  const body = (cpps) => ({ start: sel[0], end: sel[1], settings: { ...settings, cpps } });
  try {
    const wantCpps = settings.cpps !== false;
    if (!show(await api("POST", `/api/recordings/${rec.id}/analyze`, body(false)), wantCpps) || !wantCpps) return;
    show(await api("POST", `/api/recordings/${rec.id}/analyze`, body(true)), false);
  } catch (e) {
    if (seq !== anSeq) return;
    $("#anReport").value = "Greška: " + e.message; $("#anMeasures").innerHTML = ""; $("#anTiles").classList.remove("busy");
  }
}
const saveRec = (patch) => api("PUT", "/api/recordings/" + an.rec.id, patch).then(() => Object.assign(an.rec, patch)).catch((e) => toast(e.message));
$("#anTask").onchange = () => saveRec({ task: $("#anTask").value });
$("#anNotes").onchange = () => saveRec({ notes: $("#anNotes").value });
$("#anSyl").onchange = async () => { const n = Math.max(0, Math.round(+$("#anSyl").value || 0)); await saveRec({ syllables: n }); an.rec.syllables = n || null; loadSummary(); };
function tiles(r, cppsPending) {
  const cal = calibrated() ? st.settings.calibration_db : null;
  const T = [
    ["F0 prosjek", num(r.f0_mean, 1), "Hz", null, `SD ${num(r.f0_sd, 1)} Hz`],
    ["Jitter (local)", num(r.jitter_local, 2), "%", r.jitter_local == null ? null : r.jitter_local <= 1.04, "granica < 1,04 %"],
    ["Shimmer (local)", num(r.shimmer_local, 2), "%", r.shimmer_local == null ? null : r.shimmer_local <= 3.81, "granica < 3,81 %"],
    ["HNR", num(r.hnr_db, 1), "dB", r.hnr_db == null ? null : r.hnr_db >= 20, "granica > 20 dB"],
    ["CPPS", cppsPending ? "…" : num(r.cpps, 1), "dB", null, cppsPending ? "računa se…" : "viši = periodičniji glas"],
    ["Intenzitet", r.intensity_mean_db == null ? "—" : num(r.intensity_mean_db + (cal || 0), 1), cal != null ? "dB SPL" : "dB", null, cal != null ? "kalibrirano" : "relativno (nekalibrirano)"],
    ["Najduža fonacija", num(r.max_voiced_s, 1), "s", null, "MPT"],
  ];
  $("#anTiles").innerHTML = T.map(([k, v, u, ok, n]) =>
    `<div class="tile ${ok == null ? "" : ok ? "ok" : "bad"}"><div class="dim small">${k}</div><div class="big mono">${v}<small> ${u}</small></div><div class="dim small">${n}</div></div>`).join("");
}
async function loadSummary() {
  if (!an.rec) return;
  const box = $("#anSummary");
  if (!(an.rec.annotations || []).length) { box.innerHTML = '<div class="dim">Nema oznaka. Označite netečnosti u editoru: odaberite odsječak i pritisnite <kbd>A</kbd> ili „+ Oznaka“.</div>'; an.summary = null; return; }
  try {
    const s = await api("GET", `/api/recordings/${an.rec.id}/summary`); an.summary = s;
    box.innerHTML = `<div class="kv"><span class="dim">Ukupno oznaka</span><b>${s.total}</b><span class="dim">Netečnosti tipične za mucanje (SLD)</span><b>${s.sld}</b>
      <span class="dim">Oznaka u minuti</span><b>${num(s.per_minute, 1)}</b><span class="dim">% mucanih slogova (%SS)</span><b>${s.pct_ss != null ? num(s.pct_ss, 1) + " %" : "—"}</b>
      <span class="dim">Slogova</span><b>${s.syllables ?? "—"} <span class="dim small">${esc(s.syllables_source || "")}</span></b>
      <span class="dim">Prosjek 3 najduža SLD</span><b>${s.longest3_mean_s != null ? num(s.longest3_mean_s, 2) + " s" : "—"}</b></div>
      <div class="row" style="margin-top:6px">${s.by_kind.map(([k, n]) => `<span class="tag">${esc(k)}: ${n}</span>`).join("")}</div>`;
  } catch (e) { box.textContent = e.message; }
}
$("#anCopy").onclick = () => { navigator.clipboard?.writeText(reportText()).then(() => toastInfo("Kopirano"), () => toast("Kopiranje nije dopušteno")); };
function reportText() {
  const head = st.patient ? `Pacijent: ${st.patient.name}${st.patient.code ? " (" + st.patient.code + ")" : ""}\n` : "";
  let txt = (st.settings.clinic_name ? st.settings.clinic_name + "\n" : "") + head +
    `Snimka: ${recTitle(an.rec)} · ${fmtTime(an.rec.created)}${an.sel ? ` · odsječak ${num(an.sel[0], 2)}–${num(an.sel[1], 2)} s` : ""}\n` +
    (an.rec.notes ? `Opažanja: ${an.rec.notes}\n` : "") + "\n" + $("#anReport").value;
  if (an.ai) txt += `\n\n==== AI MIŠLJENJE (${an.ai.model}, ${fmtTime(an.ai.created)}) — pomoć rehabilitatoru, nije dijagnoza ====\n\n` + an.ai.text;
  return txt;
}
$("#anSave").onclick = () => {
  const who = st.patient ? st.patient.name.replace(/\W+/g, "_") : "pacijent";
  const a = document.createElement("a"); a.href = URL.createObjectURL(new Blob([reportText()], { type: "text/plain;charset=utf-8" }));
  a.download = `${who}_nalaz_${new Date(an.rec.created).toISOString().slice(0, 10)}.txt`; a.click(); setTimeout(() => URL.revokeObjectURL(a.href), 1000);
};

// ---- printable report (browser print → PDF)
$("#anPrint").onclick = () => {
  if (!an.report) return toast("Analiza još nije gotova");
  const p = st.patient && an.rec.patient_id === st.patient.id ? st.patient : null, r = an.report, S = st.settings;
  let img = ""; try { img = an.editor ? an.editor.snapshot(0.92) : ""; } catch (_) {}
  const age = p ? ageOf(p.birth, an.rec.created) : null;
  const w = window.open("", "_blank");
  if (!w) return toast("Preglednik je blokirao novi prozor — dopustite skočne prozore");
  const kv = (k, v) => (v ? `<tr><th>${esc(k)}</th><td>${esc(v)}</td></tr>` : "");
  w.document.write(`<!doctype html><html lang="hr"><head><meta charset="utf-8"><title>Nalaz — ${esc(p ? p.name : recTitle(an.rec))}</title><style>
    body{font:11pt/1.4 system-ui,Segoe UI,Roboto,sans-serif;color:#111;margin:18mm}h1{font-size:16pt;margin:0}h2{font-size:12pt;margin:16px 0 6px;border-bottom:1px solid #999}
    table{border-collapse:collapse;width:100%}th,td{padding:2px 6px;text-align:left;vertical-align:top}.pt th{width:28%;color:#444;font-weight:500}
    .m td,.m th{border-bottom:1px solid #ddd;font-size:9.5pt}.m .r{text-align:right}.m .grp td{font-weight:700;background:#f2f2f2}.m .bad td{color:#b00020}.dim{color:#666}
    img{width:100%;border:1px solid #ccc;margin-top:6px}pre{white-space:pre-wrap;font:9pt/1.35 ui-monospace,Consolas,monospace;margin:0}
    .hd{display:flex;justify-content:space-between;align-items:flex-end;border-bottom:2px solid #111;padding-bottom:6px}.sig{margin-top:40px;display:flex;justify-content:space-between}
    .sig div{border-top:1px solid #111;width:40%;padding-top:4px;text-align:center}.ai{background:#f7f7f7;padding:8px;border-left:3px solid #4da3ff}
    @media print{body{margin:0}.noprint{display:none}}</style></head><body>
    <div class="noprint" style="margin-bottom:10px"><button onclick="print()">Ispis / spremi kao PDF</button></div>
    <div class="hd"><div><h1>Akustička analiza glasa i govora</h1><div class="dim">${esc(S.clinic_name || "")}</div></div><div class="dim">Ispisano ${esc(fmtTime(Date.now()))}</div></div>
    <h2>Pacijent</h2><table class="pt">${p ? kv("Ime i prezime", p.name) + kv("Šifra / MBO", p.code) + kv("Datum rođenja", p.birth ? p.birth.split("-").reverse().join(".") + "." + (age != null ? ` (${age} god.)` : "") : "") + kv("Spol", p.sex) + kv("Dijagnoza", p.diagnosis) + kv("Razlog dolaska", p.complaint) : kv("Pacijent", "—")}</table>
    <h2>Snimka</h2><table class="pt">${kv("Zadatak", an.rec.task) + kv("Oznaka", an.rec.label) + kv("Datum snimanja", fmtTime(an.rec.created)) + kv("Izvor", an.rec.source === "out" ? "obrađeni izlaz" : "suhi mikrofon") +
      kv("Analizirani odsječak", `${num(an.sel[0], 2)}–${num(an.sel[1], 2)} s (${num(an.sel[1] - an.sel[0], 2)} s od ${num(an.rec.duration, 2)} s)`) + kv("Frekvencija uzorkovanja", an.rec.sample_rate + " Hz") +
      kv("Postavke analize", `visina ${an.settings.pitch_floor}–${an.settings.pitch_ceiling} Hz · maks. formant ${an.settings.max_formant} Hz · ${an.settings.n_formants} formanata`) +
      kv("Kalibracija", calibrated() ? `dB SPL = Praat dB ${S.calibration_db >= 0 ? "+" : "−"} ${num(Math.abs(S.calibration_db), 1)} dB` : "nije kalibrirano (intenzitet relativan)") + kv("Opažanja rehabilitatora", an.rec.notes)}</table>
    ${img ? `<h2>Oscilogram, spektrogram i konture</h2><img src="${img}">` : ""}
    <h2>Mjere</h2><div class="m">${measuresHtml(r)}</div>
    ${an.summary ? `<h2>Netečnosti</h2><table class="pt">${kv("Ukupno oznaka", String(an.summary.total)) + kv("SLD", String(an.summary.sld)) + kv("%SS", an.summary.pct_ss != null ? num(an.summary.pct_ss, 1) + " %" : "") + kv("Slogova", an.summary.syllables != null ? `${an.summary.syllables} (${an.summary.syllables_source})` : "") + kv("Prosjek 3 najduža SLD", an.summary.longest3_mean_s != null ? num(an.summary.longest3_mean_s, 2) + " s" : "") + kv("Po vrstama", an.summary.by_kind.map(([k, n]) => `${k}: ${n}`).join(", "))}</table>` : ""}
    <h2>Klinički nalaz (automatski)</h2><pre>${esc(r.report)}</pre>
    ${an.ai ? `<h2>AI mišljenje — pomoć rehabilitatoru, nije dijagnoza</h2><div class="dim">${esc(an.ai.model)} · ${esc(fmtTime(an.ai.created))}</div><div class="ai"><pre>${esc(an.ai.text)}</pre></div>` : ""}
    <div class="sig"><div>Datum</div><div>${esc(S.clinician || "Rehabilitator / logoped")}</div></div>
    <p class="dim" style="font-size:8pt;margin-top:20px">SterOidSoundBoard ${esc($("#ver").textContent)} · DigiLingua. Mjere su izračunate Praat-kompatibilnim algoritmima. Nalaz je pomoćno sredstvo i ne zamjenjuje kliničku procjenu.</p>
    <script>window.onload=()=>setTimeout(()=>print(),300)<\/script></body></html>`);
  w.document.close();
};

// ============================================================ progress (Napredak)
const PR_METRICS = [
  ["f0_mean", "F0 prosjek", "Hz", 1], ["jitter_local", "Jitter (local)", "%", 2, 1.04, "le"], ["shimmer_local", "Shimmer (local)", "%", 2, 3.81, "le"],
  ["hnr_db", "HNR", "dB", 1, 20, "ge"], ["cpps", "CPPS", "dB", 1, null, "ge"], ["max_voiced_s", "Najduža fonacija (MPT)", "s", 1, null, "ge"],
  ["f0_range_st", "Raspon F0", "st", 1], ["intensity_mean_db", "Intenzitet", "dB", 1], ["speech_rate", "Brzina govora", "slog/s", 2],
  ["articulation_rate", "Brzina artikulacije", "slog/s", 2], ["pct_ss", "% mucanih slogova (%SS)", "%", 1, null, "le"], ["sld", "SLD (broj)", "", 0, null, "le"],
];
const pr = { rows: [], loading: false };
async function loadProgress() {
  if (!st.patient) { $("#prInfo").textContent = "Odaberite pacijenta."; $("#prCharts").innerHTML = ""; $("#prTable").innerHTML = ""; return; }
  if (pr.loading) return; pr.loading = true;
  $("#prInfo").textContent = "Računam mjere za sve snimke (prvi put može potrajati)…";
  try {
    const [d] = await Promise.all([api("GET", `/api/patients/${st.patient.id}/progress`), loadSettings()]);
    pr.rows = d.rows;
    const sel = $("#prTask"), cur = sel.value || lsGet("ssb.prtask", "");
    const tasks = [...new Set(pr.rows.map((r) => r.task || "—"))];
    sel.innerHTML = ""; sel.add(new Option(`Sve snimke (${pr.rows.length})`, ""));
    tasks.forEach((t) => sel.add(new Option(`${t} (${pr.rows.filter((r) => (r.task || "—") === t).length})`, t)));
    sel.value = tasks.includes(cur) ? cur : "";
    renderProgress();
  } catch (e) { $("#prInfo").textContent = "Greška: " + e.message; }
  finally { pr.loading = false; }
}
$("#prTask").onchange = () => { lsSet("ssb.prtask", $("#prTask").value); renderProgress(); };
$("#prReload").onclick = loadProgress;
function prRows() { const t = $("#prTask").value; return pr.rows.filter((r) => !t || (r.task || "—") === t); }
function prValue(r, k) { const v = r[k]; if (v == null) return null; return k === "intensity_mean_db" && calibrated() ? v + st.settings.calibration_db : v; }
function renderProgress() {
  const rows = prRows();
  $("#prInfo").textContent = rows.length ? `${rows.length} snimaka od ${fmtDate(rows[0].created)} do ${fmtDate(rows[rows.length - 1].created)} Uspoređujte isti zadatak (npr. samo produženi vokal /a/) — odaberite ga gore.` : "Nema snimaka za prikaz.";
  const box = $("#prCharts"); box.innerHTML = "";
  PR_METRICS.forEach(([k, label, unit, dec, norm, dir]) => {
    const pts = rows.map((r) => ({ x: r.created, y: prValue(r, k), r })).filter((p) => p.y != null && isFinite(p.y));
    if (!pts.length) return;
    const first = pts[0].y, last = pts[pts.length - 1].y, dlt = last - first;
    const good = dir && pts.length > 1 && dlt !== 0 ? (dir === "le" ? dlt < 0 : dlt > 0) : null;
    const u = k === "intensity_mean_db" ? (calibrated() ? "dB SPL" : "dB (rel.)") : unit;
    const c = el("div", "prc");
    c.innerHTML = `<div class="row"><b class="grow">${label}</b><span class="mono">${num(last, dec)} ${u}</span></div>` +
      `<div class="small ${good == null ? "dim" : good ? "okc" : "badc"}">${pts.length > 1 ? `od prve: ${dlt > 0 ? "+" : ""}${num(dlt, dec)} ${u}` : "jedno mjerenje"}${norm != null ? ` · granica ${dir === "le" ? "<" : ">"} ${num(norm, dec)}` : ""}</div><canvas></canvas>`;
    box.append(c);
    drawChart(c.querySelector("canvas"), pts, norm, dir, dec);
  });
  const cols = [["created", "Datum"], ["task", "Zadatak"], ["duration", "Traj. s", 1], ["f0_mean", "F0 Hz", 1], ["jitter_local", "Jitter %", 2], ["shimmer_local", "Shimmer %", 2], ["hnr_db", "HNR dB", 1],
    ["cpps", "CPPS dB", 1], ["max_voiced_s", "MPT s", 1], ["intensity_mean_db", "Int. dB", 1], ["speech_rate", "Slog/s", 2], ["pct_ss", "%SS", 1], ["disfluencies", "Ozn.", 0]];
  $("#prTable").innerHTML = rows.length ? `<table><thead><tr>${cols.map((c) => `<th>${c[1]}</th>`).join("")}</tr></thead><tbody>${rows.slice().reverse().map((r) =>
    `<tr data-id="${r.id}" class="click">${cols.map(([k, , d]) => `<td class="${d != null ? "mono r" : ""}">${k === "created" ? fmtTime(r.created) : d != null ? (prValue(r, k) == null ? "—" : num(prValue(r, k), d)) : esc(r[k] || "—")}</td>`).join("")}</tr>`).join("")}</tbody></table>` : "";
  $("#prTable").querySelectorAll("tr[data-id]").forEach((tr) => (tr.onclick = () => { const r = st.recs.find((x) => x.id === tr.dataset.id); if (r) openAnalysis(r); }));
}
function drawChart(cv, pts, norm, dir, dec) {
  const d = window.devicePixelRatio || 1, W = (cv.clientWidth || 260) * d, H = 110 * d;
  cv.width = W; cv.height = H;
  const g = cv.getContext("2d"), P = { x: 38 * d, y: 6 * d, w: W - 46 * d, h: H - 24 * d };
  let lo = Math.min(...pts.map((p) => p.y)), hi = Math.max(...pts.map((p) => p.y));
  if (norm != null) { lo = Math.min(lo, norm); hi = Math.max(hi, norm); }
  if (hi - lo < 1e-9) { lo -= 1; hi += 1; } const pad = (hi - lo) * 0.12; lo -= pad; hi += pad;
  const x0 = pts[0].x, x1 = pts[pts.length - 1].x;
  const X = (x, i) => P.x + (x1 > x0 ? (x - x0) / (x1 - x0) : pts.length > 1 ? i / (pts.length - 1) : 0.5) * P.w;
  const Y = (y) => P.y + (1 - (y - lo) / (hi - lo)) * P.h;
  g.font = `${9 * d}px system-ui`; g.fillStyle = "#8b929c"; g.textAlign = "right"; g.textBaseline = "middle";
  for (const t of Sono.ticks(lo, hi, 3)) { g.fillStyle = "#222830"; g.fillRect(P.x, Y(t), P.w, 1); g.fillStyle = "#8b929c"; g.fillText(num(t, Math.max(0, dec - 1)), P.x - 4 * d, Y(t)); }
  if (norm != null) {
    g.fillStyle = "#3fbf6f18"; const yn = Y(norm);
    if (dir === "le") g.fillRect(P.x, yn, P.w, P.y + P.h - yn); else g.fillRect(P.x, P.y, P.w, yn - P.y);
    g.strokeStyle = "#3fbf6f88"; g.setLineDash([4 * d, 3 * d]); g.beginPath(); g.moveTo(P.x, yn); g.lineTo(P.x + P.w, yn); g.stroke(); g.setLineDash([]);
  }
  g.strokeStyle = "#4da3ff"; g.lineWidth = 1.5 * d; g.beginPath();
  pts.forEach((p, i) => (i ? g.lineTo(X(p.x, i), Y(p.y)) : g.moveTo(X(p.x, i), Y(p.y)))); g.stroke();
  pts.forEach((p, i) => { const bad = norm != null && (dir === "le" ? p.y > norm : p.y < norm); g.fillStyle = bad ? "#e5484d" : "#4da3ff"; g.beginPath(); g.arc(X(p.x, i), Y(p.y), 3 * d, 0, 7); g.fill(); });
  g.fillStyle = "#8b929c"; g.textBaseline = "top";
  g.textAlign = "left"; g.fillText(fmtDate(x0), P.x, P.y + P.h + 5 * d);
  if (pts.length > 1) { g.textAlign = "right"; g.fillText(fmtDate(x1), P.x + P.w, P.y + P.h + 5 * d); }
}
$("#prCsv").onclick = () => {
  const rows = prRows(); if (!rows.length) return;
  const keys = ["created", "task", "label", "duration", "f0_mean", "f0_sd", "f0_range_st", "jitter_local", "shimmer_local", "hnr_db", "cpps", "intensity_mean_db", "max_voiced_s", "speech_rate", "articulation_rate", "pauses", "f1", "f2", "disfluencies", "sld", "pct_ss"];
  const cell = (r, k) => { if (k === "created") return new Date(r.created).toISOString().replace("T", " ").slice(0, 16); const v = k === "intensity_mean_db" ? prValue(r, k) : r[k]; if (v == null) return ""; if (typeof v === "number") return String(+v.toFixed(4)).replace(".", ","); return '"' + String(v).replace(/"/g, '""') + '"'; };
  const csv = "﻿" + keys.join(";") + "\n" + rows.map((r) => keys.map((k) => cell(r, k)).join(";")).join("\n");
  const a = document.createElement("a"); a.href = URL.createObjectURL(new Blob([csv], { type: "text/csv;charset=utf-8" }));
  a.download = `${st.patient.name.replace(/\W+/g, "_")}_napredak.csv`; a.click(); setTimeout(() => URL.revokeObjectURL(a.href), 1000);
};

// ============================================================ clinic settings dialog
async function openSettings() {
  await loadSettings();
  const f = $("#cForm"), S = st.settings, A = S.analysis || {};
  f.clinic_name.value = S.clinic_name || ""; f.clinician.value = S.clinician || "";
  f.pitch_floor.value = A.pitch_floor ?? 75; f.pitch_ceiling.value = A.pitch_ceiling ?? 600; f.max_formant.value = A.max_formant ?? 5500; f.n_formants.value = A.n_formants ?? 5; f.cpps.checked = A.cpps !== false;
  f.cal_on.checked = S.calibration_db != null; f.calibration_db.value = S.calibration_db ?? ""; f.cal_known.value = "";
  $("#cCalcInfo").textContent = "";
  const recs = await api("GET", "/api/recordings").catch(() => []);
  const sel = f.cal_rec; sel.innerHTML = "";
  sel.add(new Option(recs.length ? "— odaberite snimku —" : "— nema snimaka —", ""));
  recs.slice(0, 50).forEach((r) => { const p = st.patients.find((x) => x.id === r.patient_id); sel.add(new Option(`${fmtTime(r.created)} · ${recTitle(r)}${p ? " · " + p.name : " · bez pacijenta"}`, r.id)); });
  const cal = recs.find((r) => r.task === "Kalibracija"); if (cal) sel.value = cal.id;
  $("#cModal").classList.remove("hidden");
}
$("#clSettingsBtn").onclick = openSettings;
$("#cClose").onclick = () => $("#cModal").classList.add("hidden");
document.querySelectorAll("#cForm [data-p]").forEach((b) => (b.onclick = () => { const [a, c, m] = b.dataset.p.split(","); const f = $("#cForm"); f.pitch_floor.value = a; f.pitch_ceiling.value = c; f.max_formant.value = m; }));
$("#cCalc").onclick = async () => {
  const f = $("#cForm"), id = f.cal_rec.value, known = parseFloat(String(f.cal_known.value).replace(",", "."));
  if (!id || !isFinite(known)) return toast("Odaberite snimku i unesite poznatu razinu u dB SPL");
  $("#cCalcInfo").textContent = "Mjerim…";
  try {
    const r = await api("POST", `/api/recordings/${id}/analyze`, {});
    if (r.intensity_mean_db == null) throw new Error("snimka je pretiha za mjerenje");
    const off = +(known - r.intensity_mean_db).toFixed(1);
    f.calibration_db.value = off; f.cal_on.checked = true;
    $("#cCalcInfo").textContent = `Izmjereno ${num(r.intensity_mean_db, 1)} dB (Praat) → pomak ${off > 0 ? "+" : ""}${num(off, 1)} dB. Spremite postavke.`;
  } catch (e) { $("#cCalcInfo").textContent = "Greška: " + e.message; }
};
$("#cForm").onsubmit = async (e) => {
  e.preventDefault(); const f = e.target;
  const cal = parseFloat(String(f.calibration_db.value).replace(",", "."));
  const body = {
    clinic_name: f.clinic_name.value.trim(), clinician: f.clinician.value.trim(),
    analysis: { pitch_floor: +f.pitch_floor.value, pitch_ceiling: +f.pitch_ceiling.value, max_formant: +f.max_formant.value, n_formants: +f.n_formants.value, cpps: f.cpps.checked },
    calibration_db: f.cal_on.checked && isFinite(cal) ? cal : null,
  };
  try { st.settings = await api("PUT", "/api/clinic/settings", body); $("#cModal").classList.add("hidden"); toastInfo("Postavke spremljene"); if (st.tab === "napredak") loadProgress(); }
  catch (er) { toast(er.message); }
};
$("#clHelpBtn").onclick = () => $("#kModal").classList.remove("hidden");
$("#kClose").onclick = () => $("#kModal").classList.add("hidden");

// ============================================================ AI opinion
let AI = null;
async function loadAiCfg() { try { AI = await api("GET", "/api/ai/config"); } catch (_) { AI = null; } return AI; }
function aiReady() {
  if (!AI) return "AI postavke nisu učitane.";
  if (!AI.model) return "AI model nije postavljen — otvorite ⚙ AI postavke.";
  if (AI.provider === "anthropic" && !AI.has_key) return "Nedostaje Anthropic API ključ — otvorite ⚙ AI postavke.";
  return null;
}
async function aiStatus() {
  await loadAiCfg();
  const names = { anthropic: "Claude", openai: "OpenAI-kompatibilan", ollama: "Ollama (lokalno)" };
  $("#anAiWho").textContent = AI ? `${names[AI.provider] || AI.provider} · ${AI.model || "—"}` : "";
  $("#anAiImg").checked = !!AI?.send_image;
  const warn = [];
  const cfgErr = aiReady(); if (cfgErr) warn.push(cfgErr);
  const p = an.rec && st.patients.find((x) => x.id === an.rec.patient_id);
  if (p && !p.ai_consent) warn.push("Pacijent nema zabilježenu suglasnost za AI analizu (Uredi pacijenta).");
  if (AI && AI.provider !== "ollama" && !cfgErr) warn.push(`Pseudonimizirani podaci šalju se vanjskom servisu (${AI.effective_url}). Za potpuno lokalnu obradu odaberite Ollama.`);
  let olErr = null;
  if (AI && AI.provider === "ollama" && !cfgErr) {
    const o = await api("GET", "/api/ai/ollama").catch(() => null);
    const m = o && o.models && o.models.find((x) => x.name === AI.model || x.name === AI.model + ":latest");
    if (!o || !o.running) olErr = o?.error || "Ollama nije dostupna.";
    else if (!m) olErr = `Model „${AI.model}” nije preuzet — ⚙ AI postavke → Preuzmi.`;
    else if (!m.vision && $("#anAiImg").checked) warn.push(`Model ${m.name} nema vid (vision): slika sonagrama se neće poslati, samo izmjereni podaci.`);
    if (olErr) warn.push(olErr);
  }
  const w = $("#anAiWarn"); w.innerHTML = warn.map(esc).join("<br>"); w.classList.toggle("hidden", !warn.length);
  $("#anAiRun").disabled = !!cfgErr || !!olErr || (p && !p.ai_consent) || !!(aiRun && aiRun.status === "running");
}
function aiBody() {
  const b = { start: an.sel[0], end: an.sel[1], question: $("#anAiQ").value.trim(), settings: an.settings };
  // local models: a smaller picture means fewer image tokens and a faster answer
  if ($("#anAiImg").checked && an.editor) { try { b.image = an.editor.snapshot(0.85, AI && AI.provider === "ollama" ? 1024 : 0); } catch (_) {} }
  return b;
}
async function ensureAnalysis() { if (!an.report || !an.sel) await runAnalysis(an.sel); if (!an.sel) throw new Error("analiza nije uspjela"); }
$("#anAiPreview").onclick = async () => {
  const pre = $("#anAiPrompt");
  if (!pre.classList.contains("hidden")) { pre.classList.add("hidden"); return; }
  try {
    await ensureAnalysis();
    const p = await api("POST", `/api/recordings/${an.rec.id}/ai/preview`, aiBody());
    pre.textContent = `Servis: ${p.provider} (${p.url}) · model: ${p.model} · slika: ${p.image ? "da" : "ne"}\n\n=== UPUTE MODELU ===\n${p.system}\n\n=== PODACI ===\n${p.user}`;
    pre.classList.remove("hidden");
  } catch (e) { toast(e.message); }
};

// ---- running AI task: live log, streamed text, stop
let aiRun = null; // {id, recId, t0, status, text, chars, nlog, stats}
const fmtSecs = (s) => (s >= 60 ? `${Math.floor(s / 60)} min ${String(Math.floor(s % 60)).padStart(2, "0")} s` : `${Math.floor(s)} s`);
function aiLogLine(ms, msg) {
  const box = $("#anAiLog"), stick = box.scrollTop + box.clientHeight >= box.scrollHeight - 4;
  box.textContent += `[+${(ms / 1000).toFixed(1).padStart(6)} s] ${msg}\n`;
  if (stick) box.scrollTop = box.scrollHeight;
}
function aiShowRunBox(on) { $("#anAiRunBox").classList.toggle("hidden", !on); }
let aiTick = 0, aiRender = 0;
function aiState() {
  const r = aiRun; if (!r) return;
  const el = (Date.now() - r.t0) / 1000, s = r.stats || {};
  let t;
  if (r.status === "running") {
    t = !r.text ? `⏳ ${fmtSecs(el)} · model učitava i čita upit…` : `✍ ${fmtSecs(el)} · ${s.tokens || 0} tokena${s.tps ? ` · ${String(s.tps).replace(".", ",")} tok/s` : ""}`;
  } else t = { done: "✓ Gotovo", error: "✗ Greška", cancelled: "■ Zaustavljeno" }[r.status] + ` nakon ${fmtSecs(r.elapsed ?? el)}` + (s.tokens ? ` · ${s.tokens} tokena` : "") + (s.tps ? ` · ${String(s.tps).replace(".", ",")} tok/s` : "");
  $("#anAiState").textContent = t;
  $("#anAiStop").classList.toggle("hidden", r.status !== "running");
}
function aiRenderText() {
  if (!aiRun || aiRender) return;
  aiRender = requestAnimationFrame(() => {
    aiRender = 0; if (!aiRun) return;
    const out = $("#anAiOut"), stick = out.getBoundingClientRect().bottom < window.innerHeight + 40;
    out.innerHTML = `<div class="dim small">${aiRun.status === "running" ? "AI piše…" : "Djelomičan odgovor (nije spremljen)"}</div><div class="md">${md(aiRun.text)}${aiRun.status === "running" ? '<span class="caret">▍</span>' : ""}</div>`;
    if (stick && aiRun.status === "running") out.lastElementChild.scrollIntoView({ block: "end" });
  });
}
// Events carry positions (log line number, text offset in characters), so a
// snapshot fetched while the task runs merges with the live events exactly.
function aiSync(task) {
  if (!aiRun || task.id !== aiRun.id) return;
  const log = task.log || [];
  if (log.length > aiRun.nlog) { log.slice(aiRun.nlog).forEach(([ms, msg]) => aiLogLine(ms, msg)); aiRun.nlog = log.length; }
  const chars = Array.from(task.text || "");
  if (chars.length > aiRun.chars) { aiRun.text += chars.slice(aiRun.chars).join(""); aiRun.chars = chars.length; aiRenderText(); }
  if (task.stats) aiRun.stats = task.stats;
}
let aiResyncT = 0;
function aiResync() {
  if (aiResyncT || !aiRun) return;
  const id = aiRun.id;
  aiResyncT = setTimeout(() => api("GET", "/api/ai/tasks/" + id).then(aiSync).catch(() => {}).finally(() => (aiResyncT = 0)), 50);
}
function aiAttach(task) {
  aiRun = { id: task.id, recId: task.recording_id, t0: Date.now() - (task.age_ms || 0), status: task.status, text: "", chars: 0, nlog: 0, stats: {} };
  $("#anAiLog").textContent = ""; aiSync(task);
  aiShowRunBox(true); aiState(); clearInterval(aiTick);
  if (task.status === "running") {
    aiTick = setInterval(aiState, 500);
    $("#anAiRun").disabled = true; $("#anAiRun").textContent = "AI radi…";
  }
}
function aiFinished(m) {
  const r = aiRun; if (!r || r.finished) return;
  r.finished = true;
  r.status = m.status; r.elapsed = (Date.now() - r.t0) / 1000;
  clearInterval(aiTick); aiState();
  $("#anAiRun").textContent = "Pokreni AI analizu";
  if (m.status === "done" && m.report) { showAi(m.report); loadAiHistory(); $("#anAiLog").classList.add("folded"); }
  else if (m.status === "error") $("#anAiOut").innerHTML = `<div class="err">${esc(m.error || "Greška")}</div>` + (r.text ? `<div class="dim small">Djelomičan odgovor:</div><div class="md">${md(r.text)}</div>` : "");
  else if (m.status === "cancelled") aiRenderText();
  aiStatus();
}
$("#anAiRun").onclick = async () => {
  const btn = $("#anAiRun"); if (btn.disabled) return;
  btn.disabled = true; btn.textContent = "Pripremam…";
  $("#anAiOut").innerHTML = ""; $("#anAiLog").classList.remove("folded");
  try {
    await ensureAnalysis();
    const { task } = await api("POST", `/api/recordings/${an.rec.id}/ai`, aiBody());
    aiAttach({ id: task, recording_id: an.rec.id, started: Date.now(), status: "running", log: [] });
    // the server may already have logged its first lines before we attached
    const full = await api("GET", "/api/ai/tasks/" + task).catch(() => null);
    if (full && aiRun && aiRun.id === task) {
      aiSync(full);
      if (full.status !== "running" && aiRun.status === "running") aiFinished({ status: full.status, report: full.report, error: full.error });
    }
  } catch (e) { $("#anAiOut").innerHTML = `<div class="err">${esc(e.message)}</div>`; btn.textContent = "Pokreni AI analizu"; aiStatus(); }
};
$("#anAiStop").onclick = () => { if (aiRun) api("POST", `/api/ai/tasks/${aiRun.id}/cancel`).catch((e) => toast(e.message)); };
$("#anAiLogToggle").onclick = () => $("#anAiLog").classList.toggle("folded");
async function aiRestore(rec) {
  // reopening a recording: follow a running AI task, or show why the last one failed
  aiRun = null; clearInterval(aiTick); aiShowRunBox(false); $("#anAiLog").textContent = "";
  const list = await api("GET", "/api/ai/tasks?recording=" + encodeURIComponent(rec.id)).catch(() => []);
  const t = list.filter((x) => x.kind === "report").pop();
  if (!t || an.rec !== rec) return;
  if (t.status === "running" || t.status === "error" || t.status === "cancelled") {
    aiAttach(t);
    if (t.status !== "running") { aiRun.elapsed = t.log.length ? t.log[t.log.length - 1][0] / 1000 : 0; aiState(); if (t.status === "error") $("#anAiOut").innerHTML = `<div class="err">Zadnji pokušaj: ${esc(t.error || "greška")}</div>`; }
  }
}
function aiHeader(m) {
  if (m.ev === "start" || m.ev === "end") api("GET", "/api/ai/tasks").then((l) => {
    const n = l.filter((t) => t.status === "running" && t.kind === "report").length;
    $("#btnAi").textContent = n ? "AI ⏳" : "AI"; $("#btnAi").classList.toggle("busy", !!n);
  }).catch(() => {});
}
HOOKS.push((m) => {
  if (m.t !== "ai_task") return;
  aiHeader(m); olTaskEvent(m);
  if (!aiRun || m.id !== aiRun.id) return;
  if (m.ev === "log") {
    if (m.n === aiRun.nlog) { aiLogLine(m.ms, m.msg); aiRun.nlog++; }
    else if (m.n > aiRun.nlog) aiResync(); // missed earlier lines: fetch them in order
  } else if (m.ev === "delta") {
    const ch = Array.from(m.text), skip = aiRun.chars - m.at; // overlap with a snapshot already shown
    if (skip < 0) aiResync();
    else if (skip < ch.length) { aiRun.text += ch.slice(skip).join(""); aiRun.chars = m.at + ch.length; aiRenderText(); aiState(); }
  }
  else if (m.ev === "stats") { aiRun.stats = m.stats; aiState(); }
  else if (m.ev === "end") aiFinished(m);
});

function md(t) {
  const inline = (x) => x.replace(/\*\*(.+?)\*\*/g, "<b>$1</b>").replace(/`([^`]+)`/g, "<code>$1</code>");
  let html = "", list = null;
  const close = () => { if (list) { html += `</${list}>`; list = null; } };
  for (const l of esc(t).split("\n")) {
    let m;
    if ((m = l.match(/^\s*#{1,6}\s+(.*)/))) { close(); html += `<h4>${inline(m[1])}</h4>`; }
    else if ((m = l.match(/^\s*[-*•]\s+(.*)/))) { if (list !== "ul") { close(); html += "<ul>"; list = "ul"; } html += `<li>${inline(m[1])}</li>`; }
    else if ((m = l.match(/^\s*\d+[.)]\s+(.*)/))) { if (list !== "ol") { close(); html += "<ol>"; list = "ol"; } html += `<li>${inline(m[1])}</li>`; }
    else if (!l.trim()) close();
    else { close(); html += `<p>${inline(l)}</p>`; }
  }
  close(); return html;
}
function showAi(rep) {
  an.ai = rep;
  const head = `<div class="dim small">${esc(rep.model)} · ${fmtTime(rep.created)} · odsječak ${num(rep.start, 2)}–${num(rep.end, 2)} s${rep.question ? " · pitanje: " + esc(rep.question) : ""}</div>`;
  $("#anAiOut").innerHTML = head + (rep.truncated ? '<div class="err small">Odgovor je skraćen (dosegnuto ograničenje duljine).</div>' : "") +
    `<div class="md">${md(rep.text)}</div><div class="row"><button class="small" id="anAiCopy">Kopiraj AI mišljenje</button><button class="small danger" id="anAiDel">Obriši</button></div>`;
  $("#anAiCopy").onclick = () => navigator.clipboard?.writeText(rep.text).then(() => toastInfo("Kopirano"), () => toast("Kopiranje nije dopušteno"));
  $("#anAiDel").onclick = async () => {
    if (!confirm("Obrisati ovo AI mišljenje?")) return;
    try { await api("DELETE", "/api/ai-reports/" + rep.id); an.ai = null; $("#anAiOut").innerHTML = ""; loadAiHistory(); } catch (e) { toast(e.message); }
  };
}
async function loadAiHistory() {
  const box = $("#anAiHist"); box.innerHTML = "";
  if (!an.rec) return;
  const list = await api("GET", `/api/recordings/${an.rec.id}/ai`).catch(() => []);
  if (!list.length) return;
  box.append(el("span", "dim", "Ranija AI mišljenja:"));
  list.forEach((r) => { const b = el("button", "small", `${fmtTime(r.created)} · ${esc(r.model)}`); b.onclick = () => showAi(r); box.append(b); });
}

// ---- AI settings panel (header "AI" button)
const AI_DEFAULT_MODEL = { anthropic: "claude-opus-5", openai: "", ollama: "gemma3:4b" };
const AI_URL_HINT = { anthropic: "https://api.anthropic.com", openai: "https://api.openai.com/v1", ollama: "http://127.0.0.1:11434" };
async function openAiPanel() {
  $("#aiPanel").classList.remove("hidden");
  await loadAiCfg(); if (!AI) return;
  $("#aiProvider").value = AI.provider; $("#aiModel").value = AI.model; $("#aiUrl").value = AI.base_url;
  $("#aiImage").checked = AI.send_image; $("#olCtx").value = String(AI.ollama_ctx || 0); aiPanelHints();
}
function aiPanelHints() {
  const pr = $("#aiProvider").value, same = AI && pr === AI.provider;
  $("#aiUrl").placeholder = AI_URL_HINT[pr];
  $("#aiKey").value = "";
  $("#aiKey").placeholder = same && AI.has_key ? "•••• spremljen (upišite za promjenu)" : pr === "openai" ? "sk-… (prazno za lokalne servere)" : "sk-ant-…";
  $("#aiKeyL").classList.toggle("hidden", pr === "ollama"); $("#aiClearKey").classList.toggle("hidden", pr === "ollama");
  $("#aiOllama").classList.toggle("hidden", pr !== "ollama");
  $("#aiInfo").innerHTML = pr === "anthropic"
    ? 'Zadani model: <b>claude-opus-5</b>. Ključ: console.anthropic.com → API Keys. Ključ se sprema samo na ovom računalu (ai.json) i nikad se ne prikazuje u pregledniku.'
    : pr === "ollama" ? "Odaberite instalirani model ili preuzmite preporučeni. Za sliku sonagrama treba model s vidom (👁)."
    : "Bilo koji servis s OpenAI /chat/completions sučeljem (OpenAI, Azure proxy, LM Studio, vLLM, OpenRouter…). Upišite model i adresu.";
  if (pr === "ollama") olLoad();
  else api("GET", "/api/ai/ollama?url=" + encodeURIComponent(AI_URL_HINT.ollama)).then((o) => {
    if (o && o.running && $("#aiProvider").value === pr) $("#aiInfo").innerHTML += ` <span class="okc">· Na ovom računalu radi Ollama ${esc(o.version)} (${o.models.length} ${o.models.length === 1 ? "model" : "modela"}) — odaberite „Ollama” za besplatnu lokalnu analizu.</span>`;
  }).catch(() => {});
}
$("#btnAi").onclick = () => ($("#aiPanel").classList.contains("hidden") ? openAiPanel() : $("#aiPanel").classList.add("hidden"));
$("#anAiCfg").onclick = () => { closeAnalysis(); openAiPanel(); window.scrollTo(0, 0); };
$("#aiProvider").onchange = () => {
  const pr = $("#aiProvider").value;
  if (!AI || pr !== AI.provider) { $("#aiModel").value = AI_DEFAULT_MODEL[pr]; $("#aiUrl").value = ""; }
  else { $("#aiModel").value = AI.model; $("#aiUrl").value = AI.base_url; }
  aiPanelHints();
};
async function saveAi(extra = {}) {
  const pr = $("#aiProvider").value;
  if (AI && pr !== AI.provider && AI.has_key && !$("#aiKey").value && pr !== "ollama" &&
      !confirm("Promjena servisa briše spremljeni API ključ prethodnog servisa. Nastaviti?")) return false;
  AI = await api("PUT", "/api/ai/config", { provider: pr, model: $("#aiModel").value, base_url: $("#aiUrl").value,
    api_key: $("#aiKey").value || null, send_image: $("#aiImage").checked, ollama_ctx: +$("#olCtx").value, ...extra });
  aiPanelHints(); return true;
}
$("#aiSave").onclick = async () => { try { if (await saveAi()) toastInfo("AI postavke spremljene"); } catch (e) { toast(e.message); } };
$("#aiClearKey").onclick = async () => { if (!confirm("Obrisati spremljeni API ključ?")) return; try { await saveAi({ clear_key: true, api_key: null }); toastInfo("Ključ obrisan"); } catch (e) { toast(e.message); } };
$("#aiTest").onclick = async () => {
  const b = $("#aiTest"); b.disabled = true; b.textContent = $("#aiProvider").value === "ollama" ? "Testiram… (učitavanje modela može potrajati)" : "Testiram…";
  try { if (!(await saveAi())) return; const r = await api("POST", "/api/ai/test"); $("#aiInfo").innerHTML = `✓ Veza radi — model <b>${esc(r.model)}</b> je odgovorio: „${esc(r.text)}“`; }
  catch (e) { $("#aiInfo").innerHTML = `<span class="err">✗ ${esc(e.message)}</span>`; }
  finally { b.disabled = false; b.textContent = "Test veze"; }
};
$("#olCtx").onchange = () => saveAi().catch((e) => toast(e.message));

// ---- Ollama: installed models, recommendations, download, delete
const OL_REC = [
  ["gemma3:4b", 3.3, true, "preporuka za GPU sa 6 GB (npr. GTX 1060 6 GB) — dobar hrvatski, čita sliku"],
  ["qwen2.5vl:3b", 3.2, true, "mali model s vidom, GPU 4–6 GB"],
  ["llama3.2:3b", 2.0, false, "najbrži, bez slike; GPU 3–4 GB ili samo CPU"],
  ["qwen2.5:7b", 4.7, false, "jači tekst, bez slike; GPU 6–8 GB"],
  ["qwen2.5vl:7b", 6.0, true, "jači model s vidom; GPU 8 GB+"],
  ["gemma3:12b", 8.1, true, "kvalitetniji; GPU 12 GB+"],
  ["llama3.2-vision", 7.8, true, "11B s vidom; GPU 12 GB+, na slabijem hardveru vrlo spor"],
];
let OL = null, olPullTask = null;
const olUrl = () => $("#aiUrl").value.trim();
async function olLoad() {
  $("#olStatus").textContent = "Provjeravam Ollamu…"; $("#olStatus").className = "ol-status";
  OL = await api("GET", "/api/ai/ollama" + (olUrl() ? "?url=" + encodeURIComponent(olUrl()) : "")).catch((e) => ({ running: false, error: e.message }));
  const s = $("#olStatus");
  if (!OL.running) {
    s.innerHTML = `✗ ${esc(OL.error || "Ollama nije dostupna")} <a href="https://ollama.com/download" target="_blank" rel="noopener">ollama.com/download</a>`; s.className = "ol-status bad";
  } else { s.textContent = `● Ollama ${OL.version} radi na ${OL.url} · ${OL.models.length} ${OL.models.length === 1 ? "model" : "modela"}`; s.className = "ol-status ok"; }
  olRender();
}
function olHas(name) { return OL && OL.models && OL.models.some((m) => m.name === name || m.name === name + ":latest"); }
function olRender() {
  const cur = $("#aiModel").value.trim(), box = $("#olModels");
  $("#aiModelList").innerHTML = (OL?.models || []).map((m) => `<option value="${esc(m.name)}">`).join("");
  box.innerHTML = "";
  if (!OL || !OL.running) box.innerHTML = '<div class="dim small">—</div>';
  else if (!OL.models.length) box.innerHTML = '<div class="dim small">Nema preuzetih modela — preuzmite jedan desno.</div>';
  (OL?.models || []).forEach((m) => {
    const sel = m.name === cur || m.name === cur + ":latest";
    const d = el("div", "ol-item" + (sel ? " sel" : ""));
    d.innerHTML = `<div class="grow"><b>${esc(m.name)}</b> ${m.vision ? '<span class="tag vis" title="Čita slike (vision)">👁 slika</span>' : ""}` +
      `<div class="small dim">${[m.params, m.quant, (m.size / 1e9).toFixed(1) + " GB", m.context ? "kontekst " + m.context : null].filter(Boolean).map(esc).join(" · ")}</div></div>`;
    const use = el("button", "small" + (sel ? " pri" : ""), sel ? "✓ u upotrebi" : "Koristi");
    use.onclick = async () => { $("#aiModel").value = m.name; try { await saveAi(); toastInfo("Model: " + m.name); olRender(); } catch (e) { toast(e.message); } };
    const del = el("button", "small", "✕"); del.title = "Obriši model s diska";
    del.onclick = async () => { if (!confirm(`Obrisati model ${m.name} iz Ollame (${(m.size / 1e9).toFixed(1)} GB)?`)) return; try { await api("POST", "/api/ai/ollama/delete", { model: m.name }); olLoad(); } catch (e) { toast(e.message); } };
    d.append(use, del); box.append(d);
  });
  const ld = OL?.loaded || [];
  $("#olLoaded").innerHTML = ld.length ? "U memoriji: " + ld.map((x) => `${esc(x.name)} — ${(x.size / 1e9).toFixed(1)} GB, ${x.gpu_pct >= 99.5 ? "100 % GPU" : x.gpu_pct <= 0.5 ? "samo CPU" : `${x.gpu_pct} % GPU / ${100 - x.gpu_pct} % CPU`}`).join("; ") : "";
  const rec = $("#olRec"); rec.innerHTML = "";
  OL_REC.forEach(([name, gbs, vis, note]) => {
    const d = el("div", "ol-item");
    d.innerHTML = `<div class="grow"><b>${esc(name)}</b> ${vis ? '<span class="tag vis">👁 slika</span>' : ""} <span class="small dim">≈ ${String(gbs).replace(".", ",")} GB</span><div class="small dim">${esc(note)}</div></div>`;
    const has = olHas(name);
    const b = el("button", "small" + (has ? "" : " pri"), has ? "✓ preuzet" : "⬇ Preuzmi");
    b.disabled = has || !OL?.running || !!olPullTask;
    b.onclick = () => olPull(name);
    d.append(b); rec.append(d);
  });
}
async function olPull(name) {
  name = (name || "").trim(); if (!name) return;
  try {
    const { task } = await api("POST", "/api/ai/ollama/pull", { model: name });
    olPullTask = task;
    $("#olPull").classList.remove("hidden"); $("#olPullTitle").textContent = "Preuzimam " + name; $("#olPullPct").textContent = "";
    $("#olPullBar").style.width = "0%"; $("#olPullLog").textContent = ""; olRender();
  } catch (e) { toast(e.message); }
}
$("#olPullBtn").onclick = () => olPull($("#olPullName").value);
$("#olPullStop").onclick = () => { if (olPullTask) api("POST", `/api/ai/tasks/${olPullTask}/cancel`).catch(() => {}); else $("#olPull").classList.add("hidden"); };
$("#olRefresh").onclick = olLoad;
$("#aiUrl").onchange = () => { if ($("#aiProvider").value === "ollama") olLoad(); };
function olTaskEvent(m) {
  if (!olPullTask || m.id !== olPullTask) return;
  if (m.ev === "log") { const l = $("#olPullLog"); l.textContent += m.msg + "\n"; l.scrollTop = l.scrollHeight; }
  else if (m.ev === "stats" && m.stats) {
    const s = m.stats; $("#olPullBar").style.width = (s.pct || 0) + "%";
    $("#olPullPct").textContent = s.total ? `${(s.completed / 1e9).toFixed(2)} / ${(s.total / 1e9).toFixed(2)} GB · ${s.pct} %` : "";
  } else if (m.ev === "end") {
    $("#olPullTitle").textContent = { done: "✓ Preuzeto", error: "✗ Greška: " + (m.error || ""), cancelled: "Preuzimanje prekinuto" }[m.status] || m.status;
    olPullTask = null; olLoad();
    if (m.status === "done") setTimeout(() => $("#olPull").classList.add("hidden"), 4000);
  }
}
// a download started in another browser window: follow it here too
api("GET", "/api/ai/tasks").then((l) => { const t = l.find((x) => x.kind === "pull" && x.status === "running"); if (t) { olPullTask = t.id; $("#olPull").classList.remove("hidden"); $("#olPullTitle").textContent = "Preuzimam " + t.label; } aiHeader({ ev: "start" }); }).catch(() => {});

// ============================================================ keyboard
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") {
    const fs = document.querySelector(".lsono.fs"); if (fs) { fs.querySelector('[data-act="fs"]').click(); return; }
    const top = [...document.querySelectorAll("body > .modal:not(.hidden)")].pop(); if (!top) return;
    if (top.id === "anModal") closeAnalysis(); else if (top.id === "pModal") closeForm(); else if (top.id) top.classList.add("hidden");
    else { const x = top.querySelector("[data-x]"); x ? x.click() : top.remove(); }
    return;
  }
  if (!st.visible || e.ctrlKey || e.metaKey || e.altKey || e.repeat) return;
  const a = document.activeElement, tag = a ? a.tagName : "";
  if (/INPUT|TEXTAREA|SELECT/.test(tag) || document.querySelector("body > .modal:not(.hidden)")) return;
  if (e.key === " " && (tag === "BUTTON" || tag === "A")) return; // the focused control handles it
  switch (e.key) {
    case " ": e.preventDefault(); toggleAudio(); break;
    case "r": case "R": resetEq(); break;
    case "d": case "D": loadDevices().then(() => toastInfo("Popis uređaja osvježen")); break;
    case "m": case "M": $("#btnMute").click(); break;
    case "1": setTab("rehab"); break;
    case "2": setTab("analiza"); break;
    case "3": setTab("napredak"); break;
    case "/": e.preventDefault(); $("#clSearch").focus(); break;
    case "?": $("#kModal").classList.remove("hidden"); break;
  }
});

// ============================================================ wiring to the shared core
buildModules(); buildEqSliders(); setEqView(lsGet("ssb.eqview", "sliders"));
HOOKS.push((m) => {
  switch (m.t) {
    case "hello": if (st.visible) { liveSub(); send({ t: "tap", src: +$("#clTap").value }); } break;
    case "board": syncAll(); break;
    case "param": case "bypass":
      if (st.currentPreset && !st.presetDirty) { st.presetDirty = true; renderPresetBadge(); }
      if (m.t === "bypass" || m.src !== ME) syncAll(); else renderChainSoon();
      break;
    case "preset": st.currentPreset = m.name || null; st.presetDirty = false; renderPresetBadge(); renderPresets(); break;
    case "clinic_presets": loadPresets(); break;
    case "clinic": if (st.visible) reloadSoon(); break;
    case "rec": showRec(m.on, 0); break;
    case "audio": AUD.running = m.running; AUD.error = m.error || null; showAudio(); break;
    case "mode":
      st.visible = m.mode === "clinic";
      if (st.visible) {
        ensureSono();
        loadSettings(); loadPresets(); loadDevices();
        loadPatients().catch((e) => toast(e.message)); syncAll();
        send({ t: "tap", src: +$("#clTap").value });
      }
      liveSub(); applyTabMute();
      break;
    case "meters":
      if (!st.visible) break;
      vu(m); showStats(m);
      if (m.live && sono) sono.push(m.live);
      if (m.rec != null) showRec(true, m.rec); else if (st.recording) showRec(false);
      break;
  }
});
setTab(["rehab", "analiza", "napredak"].includes(lsGet("ssb.cltab", "")) ? lsGet("ssb.cltab", "rehab") : "rehab");
api("GET", "/api/status").then((s) => { showRec(!!s.recording, 0); st.currentPreset = s.preset || null; renderPresetBadge(); }).catch(() => {});
})();
