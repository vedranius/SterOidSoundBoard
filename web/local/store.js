"use strict";
// Local data store of the browser edition. Nothing is ever sent to a server:
// files live either in a folder the user picks on their computer (File System
// Access API, Chrome/Edge) or in this browser's own storage (IndexedDB).
// Both use the same layout as the desktop app's data directory, so a folder
// can be shared with the installed SterOidSoundBoard program:
//   clinic.json, ai.json, audio.json, boards/default.json, presets/*.json,
//   clinic_presets/*.json, recordings/<id>.wav
const SsbStore = (() => {
  const META_DB = "ssb-meta", FILES_DB = "ssb-files";

  function idb(name, stores) {
    return new Promise((ok, fail) => {
      const r = indexedDB.open(name, 1);
      r.onupgradeneeded = () => stores.forEach((s) => r.result.objectStoreNames.contains(s) || r.result.createObjectStore(s));
      r.onsuccess = () => ok(r.result);
      r.onerror = () => fail(r.error);
    });
  }
  function tx(db, store, mode, fn) {
    return new Promise((ok, fail) => {
      const t = db.transaction(store, mode), s = t.objectStore(store);
      let out;
      Promise.resolve(fn(s)).then((v) => (out = v));
      t.oncomplete = () => ok(out);
      t.onerror = () => fail(t.error);
      t.onabort = () => fail(t.error || new Error("transaction aborted"));
    });
  }
  const req = (r) => new Promise((ok, fail) => { r.onsuccess = () => ok(r.result); r.onerror = () => fail(r.error); });

  // ---- settings of the store itself (which backend, folder handle)
  const meta = {
    db: null,
    async open() { return (this.db ||= await idb(META_DB, ["kv"])); },
    async get(k) { const db = await this.open(); return tx(db, "kv", "readonly", (s) => req(s.get(k))); },
    async set(k, v) { const db = await this.open(); return tx(db, "kv", "readwrite", (s) => { s.put(v, k); }); },
    async del(k) { const db = await this.open(); return tx(db, "kv", "readwrite", (s) => { s.delete(k); }); },
  };

  // ---- backend: this browser (IndexedDB), key = path, value = Blob
  class BrowserBackend {
    constructor() { this.kind = "browser"; this.label = "browser"; }
    async init() { this.db = await idb(FILES_DB, ["files"]); }
    async read(path) {
      const b = await tx(this.db, "files", "readonly", (s) => req(s.get(path)));
      return b == null ? null : new Uint8Array(await b.arrayBuffer());
    }
    async write(path, bytes) { const blob = new Blob([bytes]); await tx(this.db, "files", "readwrite", (s) => { s.put(blob, path); }); }
    async remove(path) { await tx(this.db, "files", "readwrite", (s) => { s.delete(path); }); }
    async list(dir) {
      const keys = await tx(this.db, "files", "readonly", (s) => req(s.getAllKeys()));
      const pre = dir ? dir.replace(/\/?$/, "/") : "";
      return keys.filter((k) => k.startsWith(pre) && !k.slice(pre.length).includes("/")).map((k) => k.slice(pre.length));
    }
    async all() { return tx(this.db, "files", "readonly", (s) => req(s.getAllKeys())); }
    async usage() {
      try { const e = await navigator.storage.estimate(); return e.usage || 0; } catch (_) { return 0; }
    }
  }

  // ---- backend: a folder on this computer (FileSystemDirectoryHandle)
  class FolderBackend {
    constructor(handle) { this.kind = "folder"; this.handle = handle; this.label = handle.name; }
    async init() {}
    async dir(parts, create) {
      let d = this.handle;
      for (const p of parts) d = await d.getDirectoryHandle(p, { create });
      return d;
    }
    split(path) { const parts = path.split("/").filter(Boolean); return [parts.slice(0, -1), parts[parts.length - 1]]; }
    async read(path) {
      const [dirs, name] = this.split(path);
      try {
        const f = await (await (await this.dir(dirs, false)).getFileHandle(name)).getFile();
        return new Uint8Array(await f.arrayBuffer());
      } catch (e) {
        if (e && (e.name === "NotFoundError" || e.name === "TypeMismatchError")) return null;
        throw e;
      }
    }
    async write(path, bytes) {
      const [dirs, name] = this.split(path);
      const fh = await (await this.dir(dirs, true)).getFileHandle(name, { create: true });
      const w = await fh.createWritable(); // written to a temporary file and swapped in on close
      await w.write(bytes);
      await w.close();
    }
    async remove(path) {
      const [dirs, name] = this.split(path);
      try { await (await this.dir(dirs, false)).removeEntry(name); } catch (e) { if (!e || e.name !== "NotFoundError") throw e; }
    }
    async list(dir) {
      try {
        const d = await this.dir(dir.split("/").filter(Boolean), false), out = [];
        for await (const [n, h] of d.entries()) if (h.kind === "file") out.push(n);
        return out;
      } catch (e) { if (e && e.name === "NotFoundError") return []; throw e; }
    }
    async all() {
      const out = [];
      const walk = async (d, pre) => { for await (const [n, h] of d.entries()) { if (h.kind === "file") out.push(pre + n); else await walk(h, pre + n + "/"); } };
      await walk(this.handle, "");
      return out.filter((p) => /^(clinic\.json|ai\.json|audio\.json|boards\/|presets\/|clinic_presets\/|recordings\/)/.test(p));
    }
  }

  const folderSupported = () => typeof window !== "undefined" && "showDirectoryPicker" in window;

  /** The configured backend, or null when the user has not chosen yet / the folder needs permission. */
  async function configured() {
    const kind = await meta.get("backend");
    if (kind === "browser") { const b = new BrowserBackend(); await b.init(); return { backend: b }; }
    if (kind === "folder") {
      const h = await meta.get("folder");
      if (!h) return { backend: null };
      const perm = await h.queryPermission({ mode: "readwrite" });
      if (perm === "granted") return { backend: new FolderBackend(h) };
      return { backend: null, needsPermission: h };
    }
    return { backend: null };
  }

  async function chooseFolder() {
    const h = await window.showDirectoryPicker({ id: "steroidsoundboard-data", mode: "readwrite", startIn: "documents" });
    if ((await h.requestPermission({ mode: "readwrite" })) !== "granted") throw new Error("permission denied");
    await meta.set("folder", h);
    await meta.set("backend", "folder");
    return new FolderBackend(h);
  }
  async function regrant(h) {
    if ((await h.requestPermission({ mode: "readwrite" })) !== "granted") throw new Error("permission denied");
    return new FolderBackend(h);
  }
  async function useBrowser() {
    await meta.set("backend", "browser");
    try { await navigator.storage.persist(); } catch (_) {}
    const b = new BrowserBackend(); await b.init(); return b;
  }
  async function forget() { await meta.del("backend"); await meta.del("folder"); }

  /** Copy every file from one backend to another (switching storage). */
  async function copyAll(from, to, progress) {
    const files = await from.all();
    let i = 0;
    for (const p of files) {
      const b = await from.read(p);
      if (b) await to.write(p, b);
      progress && progress(++i, files.length);
    }
    return files.length;
  }

  // ---- backup as a .zip (stored, no compression; opened by any unzip tool)
  const CRC = (() => { const t = new Uint32Array(256); for (let n = 0; n < 256; n++) { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; t[n] = c >>> 0; } return t; })();
  const crc32 = (b) => { let c = 0xffffffff; for (let i = 0; i < b.length; i++) c = CRC[(c ^ b[i]) & 255] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; };
  async function zip(backend) {
    const files = await backend.all(), parts = [], central = [];
    let off = 0;
    const enc = new TextEncoder();
    for (const p of files) {
      const data = await backend.read(p);
      if (!data) continue;
      const name = enc.encode(p), crc = crc32(data);
      const h = new DataView(new ArrayBuffer(30));
      h.setUint32(0, 0x04034b50, true); h.setUint16(4, 20, true); h.setUint16(6, 0x0800, true); h.setUint16(8, 0, true);
      h.setUint32(14, crc, true); h.setUint32(18, data.length, true); h.setUint32(22, data.length, true); h.setUint16(26, name.length, true);
      parts.push(new Uint8Array(h.buffer), name, data);
      const c = new DataView(new ArrayBuffer(46));
      c.setUint32(0, 0x02014b50, true); c.setUint16(4, 20, true); c.setUint16(6, 20, true); c.setUint16(8, 0x0800, true);
      c.setUint32(16, crc, true); c.setUint32(20, data.length, true); c.setUint32(24, data.length, true); c.setUint16(28, name.length, true); c.setUint32(42, off, true);
      central.push(new Uint8Array(c.buffer), name);
      off += 30 + name.length + data.length;
    }
    const csize = central.reduce((n, b) => n + b.length, 0);
    const e = new DataView(new ArrayBuffer(22));
    e.setUint32(0, 0x06054b50, true); e.setUint16(8, central.length / 2, true); e.setUint16(10, central.length / 2, true); e.setUint32(12, csize, true); e.setUint32(16, off, true);
    return new Blob([...parts, ...central, new Uint8Array(e.buffer)], { type: "application/zip" });
  }
  /** Files of a backup .zip made by `zip` (stored entries only). */
  function unzip(buf) {
    const b = new Uint8Array(buf), v = new DataView(b.buffer, b.byteOffset, b.byteLength), out = [], dec = new TextDecoder();
    let i = 0;
    while (i + 30 <= b.length && v.getUint32(i, true) === 0x04034b50) {
      const method = v.getUint16(i + 8, true), size = v.getUint32(i + 18, true), nlen = v.getUint16(i + 26, true), xlen = v.getUint16(i + 28, true);
      if (method !== 0) throw new Error("compressed zip entries are not supported");
      const name = dec.decode(b.subarray(i + 30, i + 30 + nlen)), start = i + 30 + nlen + xlen;
      out.push([name, b.slice(start, start + size)]);
      i = start + size;
    }
    return out;
  }

  return { meta, configured, chooseFolder, regrant, useBrowser, forget, copyAll, zip, unzip, folderSupported, BrowserBackend, FolderBackend };
})();
