"use strict";
// Login, users, audit log and backups of the desktop app (the browser edition
// keeps everything on the device and has no login; its Data panel does backups).
// Loaded before app.js: the app starts once AUTH_READY resolves.
const Auth = (() => {
  const tr = (hr, en) => (I18N.lang === "en" ? en : hr);
  const q = (s, r = document) => r.querySelector(s);
  const esc = (s) => String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
  const mk = (tag, cls, html) => { const e = document.createElement(tag); if (cls) e.className = cls; if (html != null) e.innerHTML = html; return e; };
  const ic = (e) => { if (window.Icons) Icons.apply(e); return e; };
  async function call(method, path, body, raw) {
    const r = await fetch(path, { method, headers: raw ? { "x-lang": I18N.lang } : { "Content-Type": "application/json", "x-lang": I18N.lang }, body: raw ? body : body ? JSON.stringify(body) : undefined });
    let j = null;
    try { j = await r.json(); } catch (_) {}
    if (!r.ok) throw new Error((j && j.error) || r.statusText);
    return j;
  }
  const toast2 = (m) => (typeof toast === "function" ? toast(m) : alert(m));
  const roleName = (r) => (r === "admin" ? tr("administrator", "administrator") : tr("kliničar", "clinician"));
  let ME = null;

  // ---------------------------------------------------------------- login screen
  function showLogin(msg) {
    if (q(".au-screen")) return;
    const ov = mk("div", "au-screen", `
      <form class="au-card" autocomplete="on">
        <div class="au-logo"><span data-ic="waves"></span>SterOidSoundBoard</div>
        <h2><span data-ic="lock"></span>${tr("Prijava", "Sign in")}</h2>
        <p class="dim small">${tr("Podaci pacijenata zaštićeni su prijavom. Upišite korisničko ime i lozinku ili PIN.", "Patient data is protected by a login. Enter your user name and password or PIN.")}</p>
        <label>${tr("Korisničko ime", "User name")}<input name="name" autocomplete="username" autocapitalize="none" required></label>
        <label>${tr("Lozinka ili PIN", "Password or PIN")}<input name="secret" type="password" autocomplete="current-password" required></label>
        <div class="au-err err">${esc(msg || "")}</div>
        <button class="pri wide" type="submit">${tr("Prijavi se", "Sign in")}</button>
      </form>`);
    document.body.append(ov);
    ic(ov);
    const f = q("form", ov);
    q("input[name=name]", ov).focus();
    f.onsubmit = async (e) => {
      e.preventDefault();
      const b = q("button", f);
      b.disabled = true;
      try {
        await call("POST", "/api/auth/login", { name: f.name.value, secret: f.secret.value });
        location.reload();
      } catch (er) {
        q(".au-err", ov).textContent = er.message;
        f.secret.value = "";
        f.secret.focus();
      } finally { b.disabled = false; }
    };
  }

  // ---------------------------------------------------------------- header button + panel
  function headerButton() {
    const b = mk("button", "", "");
    b.id = "btnUser";
    const lang = q("#langSel");
    lang.parentNode.insertBefore(b, lang);
    const panel = mk("section", "panel hidden au-panel", "");
    panel.id = "userPanel";
    const hdr = q("header");
    hdr.parentNode.insertBefore(panel, hdr.nextSibling);
    b.onclick = () => { panel.classList.toggle("hidden"); if (!panel.classList.contains("hidden")) renderPanel(panel); };
    paintButton(b);
  }
  function paintButton(b) {
    if (ME.user) {
      b.innerHTML = `<span data-ic="user"></span>${esc(ME.user.display || ME.user.name)}`;
      b.title = tr("Prijavljeni korisnik, odjava, korisnici, dnevnik, sigurnosna kopija", "Signed-in user, sign out, users, log, backup");
      b.classList.remove("au-attn");
    } else if (ME.can_setup) {
      b.innerHTML = `<span data-ic="shield"></span>${tr("Prijava", "Sign-in")}`;
      b.title = tr("Prijava još nije uključena: na mreži su dostupni samo zvuk i pedalboard. Uključite je ovdje.", "Sign-in is not switched on yet: only audio and the pedalboard are reachable on the network. Switch it on here.");
      b.classList.add("au-attn");
    } else {
      b.innerHTML = `<span data-ic="lock"></span>${tr("Ograničeno", "Limited")}`;
      b.title = tr("Podaci pacijenata dostupni su samo na računalu na kojem aplikacija radi.", "Patient data is only available on the computer the app runs on.");
    }
    ic(b);
  }

  function renderPanel(p) {
    const parts = [];
    if (ME.user) {
      parts.push(`<div class="au-box"><h3><span data-ic="user"></span>${esc(ME.user.display || ME.user.name)} <span class="tag">${esc(roleName(ME.user.role))}</span></h3>
        <div class="row"><button data-k="pw"><span data-ic="lock"></span>${tr("Promijeni lozinku / PIN", "Change password / PIN")}</button>
        <button data-k="logout"><span data-ic="log-out"></span>${tr("Odjava", "Sign out")}</button></div></div>`);
    } else if (ME.can_setup) {
      parts.push(`<div class="au-box au-setup"><h3><span data-ic="shield"></span>${tr("Uključi prijavu", "Switch on sign-in")}</h3>
        <p class="small">${tr("Dok nema korisnika, podaci pacijenata dostupni su samo na ovom računalu, a drugi uređaji na mreži vide samo zvuk i pedalboard. Napravite prvog korisnika (administratora) i prijava vrijedi za sve uređaje, i za ovo računalo.", "Until there is a user, patient data is only available on this computer; other devices on the network see only audio and the pedalboard. Create the first user (an administrator) and sign-in applies to every device, this computer included.")}</p>
        <form class="au-form" data-k="setup">
          <label>${tr("Korisničko ime", "User name")}<input name="name" autocomplete="username" autocapitalize="none" required placeholder="npr. ana"></label>
          <label>${tr("Ime za prikaz", "Display name")}<input name="display" placeholder="${tr("npr. Ana Anić, logoped", "e.g. Ana Anić, SLP")}"></label>
          <label>${tr("Lozinka ili PIN (min. 4)", "Password or PIN (min. 4)")}<input name="secret" type="password" autocomplete="new-password" required></label>
          <label>${tr("Ponovite", "Repeat")}<input name="secret2" type="password" autocomplete="new-password" required></label>
          <button class="pri" type="submit"><span data-ic="shield"></span>${tr("Napravi korisnika i uključi prijavu", "Create the user and switch on sign-in")}</button>
        </form></div>`);
    } else {
      parts.push(`<div class="au-box"><p>${tr("Podaci pacijenata dostupni su samo na računalu na kojem aplikacija radi, dok se tamo ne uključi prijava.", "Patient data is only available on the computer the app runs on until sign-in is switched on there.")}</p></div>`);
    }
    if (ME.admin) {
      parts.push(`<div class="au-box"><h3><span data-ic="users"></span>${tr("Korisnici", "Users")}</h3><div class="au-users dim small">…</div>
        ${ME.auth ? `<form class="au-form au-inline" data-k="add">
          <input name="name" placeholder="${tr("korisničko ime", "user name")}" autocapitalize="none" required>
          <input name="display" placeholder="${tr("ime za prikaz", "display name")}">
          <select name="role"><option value="clinician">${tr("kliničar", "clinician")}</option><option value="admin">${tr("administrator", "administrator")}</option></select>
          <input name="secret" type="password" placeholder="${tr("lozinka / PIN", "password / PIN")}" autocomplete="new-password" required>
          <button type="submit"><span data-ic="plus"></span>${tr("Dodaj", "Add")}</button></form>` : ""}</div>
        <div class="au-box"><h3><span data-ic="archive"></span>${tr("Sigurnosna kopija", "Backup")}</h3>
          <p class="small dim">${tr("Kartoni, sesije, snimke, preseti i dnevnik u jednoj .zip datoteci, isti format kao u pregledničkoj verziji. Lozinke, AI ključ i MCP token nisu u kopiji. Prije vraćanja aplikacija sama spremi trenutne podatke u mapu backups.", "Charts, sessions, recordings, presets and the log in one .zip file, the same format as the browser edition. Passwords, the AI key and the MCP token are not included. Before a restore the app saves the current data to the backups folder.")}</p>
          <div class="row"><button data-k="backup"><span data-ic="download"></span>${tr("Preuzmi kopiju (.zip)", "Download a backup (.zip)")}</button>
          <button data-k="restore"><span data-ic="upload"></span>${tr("Vrati iz kopije…", "Restore from a backup…")}</button>
          <input type="file" accept=".zip,application/zip" class="hidden" data-k="file"></div></div>
        <div class="au-box au-wide"><h3><span data-ic="list"></span>${tr("Dnevnik promjena", "Change log")}</h3>
          <p class="small dim">${tr("Tko je i kada mijenjao podatke pacijenata, prijave i odjave. Bez imena pacijenata i sadržaja.", "Who changed patient data and when, sign-ins and sign-outs. Without patient names or content.")}</p>
          <div class="au-audit"><button data-k="audit"><span data-ic="eye"></span>${tr("Prikaži zadnjih 300", "Show the latest 300")}</button></div></div>`);
    }
    p.innerHTML = parts.join("");
    ic(p);
    if (ME.admin) loadUsers(p);
  }

  async function loadUsers(p) {
    const box = q(".au-users", p);
    try {
      const list = await call("GET", "/api/users");
      if (!list.length) { box.textContent = tr("Još nema korisnika.", "No users yet."); return; }
      box.classList.remove("dim");
      box.innerHTML = `<table class="stable small"><thead><tr><th>${tr("Korisnik", "User")}</th><th>${tr("Ime", "Name")}</th><th>${tr("Uloga", "Role")}</th><th></th></tr></thead><tbody>${list.map((u) => `
        <tr data-u="${esc(u.name)}" data-role="${esc(u.role)}" data-off="${u.disabled ? 1 : ""}" class="${u.disabled ? "dim" : ""}"><td class="mono">${esc(u.name)}</td><td>${esc(u.display)}</td><td>${esc(roleName(u.role))}${u.disabled ? " · " + tr("isključen", "disabled") : ""}</td>
        <td class="row"><button class="small" data-k="role">${u.role === "admin" ? tr("Učini kliničarem", "Make clinician") : tr("Učini administratorom", "Make administrator")}</button>
        <button class="small" data-k="reset">${tr("Nova lozinka", "New password")}</button>
        <button class="small" data-k="toggle">${u.disabled ? tr("Uključi", "Enable") : tr("Isključi", "Disable")}</button>
        ${ME.user && ME.user.name === u.name ? "" : `<button class="small danger" data-k="del" title="${tr("Obriši korisnika", "Delete the user")}"><span data-ic="trash"></span></button>`}</td></tr>`).join("")}</tbody></table>`;
      ic(box);
    } catch (e) { box.textContent = e.message; }
  }

  const ACT = {
    login: ["Prijava", "Sign-in"], login_failed: ["Neuspjela prijava", "Failed sign-in"], logout: ["Odjava", "Sign-out"],
    POST: ["Dodavanje", "Added"], PUT: ["Izmjena", "Changed"], DELETE: ["Brisanje", "Deleted"],
    backup: ["Sigurnosna kopija", "Backup"], restore: ["Vraćanje iz kopije", "Restore"],
    user_create: ["Novi korisnik", "New user"], user_update: ["Izmjena korisnika", "User changed"], user_delete: ["Brisanje korisnika", "User deleted"], password_change: ["Promjena lozinke", "Password changed"],
  };
  const WHAT = [
    [/^\/api\/patients\/[^/]+\/notes$/, "bilješka", "note"], [/^\/api\/patients\/[^/]+\/questionnaires$/, "upitnik", "questionnaire"], [/^\/api\/patients/, "pacijent", "patient"],
    [/^\/api\/sessions/, "sesija", "session"], [/^\/api\/record\//, "snimanje", "recording"], [/^\/api\/recordings\/[^/]+\/ai/, "AI mišljenje", "AI opinion"], [/^\/api\/recordings\/[^/]+\/analyses/, "spremljena analiza", "saved analysis"],
    [/^\/api\/recordings/, "snimka", "recording"], [/^\/api\/notes/, "bilješka", "note"], [/^\/api\/questionnaires/, "upitnik", "questionnaire"], [/^\/api\/ai-reports/, "AI mišljenje", "AI opinion"],
    [/^\/api\/clinic\/presets/, "klinički preset", "clinical preset"], [/^\/api\/clinic\/settings/, "postavke klinike", "clinic settings"], [/^\/api\/ai/, "AI postavke", "AI settings"], [/^\/api\/mcp/, "MCP postavke", "MCP settings"],
  ];
  async function loadAudit(p) {
    const box = q(".au-audit", p);
    try {
      const rows = await call("GET", "/api/audit?limit=300");
      if (!rows.length) { box.textContent = tr("Dnevnik je prazan.", "The log is empty."); return; }
      const what = (t) => { const w = WHAT.find(([re]) => re.test(t || "")); return w ? tr(w[1], w[2]) : t; };
      box.innerHTML = `<div class="au-scroll"><table class="stable small"><thead><tr><th>${tr("Vrijeme", "Time")}</th><th>${tr("Korisnik", "User")}</th><th>${tr("Adresa", "Address")}</th><th>${tr("Radnja", "Action")}</th><th>${tr("Što", "What")}</th></tr></thead><tbody>${rows.map((r) => {
        const a = ACT[r.action];
        const t = r.target && r.target.startsWith("/api/") ? `${esc(what(r.target))} <span class="dim mono">${esc(r.target.split("/").slice(3).join("/"))}</span>` : esc(r.target);
        return `<tr><td class="mono">${esc(new Date(r.at * 1000).toLocaleString(I18N.lang === "en" ? "en-GB" : "hr-HR"))}</td><td>${esc(r.user)}</td><td class="mono">${esc(r.ip)}</td><td>${a ? esc(tr(a[0], a[1])) : esc(r.action)}</td><td>${t}</td></tr>`;
      }).join("")}</tbody></table></div>`;
    } catch (e) { box.textContent = e.message; }
  }

  function wire() {
    const p = q("#userPanel");
    p.addEventListener("click", async (e) => {
      const b = e.target.closest("[data-k]");
      if (!b) return;
      const k = b.dataset.k, row = b.closest("[data-u]"), name = row && row.dataset.u;
      try {
        if (k === "logout") { await call("POST", "/api/auth/logout"); location.reload(); }
        else if (k === "pw") {
          const old = prompt(tr("Trenutna lozinka / PIN:", "Current password / PIN:")); if (old == null) return;
          const nw = prompt(tr("Nova lozinka / PIN (min. 4 znaka):", "New password / PIN (min. 4 characters):")); if (nw == null) return;
          if (prompt(tr("Ponovite novu lozinku / PIN:", "Repeat the new password / PIN:")) !== nw) return toast2(tr("Lozinke se ne podudaraju.", "The passwords do not match."));
          await call("PUT", "/api/auth/password", { old, secret: nw }); toast2(tr("Lozinka promijenjena. Ostali uređaji su odjavljeni.", "Password changed. Other devices were signed out."));
        } else if (k === "audit") loadAudit(p);
        else if (k === "backup") location.href = "/api/backup?lang=" + I18N.lang;
        else if (k === "restore") q("[data-k=file]", p).click();
        else if (k === "role") { await call("PUT", `/api/users/${encodeURIComponent(name)}`, { role: row.dataset.role === "admin" ? "clinician" : "admin" }); loadUsers(p); }
        else if (k === "reset") {
          const nw = prompt(tr(`Nova lozinka / PIN za ${name}:`, `New password / PIN for ${name}:`)); if (!nw) return;
          await call("PUT", `/api/users/${encodeURIComponent(name)}`, { secret: nw }); toast2(tr("Lozinka postavljena; korisnik je odjavljen sa svih uređaja.", "Password set; the user was signed out everywhere."));
        } else if (k === "toggle") { await call("PUT", `/api/users/${encodeURIComponent(name)}`, { disabled: !row.dataset.off }); loadUsers(p); }
        else if (k === "del") { if (!confirm(tr(`Obrisati korisnika ${name}? Njegovi zapisi u dnevniku ostaju.`, `Delete the user ${name}? Their log entries stay.`))) return; await call("DELETE", `/api/users/${encodeURIComponent(name)}`); loadUsers(p); }
      } catch (er) { toast2(er.message); }
    });
    p.addEventListener("submit", async (e) => {
      e.preventDefault();
      const f = e.target, k = f.dataset.k;
      try {
        if (k === "setup") {
          if (f.secret.value !== f.secret2.value) return toast2(tr("Lozinke se ne podudaraju.", "The passwords do not match."));
          await call("POST", "/api/auth/setup", { name: f.name.value, display: f.display.value, secret: f.secret.value });
          location.reload();
        } else if (k === "add") {
          await call("POST", "/api/users", { name: f.name.value, display: f.display.value, role: f.role.value, secret: f.secret.value });
          f.reset(); loadUsers(p); toast2(tr("Korisnik dodan.", "User added."));
        }
      } catch (er) { toast2(er.message); }
    });
    p.addEventListener("change", async (e) => {
      if (e.target.dataset.k !== "file" || !e.target.files[0]) return;
      const file = e.target.files[0];
      e.target.value = "";
      if (!confirm(tr(`Vratiti podatke iz „${file.name}”? Kartoni, sesije i snimke s istim nazivima bit će zamijenjeni. Trenutni podaci prije toga se spremaju u mapu backups.`, `Restore the data from "${file.name}"? Charts, sessions and recordings with the same names will be replaced. The current data is saved to the backups folder first.`))) return;
      try {
        const r = await call("POST", "/api/restore", file, true);
        alert(tr(`Vraćeno ${r.files} datoteka. Prethodni podaci spremljeni su kao backups/${r.safety_copy}.`, `Restored ${r.files} files. The previous data was saved as backups/${r.safety_copy}.`));
        location.reload();
      } catch (er) { toast2(er.message); }
    });
  }

  const ready = (async () => {
    try {
      const r = await fetch("/api/auth/me", { headers: { "x-lang": I18N.lang } });
      ME = r.ok ? await r.json() : null;
    } catch (_) { ME = null; }
    if (!ME || ME.edition !== "desktop") return; // browser edition: no login
    const go = () => { headerButton(); wire(); };
    if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", go); else go();
    if (ME.auth && !ME.user) {
      showLogin();
      await new Promise(() => {}); // the app starts after the sign-in reloads the page
    }
  })();

  return { ready, showLogin, me: () => ME };
})();
window.AUTH_READY = Auth.ready;
