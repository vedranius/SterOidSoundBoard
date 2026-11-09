"use strict";
// Service worker of the browser edition: keeps the app's own files offline.
// It never sees patient data (that stays in the page's storage) and caches
// nothing but the files listed below; build-web.sh fills them in.
const CACHE = "ssb-__SSB_CACHE__";
const FILES = __SSB_FILES__;

self.addEventListener("install", (e) => {
  e.waitUntil(caches.open(CACHE).then((c) => c.addAll(FILES)).then(() => self.skipWaiting()));
});
self.addEventListener("activate", (e) => {
  e.waitUntil(caches.keys().then((ks) => Promise.all(ks.filter((k) => k.startsWith("ssb-") && k !== CACHE).map((k) => caches.delete(k)))).then(() => self.clients.claim()));
});
self.addEventListener("fetch", (e) => {
  const u = new URL(e.request.url);
  if (e.request.method !== "GET" || u.origin !== location.origin) return; // AI services, Ollama: straight to the network
  const path = u.pathname === "/" ? "/index.html" : u.pathname;
  // Never answer with nothing: a missing cache entry plus a failed network
  // request would otherwise show the browser's bare ERR_FAILED page.
  e.respondWith(
    caches.open(CACHE).then(async (c) => {
      const hit = await c.match(path);
      if (hit) return hit;
      try {
        return await fetch(e.request);
      } catch (err) {
        const page = e.request.mode === "navigate" ? await c.match("/index.html") : undefined;
        return page || new Response("Offline / Nema veze s mrežom", { status: 503, headers: { "Content-Type": "text/plain; charset=utf-8" } });
      }
    }).catch(() => fetch(e.request))
  );
});
