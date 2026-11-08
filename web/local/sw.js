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
  e.respondWith(
    caches.open(CACHE).then(async (c) => (await c.match(path)) || fetch(e.request).catch(() => c.match("/index.html")))
  );
});
