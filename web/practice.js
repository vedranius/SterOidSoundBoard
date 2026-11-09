"use strict";
// DigiLingua for phoneticians and speech-language pathologists: patient chart
// (Karton), session workflow (protocol checklist, SOAP, homework, next
// appointment), DigiLingua FILTER extras (real-time switch, sound-card channel
// matrix, added filters and monitors), perceptual rating (GRBAS / CAPE-V),
// reading prompter and the AI-assistant (MCP) settings. Uses window.Clinic.
const Practice = (() => {
  const C = window.Clinic;
  const { st, an, AUD, esc, num, fmtTime, fmtDate, fmtDur } = C;
  const tr = (hr, en) => (I18N.lang === "en" ? en : hr);
  const debounce = (fn, ms) => { let h = 0; return (...a) => { clearTimeout(h); h = setTimeout(() => fn(...a), ms); }; };
  const KIND = { procjena: "Procjena", terapija: "Terapija", kontrola: "Kontrola" };
  let PROTO = { tasks: C.TASKS, protocols: [] };
  api("GET", "/api/protocols").then((p) => (PROTO = p)).catch(() => {});

  // ================================================================ session workflow (sidebar)
  let active = null; // full detail of the running session of the selected patient
  const mineActive = () => st.active && st.patient && st.active.patient_id === st.patient.id;
  function startOpts() {
    const k = document.querySelector("#psKind"), p = document.querySelector("#psProto");
    return { kind: k ? k.value : "", protocol: p && p.value ? p.value : null };
  }
  async function loadActive() {
    if (!mineActive()) { active = null; return; }
    if (active && active.id === st.active.id) return;
    try { active = await api("GET", `/api/sessions/${st.active.id}/detail`); } catch (_) { active = null; }
  }
  const saveSess = debounce((patch) => { if (active) api("PUT", "/api/sessions/" + active.id, patch).catch((e) => toast(e.message)); }, 700);
  /** Task done: ticked, or recorded in this session (k-th task of a name ↔ k recordings of it). */
  function taskDone(tasks, i) {
    const t = tasks[i]; if (t.done) return true;
    const k = tasks.slice(0, i + 1).filter((x) => x.task === t.task).length;
    return st.recs.filter((r) => r.session_id === active.id && r.task === t.task).length >= k;
  }
  async function session() {
    const startBox = $("#clSessStart"), box = $("#clSessExtra");
    if (!startBox || !box) return;
    await loadActive();
    if (!st.patient || mineActive()) startBox.innerHTML = "";
    else {
      startBox.innerHTML = `<div class="pgrid2 small">
        <label>${tr("Vrsta sesije", "Session kind")}<select id="psKind"><option value="terapija">${tr("Terapija", "Therapy")}</option><option value="procjena">${tr("Procjena", "Assessment")}</option><option value="kontrola">${tr("Kontrola", "Follow-up")}</option></select></label>
        <label>${tr("Protokol", "Protocol")}<select id="psProto"><option value="">${tr("— bez protokola —", "— no protocol —")}</option>${PROTO.protocols.map((p) => `<option value="${p.id}">${esc(p.name)}</option>`).join("")}</select></label></div>
        <div id="psProtoInfo" class="dim small"></div>`;
      const pi = () => { const p = PROTO.protocols.find((x) => x.id === $("#psProto").value); $("#psProtoInfo").textContent = p ? p.description : ""; if (p) $("#psKind").value = p.kind; };
      $("#psProto").onchange = pi;
    }
    if (!active) { box.innerHTML = ""; return; }
    const s = active, tasks = s.tasks || [];
    box.innerHTML = `
      <div class="ps-kind row small"><span class="tag acc">${esc(tr(KIND[s.kind] || "Sesija", { procjena: "Assessment", terapija: "Therapy", kontrola: "Follow-up" }[s.kind] || "Session"))}</span><span class="grow"></span>
        <select id="psKindNow" class="small"><option value="terapija">${tr("Terapija", "Therapy")}</option><option value="procjena">${tr("Procjena", "Assessment")}</option><option value="kontrola">${tr("Kontrola", "Follow-up")}</option></select></div>
      <label class="side-label">${tr("Zadaci sesije", "Session tasks")} <span class="dim">${tasks.filter((_, i) => taskDone(tasks, i)).length}/${tasks.length}</span></label>
      <div class="ps-tasks">${tasks.map((t, i) => `<div class="ps-task${taskDone(tasks, i) ? " done" : ""}" data-i="${i}"><input type="checkbox" ${taskDone(tasks, i) ? "checked" : ""}><span class="grow">${esc(t.task)}</span><button class="small" data-rec title="${tr("Snimi ovaj zadatak", "Record this task")}"><span data-ic="mic"></span></button><button class="small" data-del title="${tr("Ukloni", "Remove")}">✕</button></div>`).join("")}</div>
      <div class="row small ps-add"><select id="psAddTask"><option value="">${tr("+ dodaj zadatak…", "+ add a task…")}</option>${PROTO.tasks.map((x) => `<option>${esc(x)}</option>`).join("")}</select></div>
      <label class="side-label">SOAP</label>
      <div class="seg small-seg ps-soapt">${["subjective", "objective", "assessment", "plan"].map((k, i) => `<button type="button" data-k="${k}" class="${i ? "" : "sel"}">${{ subjective: "S", objective: "O", assessment: "A", plan: "P" }[k]}</button>`).join("")}</div>
      <textarea id="psSoap" rows="4"></textarea>
      <div class="dim small" id="psSoapHint"></div>
      <label class="side-label">${tr("Domaća zadaća", "Homework")}</label>
      <textarea id="psHome" rows="2" placeholder="${tr("vježbe za kod kuće…", "exercises for home…")}">${esc(s.homework || "")}</textarea>
      <label class="side-label">${tr("Sljedeći termin", "Next appointment")}</label>
      <input id="psNext" type="datetime-local" value="${esc(s.next_date && s.next_date.length === 10 ? s.next_date + "T09:00" : s.next_date || "")}">`;
    Icons.apply(box);
    $("#psKindNow").value = s.kind || "terapija";
    $("#psKindNow").onchange = () => { s.kind = $("#psKindNow").value; saveSess({ kind: s.kind }); session(); };
    box.querySelectorAll(".ps-task").forEach((row) => {
      const i = +row.dataset.i;
      row.querySelector("input").onchange = (e) => { tasks[i].done = e.target.checked; saveSess({ tasks }); row.classList.toggle("done", e.target.checked); };
      row.querySelector("[data-rec]").onclick = () => recordTask(tasks[i].task);
      row.querySelector("[data-del]").onclick = () => { tasks.splice(i, 1); saveSess({ tasks }); session(); };
    });
    $("#psAddTask").onchange = (e) => { if (!e.target.value) return; tasks.push({ id: "t" + Date.now().toString(36), task: e.target.value, done: false, note: "" }); saveSess({ tasks }); session(); };
    const HINT = { subjective: tr("S — što klijent navodi: tegobe, napredak, domaće vježbe.", "S — what the client reports: complaints, progress, homework."),
      objective: tr("O — nalaz: mjerenja, perceptivna ocjena, opažanja u sesiji.", "O — findings: measurements, perceptual rating, observations."),
      assessment: tr("A — procjena: tumačenje, promjena u odnosu na prije.", "A — assessment: interpretation, change since before."),
      plan: tr("P — plan: ciljevi i postupci za sljedeće sesije.", "P — plan: goals and steps for the next sessions.") };
    let cur = "subjective";
    const soap = Object.assign({ subjective: "", objective: "", assessment: "", plan: "" }, s.soap || {});
    const showSoap = () => { $("#psSoap").value = soap[cur]; $("#psSoapHint").textContent = HINT[cur]; box.querySelectorAll(".ps-soapt button").forEach((b) => b.classList.toggle("sel", b.dataset.k === cur)); };
    box.querySelectorAll(".ps-soapt button").forEach((b) => (b.onclick = () => { cur = b.dataset.k; showSoap(); }));
    $("#psSoap").oninput = () => { soap[cur] = $("#psSoap").value; s.soap = soap; saveSess({ soap }); };
    $("#psHome").oninput = () => { s.homework = $("#psHome").value; saveSess({ homework: s.homework }); };
    $("#psNext").onchange = () => { s.next_date = $("#psNext").value; saveSess({ next_date: s.next_date }); };
    showSoap();
  }
  function recordTask(task) {
    C.fillTasks($("#clRecTask"), task);
    C.setTab("analiza");
    prompter();
    $("#clRecBtn").scrollIntoView({ behavior: "smooth", block: "center" });
    $("#clRecBtn").focus();
  }

  // ================================================================ reading prompter + task instructions
  const PASSAGE_HR = "Sjeverac i sunce prepirali su se o tome tko je od njih jači. Upravo tada naiđe putnik umotan u topao kaput. Oni se dogovore da se jačim smatra onaj tko prvi uspije natjerati putnika da skine kaput. Sjeverac zapuše svom snagom, ali što je jače puhao, to se putnik čvršće umatao u svoj kaput. Na kraju sjeverac odustane od svog pokušaja. Zatim sunce zasja toplo i putnik smjesta skine kaput. I tako je sjeverac morao priznati da je sunce jače od njega.";
  const PASSAGE_EN = "The North Wind and the Sun were disputing which was the stronger, when a traveler came along wrapped in a warm cloak. They agreed that the one who first succeeded in making the traveler take his cloak off should be considered stronger than the other. Then the North Wind blew as hard as he could, but the more he blew the more closely did the traveler fold his cloak around him; and at last the North Wind gave up the attempt. Then the Sun shined out warmly, and immediately the traveler took off his cloak. And so the North Wind was obliged to confess that the Sun was the stronger of the two.";
  const SENTENCES_HR = ["Mama mi je kupila novu majicu.", "Pas trči po zelenoj livadi.", "Sutra idemo autobusom na more.", "Baka peče kolače od jabuka.", "Djeca se igraju u parku iza škole."];
  function prompterText(task) {
    const en = I18N.lang === "en";
    switch (task) {
      case "Čitanje standardnog teksta": return { big: true, text: en ? PASSAGE_EN : PASSAGE_HR, note: en ? "Read aloud at a comfortable pitch and loudness." : "Čitajte naglas ugodnom visinom i glasnoćom." };
      case "Brojanje / automatizirani govor": return { big: true, text: Array.from({ length: 20 }, (_, i) => i + 1).join("  "), note: en ? "Count from 1 to 20, then the days of the week." : "Brojite od 1 do 20, zatim dane u tjednu." };
      case "Ponavljanje rečenica": return { big: true, text: SENTENCES_HR.join("\n"), note: en ? "The clinician says each sentence, the client repeats it." : "Rehabilitator izgovori rečenicu, klijent je ponovi." };
      case "Dijadohokineza (pa-ta-ka)": return { big: true, text: "pa-pa-pa…   ta-ta-ta…   ka-ka-ka…   pa-ta-ka pa-ta-ka…", note: en ? "As fast and as clearly as possible, 5 s each." : "Što brže i što jasnije, po 5 s." };
      case "Produženi vokal /a/": case "Produženi vokal /i/": case "Produženi vokal /u/":
        return { text: en ? "Take a deep breath and sustain the vowel as long as possible at a comfortable pitch and loudness (3 trials; the longest gives MPT)." : "Duboko udahnite i izgovarajte vokal što dulje možete, ugodnom visinom i glasnoćom (3 pokušaja; najdulji daje MPT)." };
      case "Produženi /s/": case "Produženi /z/":
        return { text: en ? "Sustain the sound as long as possible; start and stop the recording with the sound (s/z ratio)." : "Izgovarajte glas što dulje možete; snimanje započnite i završite uz glas (omjer s/z)." };
      case "Najtiša fonacija /a/": return { text: en ? "Sustain /a/ as softly as possible without whispering (I-low for the DSI; needs a calibrated microphone, same distance)." : "Izgovarajte /a/ što tiše možete, bez šaptanja (I-low za DSI; treba kalibriran mikrofon, ista udaljenost)." };
      case "Najglasnija fonacija /a/": return { text: en ? "Sustain /a/ as loudly as possible without straining." : "Izgovarajte /a/ što glasnije možete, bez naprezanja." };
      case "Glasovni raspon / glisando": return { text: en ? "Glide on /a/ from your lowest to your highest note and back (F0 range, F0-high for the DSI)." : "Klizite na /a/ od najdubljeg do najvišeg tona i natrag (F0 raspon, F0-high za DSI)." };
      case "Spontani govor": return { text: en ? "Talk freely for 1–2 minutes (e.g. about your day, work, hobby) — at least 300 syllables for fluency." : "Slobodno govorite 1–2 minute (npr. o danu, poslu, hobiju) — za tečnost barem 300 slogova." };
      case "Opis slike": return { text: en ? "Describe a picture in as much detail as you can." : "Opišite sliku što detaljnije." };
    }
    return null;
  }
  function prompter() {
    const box = $("#clPrompter"); if (!box) return;
    const p = prompterText($("#clRecTask").value);
    if (!p) { box.classList.add("hidden"); box.innerHTML = ""; return; }
    box.classList.remove("hidden");
    box.innerHTML = `<div class="row small"><span data-ic="file"></span><b class="grow">${tr("Upute za zadatak", "Task instructions")}</b>${p.big ? `<button class="small" data-fs><span data-ic="maximize"></span>${tr("Puni zaslon", "Full screen")}</button>` : ""}</div>
      ${p.note ? `<div class="dim small">${esc(p.note)}</div>` : ""}<div class="${p.big ? "pr-text" : "pr-note"}" data-noi18n>${esc(p.text)}</div>`;
    Icons.apply(box);
    const fs = box.querySelector("[data-fs]");
    if (fs) fs.onclick = () => { box.classList.toggle("fs"); };
  }
  $("#clRecTask").addEventListener("change", prompter);
  document.addEventListener("keydown", (e) => { if (e.key === "Escape") $("#clPrompter")?.classList.remove("fs"); });

  // ================================================================ perceptual rating (Analiza → Perceptivna ocjena)
  const GRBAS = [["G", "Grade — ukupni stupanj", "Grade — overall"], ["R", "Roughness — hrapavost", "Roughness"], ["B", "Breathiness — šumnost/zadah", "Breathiness"], ["A", "Asthenia — slabost", "Asthenia"], ["S", "Strain — napetost", "Strain"]];
  const CAPEV = [["Ukupna težina", "Overall severity"], ["Hrapavost", "Roughness"], ["Šumnost (zadah)", "Breathiness"], ["Napetost", "Strain"], ["Visina", "Pitch"], ["Glasnoća", "Loudness"]];
  function rating() {
    const box = $("#anRating"); if (!box || !an.rec) return;
    const r = an.rec, x = r.rating || {}, g = x.grbas || [null, null, null, null, null], c = x.capev || [null, null, null, null, null, null];
    box.innerHTML = `<div class="card2"><div class="h">${tr("GRBAS (Hirano) — 0 normalno, 1 blago, 2 umjereno, 3 teško", "GRBAS (Hirano) — 0 normal, 1 mild, 2 moderate, 3 severe")}</div>
      <div class="grbas">${GRBAS.map(([k, hr, en], i) => `<label><b data-noi18n>${k}</b><span class="small dim">${esc(tr(hr, en))}</span><span class="seg small-seg">${[0, 1, 2, 3].map((v) => `<button type="button" data-g="${i}" data-v="${v}" class="${g[i] === v ? "sel" : ""}">${v}</button>`).join("")}</span></label>`).join("")}</div>
      <div class="h" style="margin-top:12px">${tr("CAPE-V — vizualno-analogna ljestvica 0–100", "CAPE-V — visual analogue scale 0–100")}</div>
      <div class="capev">${CAPEV.map(([hr, en], i) => `<div class="capev-row"><span>${esc(tr(hr, en))}</span><input type="range" min="0" max="100" step="1" data-c="${i}" value="${c[i] ?? 0}" class="${c[i] == null ? "unset" : ""}"><b class="mono" data-cv="${i}">${c[i] == null ? "—" : Math.round(c[i])}</b><button type="button" class="small" data-clr="${i}" title="${tr("Bez ocjene", "Not rated")}">✕</button></div>`).join("")}</div>
      <div class="pgrid2" style="margin-top:10px"><label>${tr("Ocjenjivač", "Rater")}<input data-k="rater" value="${esc(x.rater || st.settings.clinician || "")}"></label><label>${tr("Napomena", "Note")}<input data-k="note" value="${esc(x.note || "")}"></label></div>
      <div class="row" style="margin-top:10px"><button class="pri" data-save>${tr("Spremi ocjenu", "Save rating")}</button><button data-clear>${tr("Obriši ocjenu", "Delete rating")}</button><span class="grow"></span><span class="dim small">${x.updated ? tr("spremljeno ", "saved ") + fmtTime(x.updated) : tr("nije ocijenjeno", "not rated")}</span></div></div>`;
    box.querySelectorAll("[data-g]").forEach((b) => (b.onclick = () => { g[+b.dataset.g] = +b.dataset.v; box.querySelectorAll(`[data-g="${b.dataset.g}"]`).forEach((y) => y.classList.toggle("sel", y === b)); }));
    box.querySelectorAll("[data-c]").forEach((inp) => (inp.oninput = () => { const i = +inp.dataset.c; c[i] = +inp.value; inp.classList.remove("unset"); box.querySelector(`[data-cv="${i}"]`).textContent = inp.value; }));
    box.querySelectorAll("[data-clr]").forEach((b) => (b.onclick = () => { const i = +b.dataset.clr; c[i] = null; const inp = box.querySelector(`[data-c="${i}"]`); inp.classList.add("unset"); box.querySelector(`[data-cv="${i}"]`).textContent = "—"; }));
    box.querySelector("[data-save]").onclick = async () => {
      const rt = { grbas: g.every((v) => v == null) ? null : g.map((v) => v ?? 0), capev: c.every((v) => v == null) ? null : c, rater: box.querySelector('[data-k="rater"]').value, note: box.querySelector('[data-k="note"]').value };
      try { await api("PUT", "/api/recordings/" + r.id, { rating: rt }); r.rating = { ...rt, updated: Date.now() }; C.toastInfo(tr("Ocjena spremljena", "Rating saved")); rating(); } catch (e) { toast(e.message); }
    };
    box.querySelector("[data-clear]").onclick = async () => { try { await api("PUT", "/api/recordings/" + r.id, { rating: null }); r.rating = null; rating(); } catch (e) { toast(e.message); } };
  }
  const grbasText = (rt) => (rt && rt.grbas ? rt.grbas.map((v, i) => "GRBAS"[i] + v).join(" ") : "");

  // ================================================================ Karton (patient chart)
  const Q_KINDS = [["VHI-10", 40], ["VHI-30", 120], ["V-RQOL", 100], ["S-24", 24], ["OASES", 5], ["Drugo", null]];
  async function karton() {
    const box = $("#clKarton"); if (!box) return;
    if (!st.patient) { box.innerHTML = `<div class="card2 dim">${tr("Odaberite klijenta (lijevo) ili dodajte novog — karton prikazuje anamnezu, glasovni profil, sesije, upitnike, bilješke i AI mišljenja.", "Select a client (left) or add a new one — the chart shows the history, voice profile, sessions, questionnaires, notes and AI opinions.")}</div>`; return; }
    const pid = st.patient.id;
    box.innerHTML = `<div class="card2 dim">${tr("Učitavam karton…", "Loading the chart…")}</div>`;
    let ch, prof = null;
    try { ch = await api("GET", `/api/patients/${pid}/chart`); } catch (e) { box.innerHTML = `<div class="card2 err">${esc(e.message)}</div>`; return; }
    if (!st.patient || st.patient.id !== pid) return;
    const p = ch.patient, nextDate = ch.sessions.map((s) => s.next_date).filter(Boolean).sort().pop();
    const A = [["Razlog dolaska / tegobe", "Complaint", p.complaint], ["Povijest problema", "History", p.history], ["Medicinska anamneza", "Medical history", p.medical], ["Lijekovi", "Medications", p.medications],
      ["Profesija i glasovno opterećenje", "Occupation and vocal load", p.voice_use], ["Pušenje", "Smoking", p.smoking], ["Sluh", "Hearing", p.hearing], ["Materinji jezik", "Native language", p.language],
      ["Ciljevi terapije", "Therapy goals", p.goals], ["Napomene", "Notes", p.notes]].filter((x) => x[2]);
    box.innerHTML = `
      <div class="card2 kt-head"><div class="avatar big">${esc((p.name || "?").split(/\s+/).slice(0, 2).map((w) => w[0]).join("").toUpperCase())}</div>
        <div class="grow"><h2>${esc(p.name)}${p.code ? ` <span class="dim">(${esc(p.code)})</span>` : ""}</h2>
          <div class="dim">${[ch.age != null ? C.yrs(ch.age) : null, p.sex || null, p.birth ? tr("rođ. ", "born ") + p.birth.split("-").reverse().join(".") + "." : null].filter(Boolean).map(esc).join(" · ")}</div>
          <div><b>${esc(p.diagnosis || tr("dijagnoza nije upisana", "no diagnosis entered"))}</b></div>
          <div class="row small">${p.ai_consent ? `<span class="tag okc">✓ ${tr("suglasnost za AI", "AI consent")}</span>` : `<span class="tag">✗ ${tr("nema suglasnosti za AI", "no AI consent")}</span>`}
            ${nextDate ? `<span class="tag acc"><span data-ic="calendar"></span>${tr("sljedeći termin", "next appointment")} ${esc(fmtNext(nextDate))}</span>` : ""}
            <span class="tag">${ch.sessions.length} ${tr("sesija", "sessions")}</span><span class="tag">${ch.recordings.length} ${tr("snimaka", "recordings")}</span></div></div>
        <div class="kt-acts"><button data-a="edit"><span data-ic="edit"></span>${tr("Uredi podatke", "Edit details")}</button><button data-a="rec" class="pri"><span data-ic="mic"></span>${tr("Snimi / analiziraj", "Record / analyse")}</button><button data-a="print"><span data-ic="printer"></span>${tr("Ispis kartona", "Print chart")}</button></div></div>
      <div class="kt-grid">
        <div class="card2"><div class="h"><span data-ic="file"></span>${tr("Anamneza", "History")}</div>${A.length ? `<dl class="kt-dl">${A.map(([hr, en, v]) => `<dt>${esc(tr(hr, en))}</dt><dd class="pre">${esc(v)}</dd>`).join("")}</dl>` : `<div class="dim small">${tr("Anamneza nije upisana — „Uredi podatke”.", "No history entered — “Edit details”.")}</div>`}</div>
        <div class="card2" id="ktProfile"><div class="h"><span data-ic="gauge"></span>${tr("Glasovni profil", "Voice profile")}</div><div class="dim small">${tr("Računam iz snimaka zadataka…", "Computing from the task recordings…")}</div></div>
        <div class="card2"><div class="h"><span data-ic="calendar"></span>${tr("Sesije", "Sessions")}</div><div class="kt-sess">${ch.sessions.length ? ch.sessions.map((s) => `
          <div class="kt-s click" data-s="${s.id}"><div class="row"><b class="grow">${fmtDate(s.start)}</b>${s.kind ? `<span class="tag acc">${esc(tr(KIND[s.kind] || s.kind, { procjena: "Assessment", terapija: "Therapy", kontrola: "Follow-up" }[s.kind] || s.kind))}</span>` : ""}<span class="dim small">${s.end ? fmtDur((s.end - s.start) / 1000) : tr("u tijeku", "in progress")}</span></div>
          ${s.tasks.length ? `<div class="small dim">${tr("zadaci", "tasks")} ${s.tasks.filter((t) => t.done).length}/${s.tasks.length} · ${s.recordings.length} ${tr("snimaka", "recordings")}</div>` : ""}
          ${["subjective", "assessment", "plan"].filter((k) => s.soap && s.soap[k]).map((k) => `<div class="small"><b>${k[0].toUpperCase()}:</b> ${esc(s.soap[k]).slice(0, 220)}</div>`).join("")}
          ${s.notes ? `<div class="small dim clip">${esc(s.notes)}</div>` : ""}${s.homework ? `<div class="small"><span data-ic="bookmark"></span> ${esc(s.homework).slice(0, 160)}</div>` : ""}</div>`).join("") : `<div class="dim small">${tr("Nema sesija.", "No sessions.")}</div>`}</div></div>
        <div class="card2"><div class="h"><span data-ic="list"></span>${tr("Upitnici", "Questionnaires")}</div>
          <table class="stable small"><tbody>${ch.questionnaires.map((q) => `<tr><td>${fmtDate(q.date)}</td><td><b>${esc(q.kind)}</b></td><td class="mono r">${num(q.score, 0)}${q.max ? " / " + num(q.max, 0) : ""}</td><td class="dim">${esc(q.note || "")}</td><td><button class="small" data-qdel="${q.id}">✕</button></td></tr>`).join("") || `<tr><td class="dim">${tr("Nema upisanih upitnika.", "No questionnaires entered.")}</td></tr>`}</tbody></table>
          <form class="row small kt-q"><select name="kind">${Q_KINDS.map(([k, m]) => `<option value="${k}" data-max="${m ?? ""}">${k}${m ? ` (0–${m})` : ""}</option>`).join("")}</select><input name="score" type="number" step="0.1" placeholder="${tr("rezultat", "score")}" required style="width:90px"><input name="date" type="date" value="${new Date().toISOString().slice(0, 10)}"><input name="note" placeholder="${tr("napomena", "note")}" class="grow"><button class="pri small">${tr("Dodaj", "Add")}</button></form>
          <div class="dim small">${tr("Upisuje se ukupni rezultat validiranog upitnika (npr. VHI-10: 0–40, više = veći hendikep).", "Enter the total score of the validated questionnaire (e.g. VHI-10: 0–40, higher = more handicap).")}</div></div>
        <div class="card2"><div class="h"><span data-ic="edit"></span>${tr("Perceptivne ocjene", "Perceptual ratings")}</div>
          <table class="stable small"><tbody>${ch.recordings.filter((r) => r.rating).map((r) => `<tr class="click" data-r="${r.id}"><td>${fmtDate(r.created)}</td><td>${esc(r.task || r.label || "")}</td><td class="mono">${grbasText(r.rating)}</td><td class="mono">${r.rating.capev && r.rating.capev[0] != null ? "CAPE-V " + Math.round(r.rating.capev[0]) : ""}</td></tr>`).join("") || `<tr><td class="dim">${tr("Nema ocjena — Snimanje i mjerenja → Perceptivna ocjena.", "No ratings — Recording & measures → Perceptual rating.")}</td></tr>`}</tbody></table></div>
        <div class="card2 kt-wide"><div class="h"><span data-ic="file"></span>${tr("Bilješke u kartonu", "Chart notes")}</div>
          <form class="kt-n"><input name="title" placeholder="${tr("naslov (npr. Kontrola nakon 6 tjedana)", "title (e.g. Follow-up after 6 weeks)")}"><textarea name="text" rows="3" placeholder="${tr("bilješka, nalaz, dogovor s klijentom…", "note, finding, agreement with the client…")}" required></textarea><button class="pri small">${tr("Spremi bilješku", "Save note")}</button></form>
          <div class="kt-notes">${ch.notes.map((n) => `<div class="kt-note"><div class="row small"><b class="grow">${esc(n.title || tr("Bilješka", "Note"))}</b>${n.source && n.source.startsWith("mcp") ? `<span class="tag acc"><span data-ic="sparkles"></span>AI · ${esc(n.source.slice(4))}</span>` : ""}<span class="dim">${fmtTime(n.created)}</span><button class="small" data-ndel="${n.id}">✕</button></div><div class="md small">${mdLite(n.text)}</div></div>`).join("")}</div></div>
        <div class="card2 kt-wide"><div class="h"><span data-ic="sparkles"></span>${tr("AI mišljenja", "AI opinions")}</div>
          ${ch.ai_reports.length ? ch.ai_reports.map((a) => `<details class="kt-ai"><summary>${fmtTime(a.created)} · ${esc(a.model)}${a.question ? " · " + esc(a.question).slice(0, 80) : ""}</summary><div class="md small">${mdLite(a.text)}</div></details>`).join("") : `<div class="dim small">${tr("Nema AI mišljenja. Pokreću se u Analizi (AI mišljenje) ili preko AI asistenta (MCP).", "No AI opinions. Start one in Analysis (AI opinion) or through an AI assistant (MCP).")}</div>`}</div>
      </div>`;
    Icons.apply(box);
    box.querySelector('[data-a="edit"]').onclick = () => $("#clEditP").click();
    box.querySelector('[data-a="rec"]').onclick = () => C.setTab("analiza");
    box.querySelector('[data-a="print"]').onclick = () => printChart(ch);
    box.querySelectorAll("[data-s]").forEach((d) => (d.onclick = () => C.openSession(d.dataset.s)));
    box.querySelectorAll("tr[data-r]").forEach((tr0) => (tr0.onclick = () => { const r = st.recs.find((x) => x.id === tr0.dataset.r); if (r) { C.openAnalysis(r).then(() => document.querySelector('#anTabs [data-t="ocjena"]').click()); } }));
    const qf = box.querySelector(".kt-q");
    qf.onsubmit = async (e) => {
      e.preventDefault();
      const opt = qf.kind.selectedOptions[0];
      try { await api("POST", `/api/patients/${pid}/questionnaires`, { kind: qf.kind.value, score: +qf.score.value, max: opt.dataset.max ? +opt.dataset.max : null, date: qf.date.value ? new Date(qf.date.value + "T12:00").getTime() : null, note: qf.note.value }); karton(); }
      catch (er) { toast(er.message); }
    };
    box.querySelectorAll("[data-qdel]").forEach((b) => (b.onclick = async () => { if (confirm(tr("Obrisati upitnik?", "Delete the questionnaire?"))) { await api("DELETE", "/api/questionnaires/" + b.dataset.qdel).catch((e) => toast(e.message)); karton(); } }));
    const nf = box.querySelector(".kt-n");
    nf.onsubmit = async (e) => { e.preventDefault(); try { await api("POST", `/api/patients/${pid}/notes`, { title: nf.title.value, text: nf.text.value }); karton(); } catch (er) { toast(er.message); } };
    box.querySelectorAll("[data-ndel]").forEach((b) => (b.onclick = async () => { if (confirm(tr("Obrisati bilješku?", "Delete the note?"))) { await api("DELETE", "/api/notes/" + b.dataset.ndel).catch((e) => toast(e.message)); karton(); } }));
    // voice profile (needs the analysis of the task recordings; may take a moment)
    try { prof = await api("GET", `/api/patients/${pid}/profile`); } catch (_) {}
    const pb = $("#ktProfile"); if (!pb || !prof || !st.patient || st.patient.id !== pid) return;
    pb.innerHTML = `<div class="h"><span data-ic="gauge"></span>${tr("Glasovni profil", "Voice profile")}</div>` + profileHtml(prof.profile) +
      `<div class="dim small">${tr("Iz zadnje snimke svakog zadatka (Snimanje i mjerenja → zadatak).", "From the latest recording of each task (Recording & measures → task).")}</div>`;
    Icons.apply(pb);
  }
  function fmtNext(d) { const t = new Date(d.length === 10 ? d + "T00:00" : d); return isNaN(t) ? d : d.length > 10 ? fmtTime(t.getTime()) : fmtDate(t.getTime()); }
  function profileHtml(p) {
    const row = (label, v, unit, bad, d = 1, hint = "") => `<span class="dim">${label}</span><b class="mono ${v == null ? "dim" : bad ? "badc" : "okc"}" title="${esc(hint)}">${v == null ? "—" : num(v, d) + (unit ? " " + unit : "")}</b>`;
    const dsi = p.dsi;
    return `<div class="kv kt-prof">
      ${row("MPT", p.mpt_s, "s", p.mpt_s != null && p.mpt_s < 15, 1, tr("najdulja fonacija; odrasli ≥ 15 s", "longest phonation; adults ≥ 15 s"))}
      ${row("s/z", p.sz_ratio, "", p.sz_ratio != null && p.sz_ratio > 1.4, 2, tr("> 1,4 upućuje na glotičnu insuficijenciju", "> 1.4 suggests glottal insufficiency"))}
      <span class="dim">F0 ${tr("raspon (glisando)", "range (glissando)")}</span><b class="mono ${p.f0_high_hz == null ? "dim" : ""}">${p.f0_high_hz == null ? "—" : `${num(p.f0_low_hz, 0)}–${num(p.f0_high_hz, 0)} Hz`}</b>
      ${row("I-low", p.i_low_db_spl, "dB SPL", false, 1)}
      ${row("Jitter (/a/)", p.jitter_pct, "%", p.jitter_pct != null && p.jitter_pct > 1.04, 2)}
      ${row("DSI", dsi, "", dsi != null && dsi < 1.6, 2, tr("Dysphonia Severity Index: +5 normalno, −5 teška disfonija", "Dysphonia Severity Index: +5 normal, −5 severe dysphonia"))}
    </div>${dsi != null ? `<div class="dsi-bar"><i style="left:${Math.max(0, Math.min(100, (dsi + 5) / 10 * 100))}%"></i></div><div class="row small dim"><span>−5</span><span class="grow"></span><span>0</span><span class="grow"></span><span>+5</span></div>` : ""}
    ${(p.notes || []).map((n) => `<div class="small dim">· ${esc(I18N.lang === "en" ? n[1] : n[0])}</div>`).join("")}`;
  }
  function mdLite(t) { return esc(t).replace(/^### (.*)$/gm, "<b>$1</b>").replace(/^## (.*)$/gm, "<b>$1</b>").replace(/^# (.*)$/gm, "<b>$1</b>").replace(/\*\*(.+?)\*\*/g, "<b>$1</b>").replace(/\n/g, "<br>"); }
  function printChart(ch) {
    const w = window.open("", "_blank"); if (!w) return toast(tr("Preglednik je blokirao novi prozor — dopustite skočne prozore", "The browser blocked the new window — allow pop-ups"));
    const p = ch.patient;
    w.document.write(`<!doctype html><html lang="${I18N.lang}"><head><meta charset="utf-8"><title>${esc(tr("Karton", "Chart"))} — ${esc(p.name)}</title><style>body{font:11pt/1.45 system-ui,sans-serif;margin:16mm;color:#111}h1{font-size:17pt;margin:0}h2{font-size:12pt;border-bottom:1px solid #999;margin:16px 0 6px}table{border-collapse:collapse;width:100%}td,th{padding:2px 6px;text-align:left;vertical-align:top;border-bottom:1px solid #ddd;font-size:10pt}.dim{color:#666}.pre{white-space:pre-wrap}@media print{.noprint{display:none}}</style></head><body>
      <div class="noprint"><button>${tr("Ispis / PDF", "Print / PDF")}</button></div>
      <h1>${esc(p.name)}${p.code ? " (" + esc(p.code) + ")" : ""}</h1><div class="dim">${esc(ch.settings.clinic_name || "")} · ${tr("ispisano", "printed")} ${fmtTime(Date.now())}</div>
      <h2>${tr("Osnovni podaci", "Details")}</h2><table>${[[tr("Datum rođenja", "Date of birth"), p.birth], [tr("Spol", "Sex"), p.sex], [tr("Dijagnoza", "Diagnosis"), p.diagnosis], [tr("Razlog dolaska", "Complaint"), p.complaint], [tr("Povijest problema", "History"), p.history], [tr("Medicinska anamneza", "Medical history"), p.medical], [tr("Lijekovi", "Medications"), p.medications], [tr("Glasovno opterećenje", "Vocal load"), p.voice_use], [tr("Ciljevi", "Goals"), p.goals]].filter((x) => x[1]).map(([k, v]) => `<tr><th>${esc(k)}</th><td class="pre">${esc(v)}</td></tr>`).join("")}</table>
      <h2>${tr("Sesije", "Sessions")}</h2><table>${ch.sessions.map((s) => `<tr><td>${fmtDate(s.start)}</td><td>${esc(s.kind || "")}</td><td class="pre">${["subjective", "objective", "assessment", "plan"].filter((k) => s.soap && s.soap[k]).map((k) => `<b>${k[0].toUpperCase()}:</b> ${esc(s.soap[k])}`).join("<br>")}${s.notes ? "<br>" + esc(s.notes) : ""}${s.homework ? "<br><i>" + esc(s.homework) + "</i>" : ""}</td></tr>`).join("")}</table>
      <h2>${tr("Upitnici", "Questionnaires")}</h2><table>${ch.questionnaires.map((q) => `<tr><td>${fmtDate(q.date)}</td><td>${esc(q.kind)}</td><td>${q.score}${q.max ? " / " + q.max : ""}</td></tr>`).join("") || "<tr><td>—</td></tr>"}</table>
      <h2>${tr("Perceptivne ocjene", "Perceptual ratings")}</h2><table>${ch.recordings.filter((r) => r.rating).map((r) => `<tr><td>${fmtDate(r.created)}</td><td>${esc(r.task)}</td><td>${grbasText(r.rating)}</td></tr>`).join("") || "<tr><td>—</td></tr>"}</table>
      <h2>${tr("Bilješke", "Notes")}</h2>${ch.notes.map((n) => `<p><b>${esc(n.title || "")}</b> <span class="dim">${fmtTime(n.created)}${n.source ? " · " + esc(n.source) : ""}</span><br><span class="pre">${esc(n.text)}</span></p>`).join("") || "—"}
      </body></html>`);
    w.document.close();
    w.document.querySelector(".noprint button").onclick = () => w.print();
  }

  // ================================================================ DigiLingua FILTER: real-time switch
  let bypassed = false;
  function bypassAll(on) { bypassed = !!on; filterBar(); }
  function filterBar() {
    const box = $("#flBar"); if (!box) return;
    const run = AUD.running, live = run && !bypassed;
    box.className = "card2 fl-bar " + (live ? "on" : run ? "byp" : "off");
    box.innerHTML = `<button class="fl-switch" aria-pressed="${!!live}" title="${tr("Uključi / isključi filter u stvarnom vremenu", "Switch the real-time filter on / off")}"><i></i></button>
      <div class="grow"><div class="fl-state">${live ? tr("FILTER UKLJUČEN — klijent čuje obrađeni glas", "FILTER ON — the client hears the processed voice") : run ? tr("Filter isključen — prolazi čisti signal (zvuk radi)", "Filter off — the clean signal passes (audio running)") : tr("Zvuk je zaustavljen — uključite filter u stvarnom vremenu", "Audio is stopped — switch the real-time filter on")}</div>
        <div class="small dim">${run ? esc(`${run.input || tr("bez ulaza", "no input")} → ${run.output} · ${(run.sample_rate / 1000).toFixed(1)} kHz · ${run.in_channels}×${run.out_channels} ${tr("kanala", "channels")}`) : tr("Odaberite zvučnu karticu ispod, namjestite filtre, pa uključite.", "Choose the sound card below, set the filters, then switch on.")}</div></div>
      ${run ? `<button class="small" data-stop>■ ${tr("Zaustavi zvuk", "Stop audio")}</button>` : ""}`;
    box.querySelector(".fl-switch").onclick = async () => {
      if (!AUD.running) { await C.startAudio(); send({ t: "bypass_all", on: false }); }
      else send({ t: "bypass_all", on: !bypassed ? true : false });
    };
    const sb = box.querySelector("[data-stop]"); if (sb) sb.onclick = () => $("#clAudioBtn").click();
  }

  // ================================================================ sound card: all channels + routing matrix
  const isPhones = (n) => /headphone|slušalic|kopfh|casque|auricular|cuffie/i.test(n || "");
  function devChannels() {
    const h = AUD.devices.find((d) => d.host === ($("#clHost").value || (AUD.running && AUD.running.host))) || AUD.devices[0] || {};
    const pick = (names, counts, cur, def) => { const n = cur || def; const i = (names || []).indexOf(n); return i >= 0 && counts ? counts[i] : 2; };
    return {
      in: AUD.running ? AUD.running.in_channels : pick(h.inputs, h.input_channels, $("#clIn").value, h.default_input),
      out: AUD.running ? AUD.running.out_channels : pick(h.outputs, h.output_channels, $("#clOut").value, h.default_output),
      phones: (h.outputs || []).filter(isPhones),
    };
  }
  const defIn = (c, n) => (c === 0 ? (n === 1 ? [1, 1] : [1, 0]) : c === 1 ? [0, 1] : [0, 0]);
  const defOut = (c) => (c === 0 ? [1, 0, 0, 0] : c === 1 ? [0, 1, 0, 0] : [0, 0, 0, 0]);
  let mtx = null; // working copy {inputs, outputs}
  function routing() { const r = (AUD.cfg && AUD.cfg.routing) || {}; return { inputs: r.inputs || [], outputs: r.outputs || [] }; }
  const saveRouting = debounce(async () => { try { AUD.cfg.routing = await api("PUT", "/api/audio/routing", mtx); } catch (e) { toast(e.message); } }, 250);
  function matrix() {
    const box = $("#flMatrix"); if (!box) return;
    const ch = devChannels(), r = routing();
    mtx = {
      inputs: Array.from({ length: ch.in || 0 }, (_, c) => (r.inputs.length ? r.inputs[c] || [0, 0] : defIn(c, ch.in)).slice()),
      outputs: Array.from({ length: ch.out || 0 }, (_, c) => (r.outputs.length ? r.outputs[c] || [0, 0, 0, 0] : defOut(c)).slice()),
    };
    const outName = (AUD.running && AUD.running.output) || $("#clOut").value || "";
    const cell = (on, a) => `<td class="mx"><button type="button" class="mxb${on ? " on" : ""}" ${a}>${on ? "●" : ""}</button></td>`;
    box.innerHTML = `<div class="h row"><span data-ic="sliders"></span><span class="grow">${tr("Zvučna kartica — kanali i usmjeravanje", "Sound card — channels and routing")}</span>
        <label class="chk small"><input type="checkbox" id="mxAll" ${AUD.cfg && AUD.cfg.all_channels ? "checked" : ""}> ${tr("koristi sve kanale uređaja", "use all channels of the device")}</label></div>
      <div class="dim small">${tr("Ulazi kartice → ulaz filtra (L/D); izlaz filtra i suhi mikrofon → svaki izlaz kartice (npr. izlaz za slušalice na kanalima 3/4). Promjene vrijede odmah.", "Card inputs → filter input (L/R); filter output and the dry microphone → every card output (e.g. a headphone output on channels 3/4). Changes apply at once.")}
        ${ch.phones.length ? `<br><span data-ic="headphones"></span> ${tr("Izlazi za slušalice na ovom računalu", "Headphone outputs on this computer")}: <b>${ch.phones.map(esc).join(", ")}</b> ${tr("— odaberite ih kao izlazni uređaj gore.", "— choose one as the output device above.")}` : ""}
        ${isPhones(outName) ? ` <span class="tag acc"><span data-ic="headphones"></span>${tr("izlaz je za slušalice", "output is a headphone output")}</span>` : ""}</div>
      <div class="mx-grid">
        <div><div class="side-label">${tr("Ulazi", "Inputs")} (${ch.in || 0})</div>
          <table class="mxt"><thead><tr><th></th><th>${tr("razina", "level")}</th><th>L</th><th>${tr("D", "R")}</th></tr></thead><tbody>${mtx.inputs.map((g, c) => `<tr><th>${tr("Ulaz", "In")} ${c + 1}</th><td><div class="mxm" data-in="${c}"><i></i></div></td>${cell(g[0] > 0, `data-i="${c}" data-k="0"`)}${cell(g[1] > 0, `data-i="${c}" data-k="1"`)}</tr>`).join("") || `<tr><td class="dim small" colspan="4">${tr("bez ulaza", "no input")}</td></tr>`}</tbody></table></div>
        <div><div class="side-label">${tr("Izlazi", "Outputs")} (${ch.out || 0})</div>
          <table class="mxt"><thead><tr><th></th><th>${tr("razina", "level")}</th><th title="${tr("filtar lijevo", "filter left")}">${tr("Filtar L", "Filter L")}</th><th title="${tr("filtar desno", "filter right")}">${tr("Filtar D", "Filter R")}</th><th title="${tr("suhi mikrofon lijevo (izravno)", "dry microphone left (direct)")}">${tr("Mik L", "Mic L")}</th><th title="${tr("suhi mikrofon desno (izravno)", "dry microphone right (direct)")}">${tr("Mik D", "Mic R")}</th></tr></thead><tbody>${mtx.outputs.map((g, c) => `<tr><th>${tr("Izlaz", "Out")} ${c + 1}</th><td><div class="mxm" data-out="${c}"><i></i></div></td>${[0, 1, 2, 3].map((k) => cell(g[k] > 0, `data-o="${c}" data-k="${k}"`)).join("")}</tr>`).join("")}</tbody></table></div>
      </div>
      <div class="row small"><button data-p="def">${tr("Zadano", "Default")}</button><button data-p="mono">${tr("Mikrofon (ulaz 1) → oba kanala", "Microphone (input 1) → both channels")}</button><button data-p="all">${tr("Filtar na sve izlazne parove", "Filter on every output pair")}</button><button data-p="mon">${tr("Izlazi 3/4: suhi mikrofon (kontrola)", "Outputs 3/4: dry microphone (monitor)")}</button></div>`;
    Icons.apply(box);
    $("#mxAll").onchange = async (e) => { AUD.cfg = Object.assign(C.audioCfg(), { all_channels: e.target.checked }); if (AUD.running) await C.startAudio(); else matrix(); };
    box.querySelectorAll(".mxb").forEach((b) => (b.onclick = () => {
      const k = +b.dataset.k;
      if (b.dataset.i != null) { const g = mtx.inputs[+b.dataset.i]; g[k] = g[k] > 0 ? 0 : 1; }
      else { const g = mtx.outputs[+b.dataset.o]; g[k] = g[k] > 0 ? 0 : 1; }
      b.classList.toggle("on"); b.textContent = b.classList.contains("on") ? "●" : "";
      saveRouting();
    }));
    box.querySelectorAll("[data-p]").forEach((b) => (b.onclick = () => {
      const p = b.dataset.p;
      if (p === "def") { mtx.inputs = mtx.inputs.map((_, c) => defIn(c, mtx.inputs.length)); mtx.outputs = mtx.outputs.map((_, c) => defOut(c)); }
      if (p === "mono") mtx.inputs = mtx.inputs.map((_, c) => (c === 0 ? [1, 1] : [0, 0]));
      if (p === "all") mtx.outputs = mtx.outputs.map((g, c) => (c % 2 === 0 ? [1, 0, 0, 0] : [0, 1, 0, 0]));
      if (p === "mon") mtx.outputs = mtx.outputs.map((g, c) => (c === 2 ? [0, 0, 1, 0] : c === 3 ? [0, 0, 0, 1] : g));
      AUD.cfg.routing = { inputs: mtx.inputs, outputs: mtx.outputs };
      saveRouting(); matrix();
    }));
  }
  function matrixMeters(m) {
    const set = (sel, v) => { const i = document.querySelector(sel + " i"); if (i) i.style.width = Math.max(0, Math.min(100, (20 * Math.log10((v || 0) + 1e-9) + 60) / 60 * 100)) + "%"; };
    (m.ch_in || []).forEach((v, c) => set(`.mxm[data-in="${c}"]`, v));
    (m.ch_out || []).forEach((v, c) => set(`.mxm[data-out="${c}"]`, v));
  }

  // ================================================================ added filters and monitors (chain extras)
  const EXTRA_KINDS = ["compressor", "noise_gate", "limiter", "chorus", "metronome", "notch", "filter", "gain", "delay", "vu_meter", "sonagram"];
  const KIND_HR = { compressor: ["Kompresor", "ujednačuje glasnoću tihih i glasnih dijelova"], noise_gate: ["Vrata šuma (gate)", "utišava pozadinu i disanje između riječi"],
    limiter: ["Limiter — zaštita sluha", "tvrda gornja granica razine u slušalicama"], chorus: ["Zborni govor (chorus)", "više glasova uz klijentov — učinak zbornog čitanja"],
    metronome: ["Metronom / ritam", "otkucaji za ritmizirani govor"], notch: ["Notch / brujanje", "uklanja brujanje mreže (50 Hz) ili zviždanje povratne veze"],
    filter: ["Filtar (LP/HP/BP)", "niski, visoki, pojasni ili vršni filtar"], gain: ["Pojačanje", "razina u dB"], delay: ["Jeka (delay)", "kašnjenje s povratnom vezom"],
    vu_meter: ["VU metar", "razina s ciljnom zonom za trening glasnoće"], sonagram: ["Sonagram", "spektrogram uživo na ovom mjestu lanca"] };
  const SONO_RANGE = [4000, 5000, 8000, 12000];
  const monitors = {};
  /** The board as a chain from IN to OUT (DigiLingua boards are linear). */
  function chain() {
    const out = [], seen = new Set();
    let cur = "in";
    for (let i = 0; i < 64; i++) {
      const c = BOARD.connections.find((x) => x.from === cur && !seen.has(x.to));
      if (!c) break;
      if (c.to === "out") break;
      seen.add(c.to); out.push(c.to); cur = c.to;
    }
    return out;
  }
  const nodeById = (id) => BOARD.nodes.find((n) => n.id === id);
  const label = (n) => { const r = { mic: "Mikrofon", eq: "EQ", daf: "DAF", faf: "FAF", interrupt: tr("Diskontinuitet", "Interrupter"), noise: tr("Šum", "Noise"), ears: tr("Uši", "Ears") }[n.role]; return r || (I18N.lang === "en" ? (TYPES[n.kind] || {}).name : (KIND_HR[n.kind] || [])[0]) || n.kind; };
  async function addExtra(kind, after) {
    const ids = chain(), i = ids.indexOf(after), prev = after || "in", next = i >= 0 && i + 1 < ids.length ? ids[i + 1] : after ? "out" : ids[0] || "out";
    try {
      const n = await api("POST", "/api/nodes", { kind, x: 40, y: 600 });
      await api("DELETE", "/api/connections", { from: prev, to: next }).catch(() => {});
      await api("POST", "/api/connections", { from: prev, to: n.id });
      await api("POST", "/api/connections", { from: n.id, to: next });
    } catch (e) { toast(e.message); }
  }
  async function removeExtra(id) {
    const ins = BOARD.connections.filter((c) => c.to === id).map((c) => c.from), outs = BOARD.connections.filter((c) => c.from === id).map((c) => c.to);
    try {
      for (const a of ins) for (const b of outs) await api("POST", "/api/connections", { from: a, to: b }).catch(() => {});
      await api("DELETE", "/api/nodes/" + id);
    } catch (e) { toast(e.message); }
  }
  function extras() {
    const box = $("#flExtra"); if (!box) return;
    const ids = chain(), extraNodes = BOARD.nodes.filter((n) => !n.role);
    const after = ids.map((id) => nodeById(id)).filter(Boolean);
    box.innerHTML = `<div class="h row"><span data-ic="layers"></span><span class="grow">${tr("Dodatni filtri i monitori", "Extra filters and monitors")}</span>
        <select id="fxKind">${EXTRA_KINDS.map((k) => `<option value="${k}">${esc(I18N.lang === "en" ? (TYPES[k] || {}).name || k : KIND_HR[k][0])}</option>`).join("")}</select>
        <label class="small">${tr("nakon", "after")} <select id="fxAfter">${after.map((n) => `<option value="${n.id}">${esc(label(n))}</option>`).join("")}</select></label>
        <button class="pri small" id="fxAdd"><span data-ic="plus"></span>${tr("Dodaj u lanac", "Add to the chain")}</button></div>
      <div class="dim small" id="fxHint"></div>
      <div class="fx-chain small">${["IN", ...ids.map((id) => nodeById(id)).filter(Boolean).map((n) => `<span class="${n.role ? "" : "x"}${n.bypass ? " off" : ""}">${esc(label(n))}</span>`), "OUT"].join(" › ")}</div>
      <div class="cl-grid fx-grid">${extraNodes.map((n) => `<div class="card2 mod fx${n.bypass ? " byp" : ""}" data-id="${n.id}"><div class="h row"><span class="grow">${esc(label(n))}</span><button class="tog${n.bypass ? "" : " on"}" title="${tr("Uključi / isključi", "On / off")}"></button><button class="small" data-rm title="${tr("Ukloni iz lanca", "Remove from the chain")}">✕</button></div>
        ${n.kind === "sonagram" ? '<canvas class="mon-sono big"></canvas>' : n.kind === "vu_meter" ? '<div class="mon-vu big"></div>' : ""}
        ${(TYPES[n.kind] || { params: [] }).params.map((s) => {
          const v = n.params[s.id] ?? s.default;
          return s.options.length ? `<label class="ctl"><span>${esc(s.name)}</span><select data-p="${s.id}">${s.options.map((o, i) => `<option value="${i}" ${i === Math.round(v) ? "selected" : ""}>${esc(o)}</option>`).join("")}</select></label>`
            : `<label class="ctl"><span>${esc(s.name)}</span><input type="range" data-p="${s.id}" min="0" max="1000" value="${toSlider(s, v)}"><b class="mono small" data-v="${s.id}">${fmt(s, v)}</b></label>`;
        }).join("")}</div>`).join("") || `<div class="dim small">${tr("Nema dodatnih modula. Dodajte npr. kompresor, limiter za zaštitu sluha, metronom ili sonagram na bilo koje mjesto u lancu.", "No extra modules. Add e.g. a compressor, a hearing-protection limiter, a metronome or a sonagram anywhere in the chain.")}</div>`}</div>`;
    Icons.apply(box);
    const hint = () => { const k = $("#fxKind").value; $("#fxHint").textContent = I18N.lang === "en" ? "" : KIND_HR[k] ? KIND_HR[k][1] : ""; };
    $("#fxKind").onchange = hint; hint();
    const ears = after.find((n) => n.role === "ears"); if (ears) { const i = after.indexOf(ears); if (i > 0) $("#fxAfter").value = after[i - 1].id; }
    $("#fxAdd").onclick = () => addExtra($("#fxKind").value, $("#fxAfter").value);
    for (const k of Object.keys(monitors)) delete monitors[k];
    box.querySelectorAll(".fx").forEach((c) => {
      const n = nodeById(c.dataset.id); if (!n) return;
      c.querySelector(".tog").onclick = () => { setBypass(n.id, !n.bypass); extras(); };
      c.querySelector("[data-rm]").onclick = () => removeExtra(n.id);
      c.querySelectorAll("[data-p]").forEach((inp) => {
        const s = TYPES[n.kind].params.find((x) => x.id === inp.dataset.p);
        inp.oninput = inp.onchange = () => {
          const v = inp.tagName === "SELECT" ? +inp.value : fromSlider(s, +inp.value);
          setParam(n.id, s.id, v);
          const vb = c.querySelector(`[data-v="${s.id}"]`); if (vb) vb.textContent = fmt(s, v);
          if (n.kind === "sonagram" && monitors[n.id]) monitors[n.id].sono.fmax = SONO_RANGE[Math.round(v)] || 5000;
        };
      });
      if (n.kind === "sonagram") monitors[n.id] = { sono: new Sono.TapView(c.querySelector("canvas"), { fmax: SONO_RANGE[Math.round(n.params.range ?? 1)] || 5000 }) };
      if (n.kind === "vu_meter") monitors[n.id] = { vu: new Sono.VuView(c.querySelector(".mon-vu")) };
    });
    subscribeTaps();
  }
  function subscribeTaps() {
    const on = st.visible && st.tab === "rehab" && !document.hidden;
    tapView("clinic", on ? BOARD.nodes.filter((n) => n.kind === "sonagram" && !n.role).map((n) => n.id) : []);
  }
  document.addEventListener("visibilitychange", subscribeTaps);

  // ================================================================ AI assistants (MCP) settings
  async function mcpBox() {
    const box = $("#mcpBox"); if (!box) return;
    let r;
    try { r = await api("GET", "/api/mcp/config"); } catch (_) { return; }
    const legend = `<legend>${tr("AI asistenti (MCP) — Claude, ChatGPT…", "AI assistants (MCP) — Claude, ChatGPT…")}</legend>`;
    if (!r.config) {
      box.innerHTML = legend + `<div class="dim small">${tr("AI asistent (Claude Desktop, Claude Code, ChatGPT) spaja se na instaliranu aplikaciju SterOidSoundBoard — preglednik ne može primati veze. Instalirajte desktop aplikaciju (GitHub → Releases) i pokrenite je s istom mapom podataka koju koristi ova stranica: <code>steroidsoundboard --data \"…mapa…\"</code>, zatim ovdje u desktop aplikaciji uključite MCP.", "An AI assistant (Claude Desktop, Claude Code, ChatGPT) connects to the installed SterOidSoundBoard app — a browser cannot accept connections. Install the desktop app (GitHub → Releases) and start it with the same data folder this page uses: <code>steroidsoundboard --data \"…folder…\"</code>, then switch MCP on in the desktop app.")}</div>`;
      return;
    }
    const c = r.config, exe = r.exe || "steroidsoundboard", host = location.hostname === "localhost" ? "127.0.0.1" : location.hostname, port = location.port || "8420";
    const desktop = JSON.stringify({ mcpServers: { steroidsoundboard: { command: exe, args: ["--mcp", "--data", r.data] } } }, null, 2);
    box.innerHTML = legend + `
      <div class="dim small">${tr("Claude ili ChatGPT dobivaju pristup kartonima: pretraga pacijenata, anamneza, sesije, mjerenja (Praat), napredak; mogu spremiti svoju procjenu kao bilješku. Vi odlučujete što smiju vidjeti.", "Claude or ChatGPT get access to the charts: patient search, history, sessions, measurements (Praat), progress; they can save their assessment as a note. You decide what they may see.")}</div>
      <div class="pgrid2">
        <label class="chk"><input type="checkbox" name="mcp_on" ${c.enabled ? "checked" : ""}> ${tr("uključi MCP poslužitelj", "enable the MCP server")}</label>
        <label class="chk"><input type="checkbox" name="mcp_consent" ${c.consent_only ? "checked" : ""}> ${tr("samo pacijenti sa suglasnošću za AI", "only patients with AI consent")}</label>
        <label>${tr("Identitet pacijenata", "Patient identity")}<select name="mcp_id"><option value="pseudonym" ${c.identity !== "full" ? "selected" : ""}>${tr("pseudonimizirano (bez imena, šifre, datuma rođenja)", "pseudonymised (no name, code, birth date)")}</option><option value="full" ${c.identity === "full" ? "selected" : ""}>${tr("puni podaci (ime, šifra, datum rođenja)", "full details (name, code, birth date)")}</option></select></label>
        <label class="chk"><input type="checkbox" name="mcp_write" ${c.allow_write ? "checked" : ""}> ${tr("AI smije spremati bilješke u karton", "the AI may save notes to the chart")}</label>
      </div>
      <div class="warnc small">${tr("Pacijenta se može pronaći po imenu i u pseudonimiziranom načinu. Podaci odlaze AI servisu koji koristi vaš klijent (Anthropic, OpenAI) — provjerite usklađenost s GDPR-om i suglasnost pacijenta.", "A patient can be found by name in pseudonymised mode too. Data goes to the AI service your client uses (Anthropic, OpenAI) — check GDPR compliance and the patient's consent.")}</div>
      <div class="row small"><b>${tr("Token", "Token")}</b><code class="grow" id="mcpTok">${"•".repeat(16)}</code><button type="button" class="small" data-t="show">${tr("Prikaži", "Show")}</button><button type="button" class="small" data-t="copy"><span data-ic="copy"></span></button><button type="button" class="small" data-t="new">${tr("Novi token", "New token")}</button><button type="button" class="pri small" data-t="save">${tr("Spremi MCP", "Save MCP")}</button></div>
      <details><summary>${tr("Claude Desktop (preporučeno, lokalno)", "Claude Desktop (recommended, local)")}</summary><div class="small dim">${tr("Postavke → Developer → Edit Config, dodajte u claude_desktop_config.json i ponovno pokrenite Claude:", "Settings → Developer → Edit Config, add to claude_desktop_config.json and restart Claude:")}</div><pre class="code" data-noi18n>${esc(desktop)}</pre></details>
      <details><summary>Claude Code</summary><pre class="code" data-noi18n>${esc(`claude mcp add steroidsoundboard -- "${exe}" --mcp --data "${r.data}"`)}</pre></details>
      <details><summary>${tr("ChatGPT i drugi HTTP klijenti", "ChatGPT and other HTTP clients")}</summary><div class="small dim">${tr("Adresa (Streamable HTTP), zaglavlje Authorization: Bearer &lt;token&gt; ili ?token=&lt;token&gt;. ChatGPT traži javnu HTTPS adresu: npr. Cloudflare Tunnel (cloudflared tunnel --url http://127.0.0.1:" + port + ") pa u ChatGPT → Settings → Connectors → Developer mode dodajte https://…/mcp?token=…", "Address (Streamable HTTP), header Authorization: Bearer &lt;token&gt; or ?token=&lt;token&gt;. ChatGPT needs a public HTTPS address: e.g. a Cloudflare Tunnel (cloudflared tunnel --url http://127.0.0.1:" + port + ") then ChatGPT → Settings → Connectors → Developer mode, add https://…/mcp?token=…")}</div><pre class="code" data-noi18n>http://${esc(host)}:${esc(port)}/mcp</pre></details>`;
    Icons.apply(box);
    let token = c.token;
    const save = async (extra = {}) => {
      const f = box;
      const b = { enabled: f.querySelector('[name="mcp_on"]').checked, identity: f.querySelector('[name="mcp_id"]').value, consent_only: f.querySelector('[name="mcp_consent"]').checked, allow_write: f.querySelector('[name="mcp_write"]').checked, ...extra };
      try { const x = await api("PUT", "/api/mcp/config", b); token = x.config.token; C.toastInfo(tr("MCP postavke spremljene", "MCP settings saved")); } catch (e) { toast(e.message); }
    };
    box.querySelectorAll("[data-t]").forEach((b) => (b.onclick = async () => {
      const k = b.dataset.t;
      if (k === "show") $("#mcpTok").textContent = token;
      if (k === "copy") navigator.clipboard?.writeText(token).then(() => C.toastInfo(tr("Token kopiran", "Token copied")));
      if (k === "new" && confirm(tr("Napraviti novi token? Postojeći klijenti morat će ga ponovno upisati.", "Create a new token? Existing clients will need the new one."))) { await save({ new_token: true }); $("#mcpTok").textContent = token; }
      if (k === "save") save();
    }));
  }
  $("#clSettingsBtn").addEventListener("click", () => setTimeout(mcpBox, 50));

  // ================================================================ hooks from clinic.js
  function meters(m) {
    matrixMeters(m);
    for (const [id, v] of Object.entries(m.vu || {})) {
      const mo = monitors[id], n = nodeById(id);
      if (mo && mo.vu && n) mo.vu.update(v[0], v[1], n.params.target ?? -18, n.params.tolerance ?? 6);
    }
    for (const [id, live] of Object.entries(m.taps || {})) { const mo = monitors[id]; if (mo && mo.sono) mo.sono.push(live); }
    if (m.bypass_all !== undefined && m.bypass_all !== bypassed) bypassAll(m.bypass_all);
  }
  let lastBoardSig = "";
  function board() {
    const sig = JSON.stringify([BOARD.nodes.map((n) => [n.id, n.kind, n.bypass]), BOARD.connections]);
    if (sig !== lastBoardSig) { lastBoardSig = sig; extras(); }
  }
  function audio() { filterBar(); matrix(); }
  // keep the FILTER page's extras in step with tab changes
  document.querySelectorAll("#clTabs button").forEach((b) => b.addEventListener("click", () => setTimeout(subscribeTaps, 0)));
  HOOKS.push((m) => { if (m.t === "mode") setTimeout(() => { subscribeTaps(); filterBar(); matrix(); }, 0); if (m.t === "clinic" && st.tab === "karton") setTimeout(karton, 300); });
  $("#clHost").addEventListener("change", () => setTimeout(matrix, 0));
  $("#clIn").addEventListener("change", () => setTimeout(matrix, 0));
  $("#clOut").addEventListener("change", () => setTimeout(matrix, 0));
  api("GET", "/api/status").then((s) => { bypassed = !!s.bypass_all; filterBar(); }).catch(() => {});
  setTimeout(() => { filterBar(); matrix(); extras(); prompter(); }, 0);

  return { startOpts, session, rating, karton, meters, board, audio, bypassAll };
})();
window.Practice = Practice;
