"use strict";
const $ = (s) => document.querySelector(s);
const boardEl = $("#board"), cablesEl = $("#cables");
let TYPES = {}, BOARD = { nodes: [], connections: [] }, ME = 0, ws = null, devices = [];

// ---------------------------------------------------------------- helpers
async function api(method, path, body) {
  const r = await fetch(path, { method, headers: { "Content-Type": "application/json" }, body: body ? JSON.stringify(body) : undefined });
  if (!r.ok) { let m = r.statusText; try { m = (await r.json()).error || m; } catch (_) {} throw new Error(m); }
  return r.status === 204 ? null : r.json();
}
function toast(msg) { const t = $("#toast"); t.textContent = msg; t.classList.add("show"); clearTimeout(t._h); t._h = setTimeout(() => t.classList.remove("show"), 3000); }
function send(o) { if (ws && ws.readyState === 1) ws.send(JSON.stringify(o)); }
function el(tag, cls, html) { const e = document.createElement(tag); if (cls) e.className = cls; if (html != null) e.innerHTML = html; return e; }

const toSlider = (s, v) => s.log ? Math.log(v / s.min) / Math.log(s.max / s.min) * 1000 : (v - s.min) / (s.max - s.min) * 1000;
const fromSlider = (s, x) => { const t = x / 1000; return s.log ? s.min * Math.pow(s.max / s.min, t) : s.min + t * (s.max - s.min); };
function fmt(s, v) {
  if (s.options.length) return s.options[Math.round(v)] || v;
  if (s.unit === "Hz" && v >= 1000) return (v / 1000).toFixed(2) + " kHz";
  const d = Math.abs(v) < 10 ? 2 : Math.abs(v) < 100 ? 1 : 0;
  return v.toFixed(d) + (s.unit ? " " + s.unit : "");
}

// ---------------------------------------------------------------- audio panel
$("#btnAudio").onclick = async () => { $("#audioPanel").classList.toggle("hidden"); if (!devices.length) await loadDevices(); };
async function loadDevices() {
  devices = await api("GET", "/api/devices");
  const st = await api("GET", "/api/status");
  const cfg = st.config || {};
  const hs = $("#selHost"); hs.innerHTML = "";
  devices.forEach((h) => hs.add(new Option(h.host, h.host, false, h.host === cfg.host)));
  hs.onchange = () => fillDevs(cfg); fillDevs(cfg);
  $("#selBuf").value = cfg.buffer ? String(cfg.buffer) : "";
  $("#chkNoIn").checked = !!cfg.no_input;
}
function fillDevs(cfg) {
  const h = devices.find((d) => d.host === $("#selHost").value) || devices[0]; if (!h) return;
  const fill = (sel, list, cur, def) => { sel.innerHTML = ""; sel.add(new Option(`default (${def || "none"})`, "")); list.forEach((n) => sel.add(new Option(n, n, false, n === cur))); };
  fill($("#selIn"), h.inputs, cfg.input, h.default_input);
  fill($("#selOut"), h.outputs, cfg.output, h.default_output);
}
$("#btnStart").onclick = async () => {
  $("#audioErr").textContent = "";
  const cfg = { host: $("#selHost").value || null, input: $("#selIn").value || null, output: $("#selOut").value || null,
    buffer: $("#selBuf").value ? +$("#selBuf").value : null, no_input: $("#chkNoIn").checked };
  try { showRunning(await api("POST", "/api/audio/start", cfg)); } catch (e) { $("#audioErr").textContent = e.message; }
};
$("#btnStop").onclick = () => api("POST", "/api/audio/stop");
function showRunning(r, err) {
  $("#status").textContent = r ? `${r.host} · ${r.input || "no input"} → ${r.output} · ${r.sample_rate} Hz${r.buffer ? " · " + r.buffer + " fr" : ""}` : "audio stopped" + (err ? " — " + err : "");
  $("#status").style.color = r ? "" : "var(--bad)";
}

// ---------------------------------------------------------------- palette
function buildPalette() {
  const p = $("#palette"); let cat = null;
  Object.values(TYPES).sort((a, b) => a.category.localeCompare(b.category)).forEach((t) => {
    if (t.category !== cat) { cat = t.category; p.append(el("div", "cat", cat)); }
    const b = el("button", null, t.name);
    b.onclick = async () => {
      const n = BOARD.nodes.length;
      try { await api("POST", "/api/nodes", { kind: t.kind, x: 160 + (n % 3) * 240, y: 30 + Math.floor(n / 3) * 260 }); } catch (e) { toast(e.message); }
    };
    p.append(b);
  });
}

// ---------------------------------------------------------------- board render
function render() {
  boardEl.querySelectorAll(".card").forEach((c) => c.remove());
  let maxX = 700;
  BOARD.nodes.forEach((n) => { boardEl.append(card(n)); maxX = Math.max(maxX, n.x + 230); });
  $("#io-out").style.left = maxX + 40 + "px";
  drawCables();
}

function card(n) {
  const info = TYPES[n.kind];
  const c = el("div", "card" + (n.bypass ? " byp" : "")); c.id = "c-" + n.id;
  c.style.left = n.x + "px"; c.style.top = n.y + "px";
  const hd = el("div", "hd");
  hd.append(el("span", "t", info.name));
  const byp = el("button", n.bypass ? "" : "on", "ON");
  byp.onclick = (e) => { e.stopPropagation(); n.bypass = !n.bypass; send({ t: "bypass", node: n.id, on: n.bypass }); applyBypass(n); };
  const del = el("button", null, "×");
  del.onclick = async (e) => { e.stopPropagation(); try { await api("DELETE", "/api/nodes/" + n.id); } catch (er) { toast(er.message); } };
  hd.append(byp, del);
  dragCard(hd, c, n);
  const bd = el("div", "bd");
  bd.append(el("div", "nm", "<i></i>"));
  info.params.forEach((s) => {
    const v = n.params[s.id] ?? s.default;
    const row = el("div", "prm"); row.append(el("span", null, s.name));
    const val = el("span", "v", fmt(s, v)); row.append(val);
    let input;
    if (s.options.length) {
      input = el("select"); s.options.forEach((o, i) => input.add(new Option(o, i, false, i === Math.round(v))));
      input.onchange = () => { const x = +input.value; n.params[s.id] = x; val.textContent = fmt(s, x); send({ t: "param", node: n.id, param: s.id, value: x }); };
      input.style.gridColumn = "1/3";
    } else {
      input = el("input"); input.type = "range"; input.min = 0; input.max = 1000; input.value = toSlider(s, v);
      let raf = 0;
      input.oninput = () => {
        const x = fromSlider(s, +input.value); n.params[s.id] = x; val.textContent = fmt(s, x);
        if (!raf) raf = requestAnimationFrame(() => { raf = 0; send({ t: "param", node: n.id, param: s.id, value: n.params[s.id] }); });
      };
      input.ondblclick = () => { input.value = toSlider(s, s.default); input.oninput(); };
    }
    input.dataset.param = s.id; row.append(input); bd.append(row);
  });
  const pin = el("b", "port in"); pin.dataset.node = n.id;
  const pout = el("b", "port out"); pout.dataset.node = n.id;
  c.append(hd, bd, pin, pout);
  return c;
}
function applyBypass(n) { const c = $("#c-" + n.id); if (!c) return; c.classList.toggle("byp", n.bypass); const b = c.querySelector(".hd button"); b.className = n.bypass ? "" : "on"; }

function dragCard(handle, c, n) {
  handle.addEventListener("pointerdown", (e) => {
    if (e.target.tagName === "BUTTON") return;
    handle.setPointerCapture(e.pointerId);
    const sx = e.clientX - n.x, sy = e.clientY - n.y; let last = 0;
    const move = (ev) => {
      n.x = Math.max(0, ev.clientX - sx); n.y = Math.max(0, ev.clientY - sy);
      c.style.left = n.x + "px"; c.style.top = n.y + "px"; drawCables();
      if (ev.timeStamp - last > 60) { last = ev.timeStamp; send({ t: "move", node: n.id, x: n.x, y: n.y }); }
    };
    const up = () => { handle.removeEventListener("pointermove", move); send({ t: "move", node: n.id, x: n.x, y: n.y }); };
    handle.addEventListener("pointermove", move);
    handle.addEventListener("pointerup", up, { once: true });
  });
}

// ---------------------------------------------------------------- cables
function portPos(node, dir) {
  const p = boardEl.querySelector(`.port.${dir}[data-node="${node}"]`); if (!p) return null;
  const r = p.getBoundingClientRect(), b = boardEl.getBoundingClientRect();
  return { x: r.left - b.left + boardEl.scrollLeft + r.width / 2, y: r.top - b.top + boardEl.scrollTop + r.height / 2 };
}
function curve(a, b) { const dx = Math.max(40, Math.abs(b.x - a.x) / 2); return `M${a.x},${a.y} C${a.x + dx},${a.y} ${b.x - dx},${b.y} ${b.x},${b.y}`; }
function drawCables() {
  cablesEl.innerHTML = "";
  BOARD.connections.forEach((c) => {
    const a = portPos(c.from, "out"), b = portPos(c.to, "in"); if (!a || !b) return;
    const p = document.createElementNS("http://www.w3.org/2000/svg", "path");
    p.setAttribute("d", curve(a, b));
    p.addEventListener("click", async () => { try { await api("DELETE", "/api/connections", c); } catch (e) { toast(e.message); } });
    cablesEl.append(p);
  });
}
boardEl.addEventListener("pointerdown", (e) => {
  const port = e.target.closest(".port.out"); if (!port) return;
  e.preventDefault();
  const from = port.dataset.node, a = portPos(from, "out");
  const tmp = document.createElementNS("http://www.w3.org/2000/svg", "path"); tmp.classList.add("drag"); cablesEl.append(tmp);
  const b0 = boardEl.getBoundingClientRect();
  const move = (ev) => tmp.setAttribute("d", curve(a, { x: ev.clientX - b0.left + boardEl.scrollLeft, y: ev.clientY - b0.top + boardEl.scrollTop }));
  const up = async (ev) => {
    window.removeEventListener("pointermove", move); tmp.remove();
    const t = document.elementFromPoint(ev.clientX, ev.clientY);
    const target = t && t.closest(".port.in");
    if (target) { try { await api("POST", "/api/connections", { from, to: target.dataset.node }); } catch (er) { toast(er.message); } }
  };
  window.addEventListener("pointermove", move);
  window.addEventListener("pointerup", up, { once: true });
});
boardEl.addEventListener("scroll", drawCables);
window.addEventListener("resize", drawCables);

// ---------------------------------------------------------------- live updates
const setMeter = (id, v) => { const e = $(id); if (e) e.style.height = Math.min(100, Math.max(0, (20 * Math.log10(v + 1e-9) + 60) / 60 * 100)) + "%"; };
function onMsg(m) {
  switch (m.t) {
    case "hello": ME = m.client; break;
    case "board": BOARD = m.board; render(); break;
    case "audio": showRunning(m.running, m.error); break;
    case "param": {
      if (m.src === ME) break;
      const n = BOARD.nodes.find((x) => x.id === m.node); if (!n) break;
      n.params[m.param] = m.value;
      const inp = document.querySelector(`#c-${m.node} [data-param="${m.param}"]`); if (!inp) break;
      const s = TYPES[n.kind].params.find((p) => p.id === m.param);
      if (inp.tagName === "SELECT") inp.value = Math.round(m.value); else inp.value = toSlider(s, m.value);
      inp.parentElement.querySelector(".v").textContent = fmt(s, m.value);
      break;
    }
    case "bypass": { if (m.src === ME) break; const n = BOARD.nodes.find((x) => x.id === m.node); if (n) { n.bypass = m.on; applyBypass(n); } break; }
    case "move": {
      if (m.src === ME) break; const n = BOARD.nodes.find((x) => x.id === m.node); const c = $("#c-" + m.node);
      if (n && c) { n.x = m.x; n.y = m.y; c.style.left = m.x + "px"; c.style.top = m.y + "px"; drawCables(); }
      break;
    }
    case "meters":
      setMeter("#mInL", m.in[0]); setMeter("#mInR", m.in[1]); setMeter("#mOutL", m.out[0]); setMeter("#mOutR", m.out[1]);
      $("#load").textContent = Math.round(m.load * 100) + "%"; $("#xruns").textContent = m.xruns;
      for (const [id, v] of Object.entries(m.nodes)) {
        const i = document.querySelector(`#c-${id} .nm i`);
        if (i) i.style.width = Math.min(100, Math.max(0, (20 * Math.log10(v + 1e-9) + 60) / 60 * 100)) + "%";
      }
      break;
  }
}
function connectWs() {
  ws = new WebSocket((location.protocol === "https:" ? "wss://" : "ws://") + location.host + "/ws");
  ws.onmessage = (e) => onMsg(JSON.parse(e.data));
  ws.onopen = async () => { BOARD = await api("GET", "/api/board"); render(); };
  ws.onclose = () => { $("#status").textContent = "disconnected — reconnecting…"; setTimeout(connectWs, 1500); };
}

// ---------------------------------------------------------------- boot
(async () => {
  (await api("GET", "/api/node-types")).forEach((t) => (TYPES[t.kind] = t));
  const st = await api("GET", "/api/status");
  $("#ver").textContent = "v" + st.version;
  showRunning(st.running, st.error);
  buildPalette();
  connectWs();
})();
