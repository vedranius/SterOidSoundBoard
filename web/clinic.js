"use strict";
// DigiLingua — clinical view over the same board/engine as the pedalboard.
// Controls bind to nodes by `role` (mic, eq, daf, faf, interrupt, noise, ears).
(() => {
const ROLES = ["mic", "eq", "daf", "faf", "interrupt", "noise", "ears"];
const FREQS = [20, 25, 31.5, 40, 50, 63, 80, 100, 125, 160, 200, 250, 315, 400, 500, 630, 800, 1000, 1250, 1600, 2000, 2500, 3150, 4000, 5000, 6300, 8000, 10000, 12500, 16000, 20000];
const EQ_MIN = -48, EQ_MAX = 24;
const fk = (f) => (f >= 1000 ? f / 1000 + "k" : String(f));
// Factory EQ curves (31 values, dB)
const EQ_PRESETS = {
  "Ravno": FREQS.map(() => 0),
  "Govor": FREQS.map((f) => (f < 100 ? -18 : f < 200 ? -8 : f >= 1000 && f <= 4000 ? 6 : f > 8000 ? -6 : 0)),
  "Telefon": FREQS.map((f) => (f < 300 || f > 3400 ? -40 : 0)),
  "Bas": FREQS.map((f) => (f <= 125 ? 8 : f <= 250 ? 4 : 0)),
  "Visoki": FREQS.map((f) => (f >= 8000 ? 8 : f >= 4000 ? 4 : 0)),
};

const st = {
  patients: [], patient: null, sessions: [], active: null, recs: [], tab: "rehab",
  recording: false, mutedByTab: false, visible: false,
};
const node = (role) => BOARD.nodes.find((n) => n.role === role);
const pval = (role, id) => { const n = node(role); if (!n) return 0; const s = TYPES[n.kind].params.find((p) => p.id === id); return n.params[id] ?? s.default; };
const setP = (role, id, v) => { const n = node(role); if (n) setParam(n.id, id, v); };
const fmtTime = (ms) => new Date(ms).toLocaleString("hr-HR", { dateStyle: "short", timeStyle: "short" });
const fmtDur = (s) => { s = Math.max(0, Math.round(s)); const h = Math.floor(s / 3600), m = Math.floor(s / 60) % 60, x = s % 60; return (h ? h + ":" : "") + String(m).padStart(2, "0") + ":" + String(x).padStart(2, "0"); };
const esc = (t) => String(t ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));

// ============================================================ patients
async function loadPatients(keep) {
  st.patients = await api("GET", "/api/patients");
  const sel = $("#clPatient"); sel.innerHTML = "";
  sel.add(new Option(st.patients.length ? "— odaberite pacijenta —" : "— nema pacijenata, dodajte + —", ""));
  st.patients.forEach((p) => sel.add(new Option(p.name + (p.code ? ` (${p.code})` : ""), p.id)));
  let want = keep ?? st.patient?.id;
  if (!want) { try { want = localStorage.getItem("ssb.patient"); } catch (_) {} }
  st.patient = st.patients.find((p) => p.id === want) || null;
  sel.value = st.patient ? st.patient.id : "";
  await loadPatientData();
}
$("#clPatient").onchange = async () => {
  st.patient = st.patients.find((p) => p.id === $("#clPatient").value) || null;
  try { localStorage.setItem("ssb.patient", st.patient ? st.patient.id : ""); } catch (_) {}
  $("#clPForm").classList.add("hidden");
  await loadPatientData();
};
function openForm(p) {
  const f = $("#clPForm"); f.classList.remove("hidden"); f.dataset.id = p ? p.id : "";
  f.name.value = p?.name || ""; f.code.value = p?.code || ""; f.birth.value = p?.birth || ""; f.notes.value = p?.notes || "";
  $("#clPDel").classList.toggle("hidden", !p); f.name.focus();
}
$("#clNewP").onclick = () => openForm(null);
$("#clEditP").onclick = () => (st.patient ? openForm(st.patient) : toast("Odaberite pacijenta"));
$("#clPCancel").onclick = () => $("#clPForm").classList.add("hidden");
$("#clPForm").onsubmit = async (e) => {
  e.preventDefault(); const f = e.target;
  const body = { name: f.name.value.trim(), code: f.code.value.trim(), birth: f.birth.value, notes: f.notes.value };
  try {
    const p = f.dataset.id ? await api("PUT", "/api/patients/" + f.dataset.id, body) : await api("POST", "/api/patients", body);
    f.classList.add("hidden");
    try { localStorage.setItem("ssb.patient", p.id); } catch (_) {}
    await loadPatients(p.id);
  } catch (er) { toast(er.message); }
};
$("#clPDel").onclick = async () => {
  const p = st.patient; if (!p) return;
  if (!confirm(`Trajno obrisati pacijenta "${p.name}" sa svim sesijama i snimkama?`)) return;
  try { await api("DELETE", "/api/patients/" + p.id); $("#clPForm").classList.add("hidden"); await loadPatients(""); } catch (er) { toast(er.message); }
};

async function loadPatientData() {
  const q = st.patient ? "?patient=" + encodeURIComponent(st.patient.id) : "";
  const [sessions, allActive, recs] = await Promise.all([
    st.patient ? api("GET", "/api/sessions" + q) : Promise.resolve([]),
    api("GET", "/api/sessions"),
    st.patient ? api("GET", "/api/recordings" + q) : Promise.resolve([]),
  ]);
  st.sessions = sessions; st.recs = recs;
  st.active = allActive.find((s) => !s.end) || null;
  renderSession(); renderRecs();
}

// ============================================================ sessions
let timerH = 0;
function renderSession() {
  const a = st.active, mine = a && st.patient && a.patient_id === st.patient.id;
  const other = a && !mine ? st.patients.find((p) => p.id === a.patient_id) : null;
  $("#clSessState").textContent = mine ? "Sesija u tijeku" : other ? `Aktivna sesija: ${other.name}` : "Nema aktivne sesije";
  $("#clSessBtn").textContent = mine ? "Završi sesiju" : "Započni sesiju";
  $("#clSessBtn").classList.toggle("danger", !!mine);
  const notes = $("#clSessNotes"); notes.classList.toggle("hidden", !mine);
  if (mine && document.activeElement !== notes) notes.value = a.notes || "";
  clearInterval(timerH); $("#clTimer").textContent = "";
  if (mine) { const tick = () => ($("#clTimer").textContent = fmtDur((Date.now() - a.start) / 1000)); tick(); timerH = setInterval(tick, 1000); }
  const list = $("#clSessions"); list.innerHTML = "";
  if (!st.sessions.length) list.innerHTML = '<div class="dim small">Nema sesija.</div>';
  st.sessions.forEach((s) => {
    const d = el("div", "cl-item");
    d.innerHTML = `<div><b>${fmtTime(s.start)}</b> <span class="dim">${s.end ? fmtDur((s.end - s.start) / 1000) : "u tijeku"}</span></div>` +
      (s.notes ? `<div class="small dim clip">${esc(s.notes)}</div>` : "");
    d.title = s.notes || ""; list.append(d);
  });
}
$("#clSessBtn").onclick = async () => {
  if (!st.patient) return toast("Odaberite pacijenta");
  const a = st.active, mine = a && a.patient_id === st.patient.id;
  try {
    if (mine) await api("POST", `/api/sessions/${a.id}/stop`, { notes: $("#clSessNotes").value });
    else await api("POST", "/api/sessions", { patient_id: st.patient.id });
    await loadPatientData();
  } catch (e) { toast(e.message); }
};
$("#clSessNotes").onchange = () => { if (st.active) api("PUT", "/api/sessions/" + st.active.id, { notes: $("#clSessNotes").value }).catch((e) => toast(e.message)); };

// ============================================================ recordings list
function renderRecs() {
  const list = $("#clRecs"); list.innerHTML = "";
  if (!st.patient) { list.innerHTML = '<div class="dim small">Odaberite pacijenta.</div>'; return; }
  if (!st.recs.length) list.innerHTML = '<div class="dim small">Nema snimaka. Snimajte na tabu "Analiza glasa".</div>';
  st.recs.forEach((r) => {
    const d = el("div", "cl-item row");
    const info = el("div", "grow click");
    info.innerHTML = `<b>${esc(r.label || "Snimka")}</b><div class="small dim">${fmtTime(r.created)} · ${r.duration.toFixed(1)} s · ${r.source === "out" ? "obrađeno" : "suhi mikrofon"}</div>`;
    info.onclick = () => openAnalysis(r);
    const a = el("a", "btn", "⬇"); a.href = `/api/recordings/${r.id}/wav`; a.download = ""; a.title = "Preuzmi WAV";
    const del = el("button", null, "✕"); del.title = "Obriši";
    del.onclick = async () => { if (confirm("Obrisati snimku?")) { try { await api("DELETE", "/api/recordings/" + r.id); } catch (e) { toast(e.message); } } };
    d.append(info, a, del); list.append(d);
  });
}

// ============================================================ tabs, mute
function setTab(t) {
  st.tab = t;
  document.querySelectorAll("#clTabs button").forEach((b) => b.classList.toggle("sel", b.dataset.tab === t));
  $("#clRehab").classList.toggle("hidden", t !== "rehab");
  $("#clAnaliza").classList.toggle("hidden", t !== "analiza");
  applyTabMute();
}
function applyTabMute() {
  const want = st.visible && st.tab === "analiza" && $("#clAutoMute").checked;
  if (want && !st.mutedByTab) { st.mutedByTab = true; send({ t: "mute", on: true }); }
  else if (!want && st.mutedByTab) { st.mutedByTab = false; send({ t: "mute", on: false }); }
}
document.querySelectorAll("#clTabs button").forEach((b) => (b.onclick = () => setTab(b.dataset.tab)));
$("#clAutoMute").onchange = applyTabMute;
$("#clTap").onchange = () => send({ t: "tap", src: +$("#clTap").value });

// ============================================================ EQ
const eqSvg = $("#clEq");
const eqY = (g) => ((EQ_MAX - g) / (EQ_MAX - EQ_MIN)) * 144;
const eqX = (i) => i * 10 + 5;
function drawEq() {
  const n = node("eq");
  const gains = FREQS.map((_, i) => (n ? pval("eq", TYPES.eq31.params[i].id) : 0));
  let h = "";
  for (let g = EQ_MIN; g <= EQ_MAX; g += 12) h += `<line x1="0" x2="310" y1="${eqY(g)}" y2="${eqY(g)}" class="${g === 0 ? "zero" : "grid"}"/><text x="2" y="${eqY(g) - 1.5}" class="lbl">${g > 0 ? "+" + g : g}</text>`;
  gains.forEach((g, i) => { h += `<rect x="${eqX(i) - 3}" width="6" y="${Math.min(eqY(g), eqY(0))}" height="${Math.abs(eqY(g) - eqY(0))}" class="bar"/>`; });
  h += `<polyline points="${gains.map((g, i) => eqX(i) + "," + eqY(g)).join(" ")}" class="curve"/>`;
  gains.forEach((g, i) => { h += `<circle cx="${eqX(i)}" cy="${eqY(g)}" r="2.2" class="dot"/>`; });
  eqSvg.innerHTML = h;
  eqSvg.classList.toggle("off", !n || n.bypass);
}
function eqPointer(e) {
  if (!(e.buttons & 1) || !node("eq")) return;
  e.preventDefault();
  const r = eqSvg.getBoundingClientRect();
  const i = Math.max(0, Math.min(30, Math.floor(((e.clientX - r.left) / r.width) * 31)));
  let g = EQ_MAX - ((e.clientY - r.top) / r.height) * (EQ_MAX - EQ_MIN);
  g = Math.max(EQ_MIN, Math.min(EQ_MAX, Math.round(g * 2) / 2));
  if (Math.abs(g) < 0.75) g = 0; // snap to flat
  setP("eq", TYPES.eq31.params[i].id, g);
  $("#clEqVal").textContent = `${fk(FREQS[i])} Hz: ${g > 0 ? "+" : ""}${g} dB`;
  drawEq();
}
eqSvg.addEventListener("pointerdown", (e) => { eqSvg.setPointerCapture(e.pointerId); eqPointer(e); });
eqSvg.addEventListener("pointermove", eqPointer);
$("#clEqLabels").innerHTML = FREQS.map((f) => `<span>${fk(f)}</span>`).join("");
Object.entries(EQ_PRESETS).forEach(([name, curve]) => {
  const b = el("button", "small", name);
  b.onclick = () => { if (!node("eq")) return; curve.forEach((g, i) => setP("eq", TYPES.eq31.params[i].id, g)); drawEq(); };
  $("#clEqPresets").append(b);
});

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
  { role: "ears", title: "Slušalice — glasnoća po uhu", hint: "0–200 %.", always: true, link: true,
    ctl: [["left", "Lijevo uho", 0, 200, 1, "%"], ["right", "Desno uho", 0, 200, 1, "%"]] },
];
function buildModules() {
  const box = $("#clModules"); box.innerHTML = "";
  MODULES.forEach((m) => {
    const c = el("div", "card2 mod"); c.dataset.role = m.role;
    const h = el("div", "h row"); h.append(el("span", "grow", m.title));
    if (!m.always) {
      const t = el("button", "tog", "ISKLJ."); t.onclick = () => { const n = node(m.role); if (n) { setBypass(n.id, !n.bypass); syncModules(); } };
      h.append(t);
    }
    c.append(h, el("div", "dim small", m.hint));
    m.ctl.forEach(([id, label, a, b, step, unit, scale]) => {
      const row = el("label", "ctl");
      row.append(el("span", null, label));
      if (Array.isArray(a)) {
        const s = el("select"); a.forEach((o, i) => s.add(new Option(o, i))); s.dataset.p = id;
        s.onchange = () => setP(m.role, id, +s.value);
        row.append(s);
      } else {
        const r = el("input"); r.type = "range"; r.min = a; r.max = b; r.step = step; r.dataset.p = id; r.dataset.scale = scale || 1;
        const v = el("input", "num mono"); v.type = "number"; v.min = a; v.max = b; v.step = step;
        const apply = (x) => {
          x = Math.max(a, Math.min(b, +x)); r.value = x; v.value = x;
          setP(m.role, id, x * (scale || 1));
          if (m.link && $("#clLink")?.checked) { const other = id === "left" ? "right" : "left"; setP(m.role, other, x); syncModules(); }
        };
        r.oninput = () => apply(r.value); v.onchange = () => apply(v.value);
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
function syncAll() {
  const missing = ROLES.filter((r) => !node(r));
  $("#clMissing").classList.toggle("hidden", !missing.length);
  drawEq(); syncModules();
}
$("#clLoadChain").onclick = async () => {
  if (BOARD.nodes.length && !confirm("Zamijeniti trenutni board DigiLingua lancem?")) return;
  try { await api("POST", "/api/templates/digilingua"); } catch (e) { toast(e.message); }
};

// ============================================================ live sonagram + VU
const sono = $("#clSono"), sctx = sono.getContext("2d");
sctx.fillStyle = "#000"; sctx.fillRect(0, 0, sono.width, sono.height);
function colormap(db) {
  const v = Math.max(0, Math.min(1, (db + 100) / 80));
  return [Math.round(255 * Math.min(1, v * 1.8 - 0.6) ** 1), Math.round(255 * v ** 1.4 * 0.85), Math.round(255 * Math.min(1, v * 2.2) * (1 - v * 0.55))];
}
function sonoColumn(spec) {
  const w = sono.width, h = sono.height, px = 2;
  sctx.drawImage(sono, -px, 0);
  const img = sctx.createImageData(px, h);
  for (let y = 0; y < h; y++) {
    const [r, g, b] = colormap(spec[h - 1 - y] ?? -120); // 50 Hz per bin: bins 0..159 = 0..8 kHz
    for (let x = 0; x < px; x++) { const o = (y * px + x) * 4; img.data[o] = r; img.data[o + 1] = g; img.data[o + 2] = b; img.data[o + 3] = 255; }
  }
  sctx.putImageData(img, w - px, 0);
}
function vu(m) {
  const src = +$("#clTap").value === 1 ? m.out : m.in;
  const pk = Math.max(src[0], src[1]), db = 20 * Math.log10(pk + 1e-9);
  $("#clVu").style.width = Math.max(0, Math.min(100, ((db + 60) / 60) * 100)) + "%";
  $("#clVuDb").textContent = db > -90 ? db.toFixed(1) + " dBFS" : "";
}

// ============================================================ recording
$("#clRecBtn").onclick = async () => {
  try {
    if (st.recording) { const r = await api("POST", "/api/record/stop"); toast(`Spremljeno: ${r.duration.toFixed(1)} s`); }
    else {
      if (!st.patient && !confirm("Nije odabran pacijent — snimiti bez pacijenta?")) return;
      await api("POST", "/api/record/start", { patient_id: st.patient?.id || null, source: $("#clRecSrc").value, label: $("#clRecLabel").value.trim() });
    }
  } catch (e) { toast(e.message); }
};
function showRec(on, secs) {
  st.recording = on;
  const b = $("#clRecBtn"); b.classList.toggle("live", on);
  b.textContent = on ? `ZAVRŠI SNIMANJE  ${fmtDur(secs || 0)}` : "ZAPOČNI SNIMANJE";
}

// ============================================================ analysis modal
const an = { rec: null, buf: null, sel: [0, 0], report: null, src: null, ctx: null };
async function openAnalysis(r) {
  an.rec = r; an.report = null; an.buf = null;
  $("#anModal").classList.remove("hidden");
  $("#anTitle").textContent = `Analiza glasa — ${st.patient ? st.patient.name + " — " : ""}${r.label || "snimka"} (${fmtTime(r.created)})`;
  $("#anReport").value = "Učitavanje…"; $("#anTiles").innerHTML = "";
  try {
    an.ctx = an.ctx || new (window.AudioContext || window.webkitAudioContext)();
    const data = await (await fetch(`/api/recordings/${r.id}/wav`)).arrayBuffer();
    an.buf = await an.ctx.decodeAudioData(data);
    an.sel = [0, an.buf.duration];
    drawWave(); await runAnalysis();
  } catch (e) { $("#anReport").value = "Greška: " + e.message; }
}
function closeAnalysis() { stopPlay(); $("#anModal").classList.add("hidden"); }
$("#anClose").onclick = closeAnalysis;
document.addEventListener("keydown", (e) => { if (e.key === "Escape" && !$("#anModal").classList.contains("hidden")) closeAnalysis(); });

function channel() {
  // the louder channel (mono mic on a stereo interface)
  const b = an.buf; if (b.numberOfChannels < 2) return b.getChannelData(0);
  const e = (d) => { let s = 0; for (let i = 0; i < d.length; i += 16) s += d[i] * d[i]; return s; };
  return e(b.getChannelData(0)) >= e(b.getChannelData(1)) ? b.getChannelData(0) : b.getChannelData(1);
}
function drawWave() {
  const c = $("#anWave"), g = c.getContext("2d"), w = c.width, h = c.height, d = channel();
  g.fillStyle = "#05070a"; g.fillRect(0, 0, w, h);
  g.strokeStyle = "#2b3440"; g.beginPath(); g.moveTo(0, h / 2); g.lineTo(w, h / 2); g.stroke();
  g.fillStyle = "#4da3ff";
  const step = d.length / w;
  for (let x = 0; x < w; x++) {
    let mn = 1, mx = -1;
    for (let i = Math.floor(x * step), e = Math.floor((x + 1) * step); i < e; i++) { const v = d[i]; if (v < mn) mn = v; if (v > mx) mx = v; }
    if (mx < mn) continue;
    g.fillRect(x, (0.5 - mx / 2) * h, 1, Math.max(1, ((mx - mn) / 2) * h));
  }
  showSel();
}
function showSel() {
  const D = an.buf.duration, [a, b] = an.sel;
  const box = $("#anSelBox"); box.style.left = (a / D) * 100 + "%"; box.style.width = ((b - a) / D) * 100 + "%";
  $("#anSel").textContent = `${a.toFixed(2)}–${b.toFixed(2)} s (${(b - a).toFixed(2)} s)`;
}
(() => {
  const wb = $("#anWaveBox"); let x0 = null;
  const t = (e) => { const r = wb.getBoundingClientRect(); return Math.max(0, Math.min(1, (e.clientX - r.left) / r.width)) * an.buf.duration; };
  wb.addEventListener("pointerdown", (e) => { if (!an.buf) return; wb.setPointerCapture(e.pointerId); stopPlay(); x0 = t(e); an.sel = [x0, x0]; showSel(); });
  wb.addEventListener("pointermove", (e) => { if (x0 == null) return; const x = t(e); an.sel = [Math.min(x0, x), Math.max(x0, x)]; showSel(); });
  wb.addEventListener("pointerup", () => { if (x0 == null) return; x0 = null; if (an.sel[1] - an.sel[0] < 0.02) an.sel = [0, an.buf.duration]; showSel(); drawSpec(); });
})();
$("#anAll").onclick = () => { an.sel = [0, an.buf.duration]; showSel(); runAnalysis(); };
$("#anRun").onclick = () => runAnalysis();
$("#anPlay").onclick = () => {
  if (an.src) return stopPlay();
  const s = an.ctx.createBufferSource(); s.buffer = an.buf; s.connect(an.ctx.destination);
  s.onended = () => { an.src = null; $("#anPlay").textContent = "▶ Reproduciraj odabir"; };
  an.ctx.resume(); s.start(0, an.sel[0], an.sel[1] - an.sel[0]); an.src = s; $("#anPlay").textContent = "■ Stop";
};
function stopPlay() { if (an.src) { try { an.src.stop(); } catch (_) {} an.src = null; } $("#anPlay").textContent = "▶ Reproduciraj odabir"; }

async function runAnalysis() {
  $("#anReport").value = "Analiza…";
  try {
    an.report = await api("POST", `/api/recordings/${an.rec.id}/analyze`, { start: an.sel[0], end: an.sel[1] });
    an.report.offset = an.sel[0];
    $("#anReport").value = an.report.report;
    tiles(an.report);
  } catch (e) { $("#anReport").value = "Greška: " + e.message; }
  drawSpec();
}
function tiles(r) {
  const f = (v, d) => (v == null ? "—" : v.toFixed(d));
  const T = [
    ["F0", f(r.f0_mean, 1), "Hz", r.f0_mean == null ? null : r.f0_mean >= 70 && r.f0_mean <= 300, "70–300 Hz"],
    ["Jitter", f(r.jitter_local, 2), "%", r.jitter_local == null ? null : r.jitter_local <= 1.04, "< 1.04 %"],
    ["Shimmer", f(r.shimmer_local, 2), "%", r.shimmer_local == null ? null : r.shimmer_local <= 3.81, "< 3.81 %"],
    ["HNR", f(r.hnr_db, 1), "dB", r.hnr_db == null ? null : r.hnr_db >= 20, "> 20 dB"],
  ];
  $("#anTiles").innerHTML = T.map(([k, v, u, ok, n]) =>
    `<div class="tile ${ok == null ? "" : ok ? "ok" : "bad"}"><div class="dim small">${k}</div><div class="big mono">${v}<small> ${u}</small></div><div class="dim small">norma ${n}</div></div>`).join("");
}
// STFT sonagram of the selection, with the analysed F0 contour.
function fft(re, im) {
  const n = re.length;
  for (let i = 1, j = 0; i < n; i++) { let b = n >> 1; for (; j & b; b >>= 1) j ^= b; j ^= b; if (i < j) { [re[i], re[j]] = [re[j], re[i]]; [im[i], im[j]] = [im[j], im[i]]; } }
  for (let len = 2; len <= n; len <<= 1) {
    const a = (-2 * Math.PI) / len, wr = Math.cos(a), wi = Math.sin(a);
    for (let i = 0; i < n; i += len) {
      let cr = 1, ci = 0;
      for (let k = 0; k < len / 2; k++) {
        const p = i + k, q = p + len / 2, tr = re[q] * cr - im[q] * ci, ti = re[q] * ci + im[q] * cr;
        re[q] = re[p] - tr; im[q] = im[p] - ti; re[p] += tr; im[p] += ti;
        const t = cr * wr - ci * wi; ci = cr * wi + ci * wr; cr = t;
      }
    }
  }
}
function drawSpec() {
  if (!an.buf) return;
  const c = $("#anSpec"), g = c.getContext("2d"), w = c.width, h = c.height;
  const d = channel(), sr = an.buf.sampleRate, N = 1024;
  const a = Math.floor(an.sel[0] * sr), b = Math.max(a + N, Math.floor(an.sel[1] * sr));
  const kmax = Math.min(N / 2, Math.round((8000 / sr) * N));
  const win = new Float32Array(N).map((_, i) => 0.5 - 0.5 * Math.cos((2 * Math.PI * i) / N));
  const img = g.createImageData(w, h), re = new Float32Array(N), im = new Float32Array(N);
  const cols = [];
  let peak = -200;
  for (let x = 0; x < w; x++) {
    const s = Math.floor(a + ((b - a) * x) / w - N / 2);
    for (let i = 0; i < N; i++) { const k = s + i; re[i] = (k >= 0 && k < d.length ? d[k] : 0) * win[i]; im[i] = 0; }
    fft(re, im);
    const col = new Float32Array(kmax);
    for (let k = 0; k < kmax; k++) { col[k] = 10 * Math.log10(re[k] * re[k] + im[k] * im[k] + 1e-12); if (col[k] > peak) peak = col[k]; }
    cols.push(col);
  }
  for (let x = 0; x < w; x++) for (let y = 0; y < h; y++) {
    const k = Math.floor(((h - 1 - y) / h) * kmax), v = Math.max(0, Math.min(1, (cols[x][k] - (peak - 70)) / 70));
    const o = (y * w + x) * 4, [r, gg, bb] = colormap(v * 80 - 100);
    img.data[o] = r; img.data[o + 1] = gg; img.data[o + 2] = bb; img.data[o + 3] = 255;
  }
  g.putImageData(img, 0, 0);
  g.fillStyle = "#9aa4b1"; g.font = "12px system-ui";
  for (const f of [1000, 2000, 4000, 6000]) { const y = h - (f / 8000) * h; g.fillRect(0, y, 6, 1); g.fillText(f / 1000 + " kHz", 8, y + 4); }
  const r = an.report;
  if (r && r.pitch && Math.abs(r.offset - an.sel[0]) < 1e-6) {
    const dur = an.sel[1] - an.sel[0];
    g.fillStyle = "#ffd84d";
    r.pitch.forEach(([t, f]) => { if (f > 0) g.fillRect((t / dur) * w - 1.5, h - (f / 500) * h - 1.5, 3, 3); });
    g.fillStyle = "#ffd84d99"; for (const f of [100, 200, 300, 400]) g.fillText(f + " Hz", w - 44, h - (f / 500) * h + 4);
  }
}
$("#anCopy").onclick = () => { navigator.clipboard?.writeText($("#anReport").value).then(() => toast("Kopirano"), () => toast("Kopiranje nije dopušteno")); };
$("#anSave").onclick = () => {
  const who = st.patient ? st.patient.name.replace(/\W+/g, "_") : "pacijent";
  const head = st.patient ? `Pacijent: ${st.patient.name}${st.patient.code ? " (" + st.patient.code + ")" : ""}\n` : "";
  const txt = head + `Snimka: ${an.rec.label || an.rec.id} · ${fmtTime(an.rec.created)} · odabir ${$("#anSel").textContent}\n\n` + $("#anReport").value;
  const a = document.createElement("a"); a.href = URL.createObjectURL(new Blob([txt], { type: "text/plain;charset=utf-8" }));
  a.download = `${who}_nalaz_${new Date(an.rec.created).toISOString().slice(0, 10)}.txt`; a.click(); setTimeout(() => URL.revokeObjectURL(a.href), 1000);
};

// ============================================================ wiring to the shared core
buildModules();
HOOKS.push((m) => {
  switch (m.t) {
    case "board": case "param": case "bypass": syncAll(); break;
    case "clinic": if (st.visible) loadPatients(); break;
    case "rec": showRec(m.on, 0); break;
    case "mode":
      st.visible = m.mode === "clinic";
      if (st.visible) { loadPatients().catch((e) => toast(e.message)); syncAll(); send({ t: "tap", src: +$("#clTap").value }); }
      applyTabMute();
      break;
    case "meters":
      if (!st.visible) break;
      vu(m);
      if (m.spec) sonoColumn(m.spec);
      if (m.rec != null) showRec(true, m.rec); else if (st.recording) showRec(false);
      break;
  }
});
api("GET", "/api/status").then((s) => showRec(!!s.recording, 0)).catch(() => {});
})();
