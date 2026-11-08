"use strict";
// AI providers called straight from the browser (port of crates/app/src/ai.rs
// and ollama.rs). The API key stays in the local data store and goes only to
// the AI service the user configured; there is no server in between.
const SsbAi = (() => {
  const DEFAULT_CLAUDE_MODEL = "claude-opus-5";
  const ANTHROPIC_VERSION = "2023-06-01";
  const FALLBACK_BETA = "server-side-fallback-2026-07-01";
  const MAX_TOKENS = 16000;
  const OLLAMA_URL = "http://127.0.0.1:11434", OLLAMA_MODEL = "gemma3:4b";
  const NUM_PREDICT = 4096, MAX_CTX = 32768;
  const en = () => typeof I18N !== "undefined" && I18N.lang === "en";
  const tr = (hr, e) => (en() ? e : hr);
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const secs = (t0) => ((performance.now() - t0) / 1000).toFixed(0);

  const defaults = () => ({ provider: "anthropic", base_url: "", model: DEFAULT_CLAUDE_MODEL, api_key: "", send_image: true, ollama_ctx: 0 });
  const ollamaRoot = (u) => { let b = String(u || "").trim().replace(/\/+$/, ""); b = b.replace(/\/v1$/, "").replace(/\/+$/, ""); return b || OLLAMA_URL; };
  function base(c) {
    if (c.provider === "ollama") return ollamaRoot(c.base_url);
    const b = String(c.base_url || "").trim().replace(/\/+$/, "");
    if (b) return b;
    return c.provider === "openai" ? "https://api.openai.com/v1" : "https://api.anthropic.com";
  }
  const providerName = (c) => ({ openai: "OpenAI-kompatibilan", ollama: "Ollama (lokalno)" }[c.provider] || "Anthropic Claude");
  const pub = (c) => ({ provider: c.provider, base_url: c.base_url, effective_url: base(c), model: c.model, has_key: !!c.api_key, send_image: c.send_image,
    ollama_ctx: c.ollama_ctx, default_claude_model: DEFAULT_CLAUDE_MODEL, default_ollama_model: OLLAMA_MODEL });
  function check(c) {
    if (!["anthropic", "openai", "ollama"].includes(c.provider)) throw new Error(tr("nepoznat AI servis", "unknown AI service"));
    if (!String(c.model || "").trim()) throw new Error(tr("AI model nije postavljen (AI postavke)", "no AI model set (AI settings)"));
    if (c.provider === "anthropic" && !c.api_key) throw new Error(tr("nedostaje Anthropic API ključ (AI postavke)", "the Anthropic API key is missing (AI settings)"));
  }
  /** Data URL → [media type, base64] (validated like the server). */
  function parseImage(url) {
    const m = /^data:(image\/(?:jpeg|png));base64,(.+)$/s.exec(String(url).trim());
    if (!m) throw new Error(tr("slika mora biti JPEG ili PNG", "the image must be JPEG or PNG"));
    if (m[2].length * 0.75 > 5 << 20) throw new Error(tr("slika je prevelika", "the image is too large"));
    return [m[1], m[2].trim()];
  }

  function errorMessage(code, body) {
    let v = null; try { v = JSON.parse(body); } catch (_) {}
    const msg = String((v && ((v.error && (v.error.message || (typeof v.error === "string" && v.error))) || v.message)) || body).slice(0, 400);
    const hint = { 401: tr(" — provjerite API ključ", " — check the API key"), 403: tr(" — provjerite API ključ", " — check the API key"),
      404: tr(" — provjerite model i adresu servisa", " — check the model and the service address"), 429: tr(" — previše zahtjeva ili nema kredita", " — too many requests or no credit") }[code] || "";
    return tr(`AI servis je vratio grešku ${code}: ${msg}${hint}`, `The AI service returned error ${code}: ${msg}${hint}`);
  }
  function netError(url, e) {
    const local = /^http:\/\/(127\.0\.0\.1|localhost|\[::1\])/.test(url);
    const why = local
      ? tr(" Ako koristite Ollamu iz preglednika, pokrenite je s dopuštenjem za ovu stranicu: OLLAMA_ORIGINS=" + location.origin + " (vidi upute u AI postavkama).",
           " If you use Ollama from the browser, start it allowing this page: OLLAMA_ORIGINS=" + location.origin + " (see the AI settings help).")
      : tr(" Servis mora dopuštati pozive iz preglednika (CORS).", " The service must allow calls from a browser (CORS).");
    return new Error(tr(`nije moguće spojiti se na ${url}: ${e.message || e}.`, `cannot connect to ${url}: ${e.message || e}.`) + why);
  }

  async function post(url, headers, body, p) {
    for (let attempt = 0; ; attempt++) {
      let r;
      try { r = await fetch(url, { method: "POST", headers: { "content-type": "application/json", ...headers }, body: JSON.stringify(body), signal: p.signal }); }
      catch (e) {
        if (p.cancelled()) throw new Error(tr("prekinuto", "stopped"));
        if (attempt >= 1) throw netError(url, e);
        p.log(tr(`Veza nije uspjela (${e.message}) — ponovni pokušaj za 2 s.`, `Connection failed (${e.message}) — retrying in 2 s.`));
        await sleep(2000); continue;
      }
      if (r.ok) { try { return [0, await r.json()]; } catch (e) { throw new Error(tr(`neispravan odgovor AI servisa: ${e.message}`, `invalid answer from the AI service: ${e.message}`)); } }
      const text = await r.text().catch(() => "");
      const code = r.status;
      if (!(code === 408 || code === 429 || code >= 500) || attempt >= 2) return [code, errorMessage(code, text)];
      const d = Math.min(+r.headers.get("retry-after") || 2 << attempt, 30);
      p.log(tr(`Servis je zauzet ili preopterećen (${code}) — ponovni pokušaj za ${d} s.`, `The service is busy or overloaded (${code}) — retrying in ${d} s.`));
      if (p.cancelled()) throw new Error(tr("prekinuto", "stopped"));
      await sleep(d * 1000);
    }
  }

  function parseAnthropic(v) {
    if (v.stop_reason === "refusal") {
      const cat = (v.stop_details && v.stop_details.category) || tr("nepoznato", "unknown");
      throw new Error(tr(`AI model je odbio zahtjev (kategorija: ${cat}). Pokušajte preformulirati pitanje.`, `The AI model declined the request (category: ${cat}). Try rephrasing the question.`));
    }
    const text = (v.content || []).filter((b) => b.type === "text").map((b) => b.text);
    if (!text.length) throw new Error(tr(`AI nije vratio tekst (stop_reason: ${JSON.stringify(v.stop_reason)})`, `the AI returned no text (stop_reason: ${JSON.stringify(v.stop_reason)})`));
    return { text: text.join("\n\n"), model: v.model || "", truncated: v.stop_reason === "max_tokens" };
  }
  function parseOpenai(v) {
    const ch = (v.choices || [])[0] || {}, msg = ch.message || {};
    const text = typeof msg.content === "string" ? msg.content : Array.isArray(msg.content) ? msg.content.map((x) => x.text).filter(Boolean).join("\n") : "";
    if (!text.trim()) {
      if (msg.refusal) throw new Error(tr(`AI model je odbio zahtjev: ${msg.refusal}`, `The AI model declined the request: ${msg.refusal}`));
      throw new Error(tr("AI nije vratio tekst", "the AI returned no text"));
    }
    return { text, model: v.model || "", truncated: ch.finish_reason === "length" };
  }
  const stripThinking = (t) => t.replace(/<think>[\s\S]*?(<\/think>|$)/g, "").trim();

  /** Lines of a streamed response body (SSE or NDJSON). */
  async function* lines(resp, p) {
    const rd = resp.body.getReader(), dec = new TextDecoder();
    let buf = "";
    for (;;) {
      if (p.cancelled()) { try { rd.cancel(); } catch (_) {} throw new Error(tr("prekinuto", "stopped")); }
      const { value, done } = await rd.read();
      if (done) break;
      buf += dec.decode(value, { stream: true });
      let i;
      while ((i = buf.indexOf("\n")) >= 0) { yield buf.slice(0, i).replace(/\r$/, ""); buf = buf.slice(i + 1); }
    }
    if (buf.trim()) yield buf;
  }

  async function openaiStream(url, headers, body, p) {
    const t0 = performance.now();
    let resp;
    try { resp = await fetch(url, { method: "POST", headers: { "content-type": "application/json", accept: "text/event-stream", ...headers }, body: JSON.stringify(body), signal: p.signal }); }
    catch (e) { if (p.cancelled()) throw new Error(tr("prekinuto", "stopped")); throw netError(url, e); }
    if (!resp.ok) throw new Error(errorMessage(resp.status, await resp.text().catch(() => "")));
    if (!(resp.headers.get("content-type") || "").includes("event-stream")) {
      const a = parseOpenai(await resp.json());
      p.log(tr(`Odgovor primljen u cjelini nakon ${secs(t0)} s.`, `Answer received in one piece after ${secs(t0)} s.`));
      p.delta(a.text);
      return a;
    }
    let text = "", model = "", finish = "", n = 0, thinking = false, gen = 0, last = performance.now();
    for await (const line of lines(resp, p)) {
      if (!line.startsWith("data:")) continue;
      const data = line.slice(5).trim();
      if (data === "[DONE]") break;
      let v; try { v = JSON.parse(data); } catch (_) { continue; }
      if (v.error) throw new Error(tr("AI servis: ", "AI service: ") + (v.error.message || v.error));
      if (!model) model = v.model || "";
      const ch = (v.choices || [])[0] || {}, d = ch.delta || {};
      if ((d.reasoning_content || d.reasoning) && !thinking) { thinking = true; p.log(tr(`Model razmišlja prije odgovora (nakon ${secs(t0)} s)…`, `The model is thinking before answering (after ${secs(t0)} s)…`)); }
      if (d.content) {
        if (!gen) { gen = performance.now(); p.log(tr(`Prvi dio odgovora nakon ${secs(t0)} s — model piše…`, `First words after ${secs(t0)} s — the model is writing…`)); }
        n++; text += d.content; p.delta(d.content);
      }
      if (ch.finish_reason) finish = ch.finish_reason;
      if (performance.now() - last > 1000) {
        const g = gen ? (performance.now() - gen) / 1000 : 0;
        p.stats({ tokens: n, tps: g > 0.5 ? Math.round((n / g) * 10) / 10 : 0, elapsed: Math.floor((performance.now() - t0) / 1000) });
        last = performance.now();
      }
    }
    const g = gen ? (performance.now() - gen) / 1000 : 0;
    p.log(tr(`Gotovo za ${secs(t0)} s: ${n} dijelova odgovora${g > 0.5 ? ` (${(n / g).toFixed(1)} po s)` : ""}.`, `Done in ${secs(t0)} s: ${n} answer pieces${g > 0.5 ? ` (${(n / g).toFixed(1)} per s)` : ""}.`));
    p.stats({ tokens: n, tps: g > 0.5 ? Math.round((n / g) * 10) / 10 : 0, elapsed: Math.floor((performance.now() - t0) / 1000), done: true });
    const answer = stripThinking(text);
    if (!answer) throw new Error(tr(`AI nije vratio tekst (razlog završetka: ${finish || "?"})`, `the AI returned no text (finish reason: ${finish || "?"})`));
    return { text: answer, model, truncated: finish === "length" };
  }

  // ---------------------------------------------------------------- Ollama
  async function oget(url, ms) {
    const ac = new AbortController(), t = setTimeout(() => ac.abort(), ms);
    try {
      const r = await fetch(url, { signal: ac.signal });
      if (!r.ok) throw new Error(`Ollama ${r.status}: ${await r.text()}`);
      return await r.json();
    } finally { clearTimeout(t); }
  }
  const notRunning = (root, e) => tr(
    `Ollama nije dostupna na ${root} (${e}). Instalirajte je s ollama.com i pokrenite je tako da dopušta ovu stranicu: OLLAMA_ORIGINS=${location.origin} ollama serve`,
    `Ollama is not reachable at ${root} (${e}). Install it from ollama.com and start it allowing this page: OLLAMA_ORIGINS=${location.origin} ollama serve`);
  async function capabilities(root, model) {
    const r = await fetch(`${root}/api/show`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ model }) });
    if (r.status === 404) throw new Error(tr(`Model „${model}” nije preuzet u Ollami — preuzmite ga u AI postavkama (Preuzmi) ili naredbom \`ollama pull ${model}\`.`, `Model “${model}” is not downloaded in Ollama — download it in the AI settings (Download) or with \`ollama pull ${model}\`.`));
    if (!r.ok) throw new Error(`Ollama ${r.status}`);
    const v = await r.json();
    const caps = v.capabilities || [], fam = (v.details && v.details.families) || [];
    const vision = caps.includes("vision") || "projector_info" in v || fam.some((f) => f === "clip" || f === "mllama");
    const mi = v.model_info || {}, k = Object.keys(mi).find((x) => x.endsWith(".context_length"));
    return [vision, k ? mi[k] : null];
  }
  async function loaded(root) {
    try {
      const v = await oget(`${root}/api/ps`, 4000);
      return (v.models || []).map((m) => ({ name: m.name, size: m.size || 0, size_vram: m.size_vram || 0, gpu_pct: m.size ? Math.round(((m.size_vram || 0) / m.size) * 100) : 0, context: m.context_length, expires_at: m.expires_at }));
    } catch (_) { return []; }
  }
  async function ollamaStatus(root) {
    let version;
    try { version = (await oget(`${root}/api/version`, 4000)).version || "?"; }
    catch (e) { return { running: false, url: root, error: notRunning(root, e.message || e) }; }
    let tags = {};
    try { tags = await oget(`${root}/api/tags`, 10000); } catch (_) {}
    const models = [];
    for (const m of (tags.models || []).slice(0, 60)) {
      const [vision, ctx] = await capabilities(root, m.name).catch(() => [false, null]);
      models.push({ name: m.name, size: m.size, params: m.details && m.details.parameter_size, quant: m.details && m.details.quantization_level, family: m.details && m.details.family, vision, context: ctx });
    }
    return { running: true, url: root, version, models, loaded: await loaded(root) };
  }
  const gb = (b) => (b / 1e9).toFixed(1) + " GB";
  function describeLoad(m) {
    const ctx = m.context ? tr(`, kontekst ${m.context}`, `, context ${m.context}`) : "";
    if (m.gpu_pct >= 99.5) return tr(`Model je u memoriji: ${gb(m.size)} potpuno na GPU-u${ctx} — brzo.`, `Model loaded: ${gb(m.size)} fully on the GPU${ctx} — fast.`);
    if (m.gpu_pct <= 0.5) return tr(`Model je u memoriji: ${gb(m.size)} samo na CPU-u${ctx} — GPU se ne koristi, bit će sporo (provjerite upravljački program za NVIDIA/CUDA).`, `Model loaded: ${gb(m.size)} on the CPU only${ctx} — the GPU is not used, this will be slow (check the NVIDIA/CUDA driver).`);
    return tr(`Model je u memoriji: ${gb(m.size)} — ${m.gpu_pct} % na GPU-u, ${100 - m.gpu_pct} % na CPU-u${ctx}. Ne stane cijeli u memoriju grafičke kartice pa je sporije; manji model ili manji kontekst bio bi brži.`,
      `Model loaded: ${gb(m.size)} — ${m.gpu_pct} % on the GPU, ${100 - m.gpu_pct} % on the CPU${ctx}. It does not fit in the graphics card memory, so it is slower; a smaller model or context would be faster.`);
  }
  const estimateTokens = (t) => Math.ceil([...t].length / 2.5);
  function contextSize(configured, promptTokens, image, modelMax) {
    const cap = Math.min(Math.max(modelMax || MAX_CTX, 2048), MAX_CTX);
    if (configured > 0) return Math.min(configured, cap);
    const need = promptTokens + (image ? 1600 : 0) + NUM_PREDICT + 256;
    return Math.min(Math.max(Math.ceil(need / 1024) * 1024, Math.min(4096, cap)), cap);
  }
  async function ollamaPull(root, model, p) {
    p.log(tr(`Preuzimam model ${model} s ollama.com (preko Ollame na ${root})…`, `Downloading model ${model} from ollama.com (through Ollama at ${root})…`));
    let r;
    try { r = await fetch(`${root}/api/pull`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ model, stream: true }), signal: p.signal }); }
    catch (e) { throw new Error(notRunning(root, e.message || e)); }
    if (!r.ok) throw new Error(`Ollama ${r.status}: ${await r.text()}`);
    let lastStatus = "", lastEmit = 0;
    for await (const line of lines(r, p)) {
      let v; try { v = JSON.parse(line); } catch (_) { continue; }
      if (v.error) throw new Error("Ollama: " + v.error);
      const status = v.status || "", done = v.completed || 0, total = v.total || 0, phase = status.split(/\s+/)[0];
      if (phase !== lastStatus.split(/\s+/)[0]) {
        p.log(phase === "pulling" && total ? tr(`Preuzimanje dijela ${(v.digest || "").slice(0, 19)} (${gb(total)})`, `Downloading layer ${(v.digest || "").slice(0, 19)} (${gb(total)})`)
          : phase === "pulling" ? tr("Dohvaćam popis datoteka modela…", "Fetching the model manifest…")
          : phase === "verifying" ? tr("Provjeravam preuzete datoteke…", "Verifying the downloaded files…")
          : phase === "writing" ? tr("Zapisujem model…", "Writing the model…")
          : phase === "success" ? tr(`Model ${model} je preuzet.`, `Model ${model} downloaded.`) : status);
        lastStatus = status;
      }
      if (total && performance.now() - lastEmit > 250) { p.stats({ status, completed: done, total, pct: Math.round((done / total) * 100) }); lastEmit = performance.now(); }
      if (status === "success") { p.stats({ status: "success", pct: 100 }); return; }
    }
    throw new Error(tr("preuzimanje nije završeno (veza prekinuta)", "download did not finish (connection lost)"));
  }
  async function ollamaDelete(root, model) {
    let r;
    try { r = await fetch(`${root}/api/delete`, { method: "DELETE", headers: { "content-type": "application/json" }, body: JSON.stringify({ model }) }); }
    catch (e) { throw new Error(notRunning(root, e.message || e)); }
    if (!r.ok) throw new Error(`Ollama ${r.status}: ${await r.text()}`);
  }
  async function ollamaChat(root, model, ctxSetting, system, user, image, maxTokens, p) {
    const t0 = performance.now();
    p.log(tr(`Ollama na ${root}, model ${model}.`, `Ollama at ${root}, model ${model}.`));
    let caps;
    try { caps = await capabilities(root, model); } catch (e) { if (/Failed to fetch|NetworkError|Load failed/.test(String(e.message))) throw new Error(notRunning(root, e.message)); throw e; }
    const [vision, modelCtx] = caps;
    if (image && !vision) { p.log(tr("Ovaj model nema vid (vision) — šaljem samo podatke, bez slike sonagrama.", "This model has no vision — sending the data only, without the spectrogram image.")); image = null; }
    const promptTokens = estimateTokens(system) + estimateTokens(user);
    const numPredict = Math.min(maxTokens, NUM_PREDICT);
    const numCtx = contextSize(ctxSetting, promptTokens, !!image, modelCtx);
    const need = promptTokens + (image ? 1600 : 0);
    const img = image ? tr(` + slika (${Math.floor((image[1].length * 3) / 4 / 1024)} kB)`, ` + image (${Math.floor((image[1].length * 3) / 4 / 1024)} kB)`) : "";
    p.log(tr(`Upit ≈ ${promptTokens} tokena${img}; kontekst ${numCtx} tokena, odgovor do ${numPredict} tokena.`, `Prompt ≈ ${promptTokens} tokens${img}; context ${numCtx} tokens, answer up to ${numPredict} tokens.`));
    if (need + 512 > numCtx) p.log(tr(`UPOZORENJE: kontekst ${numCtx} je premalen za upit (≈ ${need}) — Ollama će odrezati početak upita. Povećajte kontekst u AI postavkama ili odaberite „automatski”.`, `WARNING: context ${numCtx} is too small for the prompt (≈ ${need}) — Ollama will cut the start of the prompt. Increase the context in the AI settings or choose “automatic”.`));
    const now = await loaded(root), here = now.find((m) => m.name === model);
    p.log(here ? describeLoad(here) : tr("Učitavam model u memoriju (prvi put nakon pokretanja može potrajati)…", "Loading the model into memory (the first time after start-up can take a while)…"));
    p.log(tr("Model čita upit — prvi dio odgovora stiže kad obradi cijeli upit (na CPU-u to može trajati i nekoliko minuta).", "The model is reading the prompt — the first words arrive once it has processed all of it (on a CPU this can take several minutes)."));
    let first = false;
    if (!here) (async () => { // report where the model landed (GPU/CPU)
      const start = performance.now();
      while (!first && !p.cancelled() && performance.now() - start < 30 * 60e3) {
        await sleep(1500);
        const m = (await loaded(root)).find((x) => x.name === model);
        if (m) { p.log(tr(`${describeLoad(m)} (učitano nakon ${secs(start)} s)`, `${describeLoad(m)} (loaded after ${secs(start)} s)`)); return; }
      }
    })();
    const msg = { role: "user", content: user };
    if (image) msg.images = [image[1]];
    const body = { model, messages: [{ role: "system", content: system }, msg], stream: true, keep_alive: "30m", options: { num_ctx: numCtx, num_predict: numPredict, temperature: 0.4 } };
    let r;
    try { r = await fetch(`${root}/api/chat`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body), signal: p.signal }); }
    catch (e) { first = true; if (p.cancelled()) throw new Error(tr("prekinuto", "stopped")); throw new Error(notRunning(root, e.message || e)); }
    if (!r.ok) { first = true; throw new Error(tr(`Ollama je vratila grešku ${r.status}: `, `Ollama returned error ${r.status}: `) + (await r.text())); }
    let text = "", n = 0, thinking = 0, gen = 0, last = performance.now();
    try {
      for await (const line of lines(r, p)) {
        if (!line.trim()) continue;
        let v; try { v = JSON.parse(line); } catch (e) { throw new Error(tr(`neispravan odgovor Ollame: ${e.message}`, `invalid answer from Ollama: ${e.message}`)); }
        if (v.error) throw new Error("Ollama: " + v.error);
        if (v.message && v.message.thinking) { if (!thinking) { p.log(tr(`Model razmišlja prije odgovora (nakon ${secs(t0)} s)…`, `The model is thinking before answering (after ${secs(t0)} s)…`)); first = true; } thinking++; }
        if (v.message && v.message.content) {
          if (!gen) { gen = performance.now(); first = true; p.log(tr(`Prvi dio odgovora nakon ${secs(t0)} s — model piše…`, `First words after ${secs(t0)} s — the model is writing…`)); }
          n++; text += v.message.content; p.delta(v.message.content);
        }
        if (performance.now() - last > 1000) {
          const g = gen ? (performance.now() - gen) / 1000 : 0;
          p.stats({ tokens: n, thinking, tps: g > 0.5 ? Math.round((n / g) * 10) / 10 : 0, elapsed: Math.floor((performance.now() - t0) / 1000) });
          last = performance.now();
        }
        if (v.done) {
          const ns = (k) => (v[k] || 0) / 1e9, pc = v.prompt_eval_count || 0, pd = ns("prompt_eval_duration"), ec = v.eval_count || n, ed = ns("eval_duration");
          const rate = (c, d) => (d > 0 ? `${(c / d).toFixed(1)} tok/s` : "—");
          p.log(tr(`Gotovo za ${secs(t0)} s: učitavanje modela ${ns("load_duration").toFixed(1)} s · upit ${pc} tokena za ${pd.toFixed(1)} s (${rate(pc, pd)}) · odgovor ${ec} tokena za ${ed.toFixed(1)} s (${rate(ec, ed)}).`,
            `Done in ${secs(t0)} s: model loading ${ns("load_duration").toFixed(1)} s · prompt ${pc} tokens in ${pd.toFixed(1)} s (${rate(pc, pd)}) · answer ${ec} tokens in ${ed.toFixed(1)} s (${rate(ec, ed)}).`));
          if (pc > 0 && pc + 64 >= numCtx) p.log(tr("UPOZORENJE: upit je popunio cijeli kontekst — dio je vjerojatno odrezan. Povećajte kontekst.", "WARNING: the prompt filled the whole context — part of it was probably cut. Increase the context."));
          if (v.done_reason === "length") p.log(tr("Odgovor je dosegnuo najveću duljinu i skraćen je.", "The answer reached the maximum length and was cut."));
          p.stats({ tokens: ec, tps: ed > 0 ? Math.round((ec / ed) * 10) / 10 : 0, prompt_tokens: pc, prompt_tps: pd > 0 ? Math.round((pc / pd) * 10) / 10 : 0, elapsed: Math.floor((performance.now() - t0) / 1000), done: true });
          const answer = stripThinking(text);
          if (!answer) throw new Error(tr("model nije vratio tekst odgovora", "the model returned no answer text"));
          return { text: answer, model, truncated: v.done_reason === "length" };
        }
      }
    } finally { first = true; }
    throw new Error(tr("Ollama je prekinula vezu prije kraja odgovora", "Ollama closed the connection before the answer was complete"));
  }

  /** One request to the configured provider (progress: log/delta/stats/cancelled/signal). */
  async function ask(c, system, user, image, maxTokens, p) {
    check(c);
    const b = base(c), model = c.model.trim();
    const size = tr(`${[...system].length + [...user].length} znakova`, `${[...system].length + [...user].length} characters`) + (image ? tr(` + slika ${Math.floor((image[1].length * 3) / 4 / 1024)} kB`, ` + image ${Math.floor((image[1].length * 3) / 4 / 1024)} kB`) : "");
    if (c.provider === "ollama") return ollamaChat(b, model, c.ollama_ctx || 0, system, user, image, maxTokens, p);
    if (c.provider === "anthropic") {
      const url = b.endsWith("/v1") ? `${b}/messages` : `${b}/v1/messages`;
      let fallbacks = model.startsWith("claude-opus-5") || model.startsWith("claude-fable-5");
      p.log(tr(`Šaljem upit (${size}) na ${url}, model ${model}.`, `Sending the prompt (${size}) to ${url}, model ${model}.`));
      p.log(tr("Claude šalje odgovor u cjelini — čekam (temeljita analiza obično traje 30 s do 2 min)…", "Claude sends the answer in one piece — waiting (a thorough analysis usually takes 30 s to 2 min)…"));
      const t0 = performance.now();
      for (;;) {
        // the key goes only to Anthropic; the header opts in to calls from a browser
        const headers = { "x-api-key": c.api_key.trim(), "anthropic-version": ANTHROPIC_VERSION, "anthropic-dangerous-direct-browser-access": "true" };
        if (fallbacks) headers["anthropic-beta"] = FALLBACK_BETA;
        const content = [];
        if (image) content.push({ type: "image", source: { type: "base64", media_type: image[0], data: image[1] } });
        content.push({ type: "text", text: user });
        const body = { model, max_tokens: maxTokens, system, messages: [{ role: "user", content }] };
        if (fallbacks) body.fallbacks = "default";
        const [code, v] = await post(url, headers, body, p);
        if (code === 0) {
          const a = parseAnthropic(v), u = v.usage || {};
          p.log(tr(`Odgovor primljen nakon ${secs(t0)} s: model ${a.model}, upit ${u.input_tokens || 0} tokena, odgovor ${u.output_tokens || 0} tokena.`, `Answer received after ${secs(t0)} s: model ${a.model}, prompt ${u.input_tokens || 0} tokens, answer ${u.output_tokens || 0} tokens.`));
          p.stats({ tokens: u.output_tokens || 0, prompt_tokens: u.input_tokens || 0, elapsed: Math.floor((performance.now() - t0) / 1000), done: true });
          p.delta(a.text);
          return a;
        }
        if (code === 400 && fallbacks && /fallback/i.test(v)) { p.log(tr("Servis ne podržava zamjenski model (fallback) — ponavljam bez njega.", "The service does not support the fallback model — retrying without it.")); fallbacks = false; continue; }
        throw new Error(v);
      }
    }
    const url = `${b}/chat/completions`;
    const headers = c.api_key ? { authorization: `Bearer ${c.api_key.trim()}` } : {};
    p.log(tr(`Šaljem upit (${size}) na ${url}, model ${model}.`, `Sending the prompt (${size}) to ${url}, model ${model}.`));
    const uc = image ? [{ type: "text", text: user }, { type: "image_url", image_url: { url: `data:${image[0]};base64,${image[1]}` } }] : user;
    return openaiStream(url, headers, { model, messages: [{ role: "system", content: system }, { role: "user", content: uc }], stream: true }, p);
  }
  /** Model ids of the configured service, free OpenRouter models first. */
  async function listModels(c) {
    const b = base(c);
    let url, headers = {};
    if (c.provider === "ollama") url = `${b}/api/tags`;
    else if (c.provider === "anthropic") { url = (b.endsWith("/v1") ? `${b}/models` : `${b}/v1/models`) + "?limit=100"; headers = { "x-api-key": c.api_key.trim(), "anthropic-version": ANTHROPIC_VERSION, "anthropic-dangerous-direct-browser-access": "true" }; }
    else { url = `${b}/models`; if (c.api_key) headers = { authorization: `Bearer ${c.api_key.trim()}` }; }
    let r;
    try { r = await fetch(url, { headers }); } catch (e) { throw netError(url, e); }
    if (!r.ok) throw new Error(errorMessage(r.status, await r.text().catch(() => "")));
    const v = await r.json();
    const ids = [...new Set((v.data || v.models || []).map((m) => m.id || m.name).filter(Boolean))];
    return ids.sort((a, b) => (a.endsWith(":free") === b.endsWith(":free") ? a.toLowerCase().localeCompare(b.toLowerCase()) : a.endsWith(":free") ? -1 : 1));
  }
  const silent = () => { const ac = new AbortController(); return { log() {}, delta() {}, stats() {}, cancelled: () => false, signal: ac.signal }; };
  const test = (c) => (en() ? ask(c, "Answer briefly.", "This is a connection test. Answer with the single word: OK", null, 1024, silent())
    : ask(c, "Odgovaraj kratko.", "Ovo je test veze. Odgovori samo riječju: OK", null, 1024, silent()));

  return { listModels, DEFAULT_CLAUDE_MODEL, OLLAMA_URL, OLLAMA_MODEL, MAX_TOKENS, defaults, base, providerName, pub, check, parseImage, ask, test, ollamaRoot, ollamaStatus, ollamaPull, ollamaDelete };
})();
