"use strict";
// Browser edition start-up: the UI's `/api/...` requests and its `/ws` socket
// are answered inside the page (local/server.js); the user decides where the
// data lives; the app can be installed (PWA) and works offline.
(() => {
  const tr = (hr, en) => (I18N.lang === "en" ? en : hr);
  const nativeFetch = window.fetch.bind(window);
  const NativeWS = window.WebSocket;
  let readyOk, readyFail;
  const ready = new Promise((ok, fail) => { readyOk = ok; readyFail = fail; });
  ready.catch(() => {});

  // ---------------------------------------------------------------- /api and /ws stay in the page
  const isApi = (u) => { try { const x = new URL(u, location.href); return x.origin === location.origin && x.pathname.startsWith("/api/"); } catch (_) { return false; } };
  window.fetch = async (input, init = {}) => {
    const url = typeof input === "string" ? input : input.url;
    if (!isApi(url)) return nativeFetch(input, init);
    await ready;
    const method = (init.method || (input.method) || "GET").toUpperCase();
    let body = null, raw = null;
    if (init.body != null) {
      const ct = String((init.headers && (init.headers["Content-Type"] || init.headers["content-type"])) || "");
      if (typeof init.body === "string" && ct.includes("json")) { try { body = JSON.parse(init.body); } catch (_) { body = null; } }
      else raw = new Response(init.body);
    }
    return SsbServer.handle(method, url, body, raw);
  };
  window.WebSocket = function (url, protocols) {
    if (/\/ws$/.test(String(url))) return new SsbServer.LocalSocket();
    return new NativeWS(url, protocols);
  };
  window.WebSocket.prototype = NativeWS.prototype;
  Object.assign(window.WebSocket, { CONNECTING: 0, OPEN: 1, CLOSING: 2, CLOSED: 3 });

  // links to /api/… (WAV, TextGrid) become downloads made in the page
  document.addEventListener("click", async (e) => {
    const a = e.target.closest && e.target.closest("a[href]");
    if (!a || !isApi(a.getAttribute("href"))) return;
    e.preventDefault();
    try {
      const r = await window.fetch(a.getAttribute("href"));
      if (!r.ok) throw new Error((await r.json().catch(() => ({}))).error || r.statusText);
      const name = (/filename="([^"]+)"/.exec(r.headers.get("content-disposition") || "") || [])[1] || a.getAttribute("download") || "download";
      const url = URL.createObjectURL(await r.blob()), d = document.createElement("a");
      d.href = url; d.download = name; document.body.append(d); d.click(); d.remove();
      setTimeout(() => URL.revokeObjectURL(url), 2000);
    } catch (er) { (window.toast || alert)(er.message); }
  }, true);

  // ---------------------------------------------------------------- install (PWA) + offline
  let installEvt = null;
  addEventListener("beforeinstallprompt", (e) => { e.preventDefault(); installEvt = e; document.querySelectorAll(".ssb-install").forEach((b) => b.classList.remove("hidden")); });
  addEventListener("appinstalled", () => { installEvt = null; document.querySelectorAll(".ssb-install").forEach((b) => b.classList.add("hidden")); });
  const installed = () => matchMedia("(display-mode: standalone)").matches || matchMedia("(display-mode: window-controls-overlay)").matches;
  async function install() {
    if (installEvt) { installEvt.prompt(); await installEvt.userChoice.catch(() => {}); installEvt = null; return; }
    alert(tr("Instalacija: u Chromeu ili Edgeu kliknite ikonu „Instaliraj” na desnom kraju adresne trake (ili izbornik ⋮ → Spremi i dijeli → Instaliraj). Na iPadu: Dijeli → Dodaj na početni zaslon.",
      "To install: in Chrome or Edge click the “Install” icon at the right end of the address bar (or menu ⋮ → Save and share → Install). On iPad: Share → Add to Home Screen."));
  }
  if ("serviceWorker" in navigator && location.protocol !== "file:") navigator.serviceWorker.register("sw.js").catch((e) => console.warn("service worker", e));

  // ---------------------------------------------------------------- UI helpers
  const el = (tag, cls, html) => { const e = document.createElement(tag); if (cls) e.className = cls; if (html != null) e.innerHTML = html; return e; };
  const esc = (t) => String(t ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
  const PRIVACY = () => tr(
    "SterOidSoundBoard radi u potpunosti u vašem pregledniku. Podaci o pacijentima, anamneze, snimke i analize <b>nikad se ne šalju na poslužitelj</b> — stranica s interneta dohvaća samo samu aplikaciju. Jedina iznimka je AI mišljenje, i to samo kad ga pokrenete: pseudonimizirani podaci (bez imena, šifre i datuma rođenja) idu izravno AI servisu koji sami odaberete.",
    "SterOidSoundBoard runs entirely in your browser. Patient data, histories, recordings and analyses are <b>never sent to a server</b> — the website only delivers the app itself. The one exception is the AI opinion, and only when you start it: pseudonymised data (no name, code or birth date) goes straight to the AI service you chose.");

  /** First start (or the folder needs permission again): where should the data live? */
  function chooser(state) {
    return new Promise((done) => {
      const ov = el("div", "ssb-boot");
      const folderOk = SsbStore.folderSupported();
      ov.innerHTML = `<div class="ssb-card" role="dialog" aria-modal="true">
        <div class="ssb-logo"><span data-ic="waves"></span>SterOidSoundBoard</div>
        <h2>${tr("Gdje spremati podatke?", "Where should your data be kept?")}</h2>
        <p class="small">${PRIVACY()}</p>
        ${state.needsPermission ? `<button class="pri wide" data-k="again"><span data-ic="archive"></span>${tr("Nastavi s mapom", "Continue with folder")} „${esc(state.needsPermission.name)}”</button>
          <p class="small dim">${tr("Preglednik traži ponovnu potvrdu pristupa mapi s podacima.", "The browser asks you to confirm access to the data folder again.")}</p>` : ""}
        <div class="ssb-opts">
          <button class="ssb-opt" data-k="folder" ${folderOk ? "" : "disabled"}>
            <b><span data-ic="save"></span>${tr("Mapa na ovom računalu", "A folder on this computer")}${state.needsPermission ? "" : ` <span class="tag">${tr("preporučeno", "recommended")}</span>`}</b>
            <span class="small">${folderOk ? tr("Vi birate mapu (npr. šifrirani disk ili mapa koju sigurnosno kopirate). Ista struktura kao instalirani program SterOidSoundBoard.", "You choose the folder (e.g. an encrypted disk or a folder you back up). Same layout as the installed SterOidSoundBoard program.")
              : tr("Vaš preglednik ne podržava odabir mape — koristite Chrome ili Edge na računalu.", "Your browser cannot use a folder — use Chrome or Edge on a computer.")}</span></button>
          <button class="ssb-opt" data-k="browser">
            <b><span data-ic="layers"></span>${tr("U ovom pregledniku", "In this browser")}</b>
            <span class="small">${tr("Podaci ostaju u pregledniku na ovom uređaju. Brisanje podataka preglednika briše i njih — redovito radite sigurnosnu kopiju (Podaci → Sigurnosna kopija).", "Data stays in the browser on this device. Clearing browser data deletes it too — make regular backups (Data → Backup).")}</span></button>
        </div>
        <div class="row ssb-foot">
          <button class="ssb-install ${installEvt && !installed() ? "" : "hidden"}" data-k="install"><span data-ic="download"></span>${tr("Instaliraj aplikaciju", "Install the app")}</button>
          <span class="grow"></span>
          <select data-noi18n aria-label="Jezik / Language"><option value="hr">HR</option><option value="en">EN</option></select>
        </div>
        <p class="err small" data-k="err"></p>
      </div>`;
      document.body.append(ov);
      if (window.Icons) Icons.apply(ov);
      const sel = ov.querySelector("select"); sel.value = I18N.lang; sel.onchange = () => I18N.set(sel.value);
      const err = ov.querySelector("[data-k=err]");
      const finish = (b) => { ov.remove(); done(b); };
      ov.addEventListener("click", async (e) => {
        const k = e.target.closest("[data-k]")?.dataset.k;
        try {
          if (k === "again") finish(await SsbStore.regrant(state.needsPermission));
          else if (k === "folder") finish(await SsbStore.chooseFolder());
          else if (k === "browser") finish(await SsbStore.useBrowser());
          else if (k === "install") install();
        } catch (er) { if (er.name !== "AbortError") err.textContent = er.message; }
      });
    });
  }

  /** "Data" panel in the header: where the data is, backup/restore, move, install. */
  function dataPanel() {
    const hdr = document.querySelector("header");
    if (!hdr || document.getElementById("btnData")) return;
    const btn = el("button", null, `<span data-ic="archive"></span>${tr("Podaci", "Data")}`);
    btn.id = "btnData"; btn.title = tr("Gdje su podaci, sigurnosna kopija, instalacija", "Where the data is, backup, install");
    hdr.insertBefore(btn, document.getElementById("langSel"));
    const panel = el("section", "panel hidden ssb-data");
    panel.id = "dataPanel";
    document.querySelector("header").after(panel);
    const render = async () => {
      const b = SsbServer.store;
      const where = b.kind === "folder" ? `${tr("mapa na računalu", "folder on this computer")}: <b>${esc(b.label)}</b>` : `<b>${tr("u ovom pregledniku", "in this browser")}</b>`;
      let usage = "";
      if (b.kind === "browser" && b.usage) { const u = await b.usage(); if (u) usage = ` · ${(u / 1e6).toFixed(1)} MB`; }
      panel.innerHTML = `<div class="ssb-data-in">
        <div><span class="dim">${tr("Podaci se spremaju", "Data is stored")}:</span> ${where}${usage}</div>
        <div class="row wrap">
          <button data-k="backup"><span data-ic="download"></span>${tr("Sigurnosna kopija (.zip)", "Backup (.zip)")}</button>
          <button data-k="restore"><span data-ic="upload"></span>${tr("Vrati iz kopije…", "Restore from backup…")}</button>
          <button data-k="folder" ${SsbStore.folderSupported() ? "" : "disabled"}><span data-ic="save"></span>${tr("Premjesti u mapu na računalu…", "Move to a folder on this computer…")}</button>
          ${b.kind === "folder" ? `<button data-k="browser"><span data-ic="layers"></span>${tr("Premjesti u preglednik", "Move into the browser")}</button>` : ""}
          <button class="ssb-install ${installEvt && !installed() ? "" : "hidden"}" data-k="install"><span data-ic="download"></span>${tr("Instaliraj aplikaciju", "Install the app")}</button>
        </div>
        <p class="small dim">${PRIVACY()}</p>
        <input type="file" accept=".zip,application/zip" class="hidden" data-k="file">
      </div>`;
      Icons.apply(panel);
    };
    btn.onclick = async () => { panel.classList.toggle("hidden"); if (!panel.classList.contains("hidden")) await render(); };
    panel.addEventListener("click", async (e) => {
      const k = e.target.closest("[data-k]")?.dataset.k;
      try {
        if (k === "backup") {
          await SsbServer.saveNow();
          const blob = await SsbStore.zip(SsbServer.store), a = el("a");
          a.href = URL.createObjectURL(blob); a.download = `SterOidSoundBoard_${new Date().toISOString().slice(0, 10)}.zip`; a.click();
          setTimeout(() => URL.revokeObjectURL(a.href), 5000);
        } else if (k === "restore") panel.querySelector("[data-k=file]").click();
        else if (k === "folder" || k === "browser") {
          if (!confirm(tr("Kopirati sve podatke na novo mjesto i nastaviti raditi ondje? Stari podaci ostaju gdje jesu dok ih sami ne obrišete.", "Copy all data to the new place and keep working there? The old data stays where it is until you delete it yourself."))) return;
          const from = SsbServer.store;
          await SsbServer.saveNow();
          const to = k === "folder" ? await SsbStore.chooseFolder() : await SsbStore.useBrowser();
          const n = await SsbStore.copyAll(from, to);
          (window.toastInfo || toast)(tr(`Kopirano datoteka: ${n}. Aplikacija se ponovno učitava…`, `Files copied: ${n}. Reloading the app…`));
          setTimeout(() => location.reload(), 900);
        } else if (k === "install") install();
      } catch (er) { if (er.name !== "AbortError") toast(er.message); }
    });
    panel.addEventListener("change", async (e) => {
      if (e.target.dataset.k !== "file" || !e.target.files[0]) return;
      try {
        const files = SsbStore.unzip(await e.target.files[0].arrayBuffer());
        if (!files.some(([n]) => n === "clinic.json")) throw new Error(tr("To nije sigurnosna kopija SterOidSoundBoarda (nema clinic.json).", "This is not a SterOidSoundBoard backup (no clinic.json)."));
        if (!confirm(tr(`Vratiti ${files.length} datoteka iz kopije? Postojeći podaci s istim nazivima bit će zamijenjeni.`, `Restore ${files.length} files from the backup? Existing data with the same names will be replaced.`))) return;
        for (const [n, b] of files) await SsbServer.store.write(n, b);
        location.reload();
      } catch (er) { toast(er.message); }
    });
  }

  // ---------------------------------------------------------------- start
  async function boot() {
    try {
      // one tab at a time writes the data (two tabs would overwrite each other's changes)
      if (navigator.locks) {
        const got = await new Promise((ok) => navigator.locks.request("ssb-data", { ifAvailable: true }, (lock) => { ok(!!lock); return lock ? new Promise(() => {}) : null; }));
        if (!got) {
          const ov = el("div", "ssb-boot", `<div class="ssb-card"><h2>${tr("Aplikacija je već otvorena", "The app is already open")}</h2><p>${tr("SterOidSoundBoard je otvoren u drugoj kartici ili prozoru. Zatvorite ga ondje pa ponovno učitajte ovu stranicu — tako se podaci ne mogu međusobno prebrisati.", "SterOidSoundBoard is open in another tab or window. Close it there and reload this page — this way the data cannot be overwritten.")}</p><button class="pri">${tr("Ponovno učitaj", "Reload")}</button></div>`);
          document.body.append(ov);
          ov.querySelector("button").onclick = () => location.reload();
          throw new Error("locked");
        }
      }
      const engine = await SsbEngine.load(fetch("steroid_web.wasm"));
      let { backend, needsPermission } = await SsbStore.configured();
      if (!backend) backend = await chooser({ needsPermission });
      await SsbServer.init(engine, backend);
      readyOk();
      dataPanel();
    } catch (e) {
      readyFail(e);
      if (e.message !== "locked") { console.error(e); alert(tr("Aplikacija se nije mogla pokrenuti: ", "The app could not start: ") + e.message); }
    }
  }
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", boot); else boot();
})();
