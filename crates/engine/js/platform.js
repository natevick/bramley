(function () {
  "use strict";
  const G = globalThis;
  const OP = {};
  for (const k of Object.getOwnPropertyNames(G)) {
    if (k.startsWith("__op_")) {
      OP[k.slice(5)] = G[k];
      delete G[k];
    }
  }
  const hostCalls = Object.create(null);
  function hc(name) { hostCalls[name] = (hostCalls[name] || 0) + 1; }
  const uncaught = [];
  function reportError(where, e) {
    const msg = where + ": " + (e && e.stack ? e.stack : String(e));
    if (uncaught.length < 30) uncaught.push(msg.slice(0, 600));
    OP.print("error", msg.slice(0, 2000));
  }
  const now = () => OP.now();
  const ORIGIN = "app://bramley";
  const LOCATION_HREF = ORIGIN + "/index.html";
  const USER_AGENT = OP.user_agent();

  G.window = G;
  G.self = G;
  G.top = G;
  G.parent = G;
  G.frames = G;

  const consoleObj = {};
  for (const lvl of ["log", "info", "warn", "error", "debug", "trace"]) {
    consoleObj[lvl] = function (...a) {
      hc("console." + lvl);
      const text = a.map(x => { try { return typeof x === "string" ? x : (x instanceof Error ? x.stack || String(x) : JSON.stringify(x)); } catch (e) { return String(x); } }).join(" ");
      OP.print(lvl, text.slice(0, 2000));
    };
  }
  for (const k of ["group", "groupCollapsed", "groupEnd", "time", "timeEnd", "timeLog", "count", "countReset", "table", "dir", "dirxml", "profile", "profileEnd", "clear"]) consoleObj[k] = function () {};
  consoleObj.assert = function (c, ...a) { if (!c) consoleObj.error("Assertion failed", ...a); };
  G.console = consoleObj;

  let timerSeq = 1;
  const timers = new Map();
  const tasks = [];
  function queueTask(fn) { tasks.push(fn); }
  function setTimeoutImpl(fn, ms, args, interval) {
    const id = timerSeq++;
    const d = Math.max(0, Number(ms) || 0);
    timers.set(id, { due: now() + d, fn, args, interval: interval ? Math.max(1, d) : null, seq: id });
    return id;
  }
  G.setTimeout = function (fn, ms, ...args) { hc("setTimeout"); return setTimeoutImpl(fn, ms, args, false); };
  G.setInterval = function (fn, ms, ...args) { hc("setInterval"); return setTimeoutImpl(fn, ms, args, true); };
  G.clearTimeout = function (id) { hc("clearTimeout"); timers.delete(id); };
  G.clearInterval = function (id) { hc("clearInterval"); timers.delete(id); };
  G.queueMicrotask = function (fn) { hc("queueMicrotask"); Promise.resolve().then(fn).catch(e => reportError("microtask", e)); };
  G.requestAnimationFrame = function (fn) { hc("requestAnimationFrame"); return setTimeoutImpl(() => fn(now()), 16, [], false); };
  G.cancelAnimationFrame = function (id) { timers.delete(id); };
  G.requestIdleCallback = function (fn) { return setTimeoutImpl(() => fn({ didTimeout: false, timeRemaining: () => 10 }), 1, [], false); };
  G.cancelIdleCallback = function (id) { timers.delete(id); };

  function runTimers() {
    if (tasks.length) {
      const t = tasks.shift();
      try { t(); } catch (e) { reportError("task", e); }
      return -2;
    }
    const t = now();
    let best = null;
    for (const [id, tm] of timers) {
      if (tm.due <= t && (!best || tm.due < best[1].due || (tm.due === best[1].due && tm.seq < best[1].seq))) best = [id, tm];
    }
    if (best) {
      const [id, tm] = best;
      if (tm.interval !== null) { tm.due = t + tm.interval; tm.seq = timerSeq++; } else timers.delete(id);
      try { if (typeof tm.fn === "function") tm.fn.apply(G, tm.args || []); } catch (e) { reportError("timer", e); }
      return -2;
    }
    let next = -1;
    for (const tm of timers.values()) { const d = tm.due - t; if (next < 0 || d < next) next = d; }
    return next;
  }

  class DOMException extends Error {
    constructor(message = "", name = "Error") {
      super(message);
      this.name = name;
      const codes = { IndexSizeError: 1, NotFoundError: 8, NotSupportedError: 9, InvalidStateError: 11, SyntaxError: 12, InvalidAccessError: 15, TypeMismatchError: 17, SecurityError: 18, NetworkError: 19, AbortError: 20, QuotaExceededError: 22, TimeoutError: 23 };
      this.code = codes[name] || 0;
    }
  }
  G.DOMException = DOMException;

  class Event {
    constructor(type, init) {
      init = init || {};
      this.type = String(type);
      this.bubbles = !!init.bubbles;
      this.cancelable = !!init.cancelable;
      this.composed = !!init.composed;
      this.defaultPrevented = false;
      this.target = null;
      this.currentTarget = null;
      this.srcElement = null;
      this.eventPhase = 0;
      this.isTrusted = false;
      this.timeStamp = now();
      this._stop = false;
      this._stopImm = false;
    }
    preventDefault() { if (this.cancelable) this.defaultPrevented = true; }
    stopPropagation() { this._stop = true; }
    stopImmediatePropagation() { this._stop = true; this._stopImm = true; }
    get returnValue() { return !this.defaultPrevented; }
    initEvent(type, bubbles, cancelable) { this.type = String(type); this.bubbles = !!bubbles; this.cancelable = !!cancelable; }
    composedPath() { return this.target ? [this.target] : []; }
  }
  Event.NONE = 0; Event.CAPTURING_PHASE = 1; Event.AT_TARGET = 2; Event.BUBBLING_PHASE = 3;
  G.Event = Event;
  class CustomEvent extends Event { constructor(type, init) { super(type, init); this.detail = init && "detail" in init ? init.detail : null; } }
  G.CustomEvent = CustomEvent;
  class ProgressEvent extends Event { constructor(type, init) { super(type, init); init = init || {}; this.lengthComputable = !!init.lengthComputable; this.loaded = init.loaded || 0; this.total = init.total || 0; } }
  G.ProgressEvent = ProgressEvent;
  class ErrorEvent extends Event { constructor(type, init) { super(type, init); init = init || {}; this.message = init.message || ""; this.error = init.error || null; } }
  G.ErrorEvent = ErrorEvent;
  class MessageEvent extends Event { constructor(type, init) { super(type, init); init = init || {}; this.data = init.data; this.origin = init.origin || ""; this.source = init.source || null; } }
  G.MessageEvent = MessageEvent;
  class MediaKeyMessageEvent extends Event { constructor(type, init) { super(type, init); this.messageType = init.messageType; this.message = init.message; } }
  G.MediaKeyMessageEvent = MediaKeyMessageEvent;
  class MediaEncryptedEvent extends Event { constructor(type, init) { super(type, init); init = init || {}; this.initDataType = init.initDataType || ""; this.initData = init.initData || null; } }
  G.MediaEncryptedEvent = MediaEncryptedEvent;

  const LISTENERS = new WeakMap();
  function listenersOf(t) { let m = LISTENERS.get(t); if (!m) { m = new Map(); LISTENERS.set(t, m); } return m; }
  function addListener(target, type, cb, opts) {
    if (!cb) return;
    const capture = typeof opts === "boolean" ? opts : !!(opts && opts.capture);
    const once = !!(opts && typeof opts === "object" && opts.once);
    const m = listenersOf(target);
    let arr = m.get(type);
    if (!arr) { arr = []; m.set(type, arr); }
    if (arr.some(l => l.cb === cb && l.capture === capture)) return;
    arr.push({ cb, capture, once });
    if (opts && typeof opts === "object" && opts.signal) opts.signal.addEventListener("abort", () => removeListener(target, type, cb, opts));
  }
  function removeListener(target, type, cb, opts) {
    const capture = typeof opts === "boolean" ? opts : !!(opts && opts.capture);
    const arr = listenersOf(target).get(type);
    if (!arr) return;
    const i = arr.findIndex(l => l.cb === cb && l.capture === capture);
    if (i >= 0) arr.splice(i, 1);
  }
  function dispatch(target, ev) {
    try { ev.target = target; ev.srcElement = target; } catch (e) {}
    ev.currentTarget = target;
    ev.eventPhase = 2;
    const handler = target["on" + ev.type];
    if (typeof handler === "function") {
      try { const r = handler.call(target, ev); if (r === false) ev.preventDefault(); } catch (e) { reportError("on" + ev.type, e); }
    }
    const arr = listenersOf(target).get(ev.type);
    if (arr) {
      for (const l of arr.slice()) {
        if (l.once) removeListener(target, ev.type, l.cb, { capture: l.capture });
        try {
          if (typeof l.cb === "function") l.cb.call(target, ev);
          else if (l.cb && typeof l.cb.handleEvent === "function") l.cb.handleEvent(ev);
        } catch (e) { reportError("listener " + ev.type, e); }
        if (ev._stopImm) break;
      }
    }
    ev.currentTarget = null;
    ev.eventPhase = 0;
    return !ev.defaultPrevented;
  }
  class EventTarget {
    addEventListener(type, cb, opts) { addListener(this, String(type), cb, opts); }
    removeEventListener(type, cb, opts) { removeListener(this, String(type), cb, opts); }
    dispatchEvent(ev) { return dispatch(this, ev); }
  }
  G.EventTarget = EventTarget;
  G.addEventListener = function (type, cb, opts) { hc("window.addEventListener"); addListener(G, String(type), cb, opts); };
  G.removeEventListener = function (type, cb, opts) { removeListener(G, String(type), cb, opts); };
  G.dispatchEvent = function (ev) { return dispatch(G, ev); };
  function fire(target, type, init) { return dispatch(target, new Event(type, init)); }

  const B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  G.btoa = function (s) {
    s = String(s);
    let out = "";
    for (let i = 0; i < s.length; i += 3) {
      const a = s.charCodeAt(i), b = s.charCodeAt(i + 1), c = s.charCodeAt(i + 2);
      if (a > 255 || b > 255 || c > 255) throw new DOMException("Invalid character", "InvalidCharacterError");
      const n = (a << 16) | ((b || 0) << 8) | (c || 0);
      out += B64[(n >> 18) & 63] + B64[(n >> 12) & 63] + (i + 1 < s.length ? B64[(n >> 6) & 63] : "=") + (i + 2 < s.length ? B64[n & 63] : "=");
    }
    return out;
  };
  G.atob = function (s) {
    s = String(s).replace(/[\s=]/g, "");
    let out = "", buf = 0, bits = 0;
    for (const ch of s) {
      const v = B64.indexOf(ch);
      if (v < 0) throw new DOMException("Invalid character", "InvalidCharacterError");
      buf = (buf << 6) | v; bits += 6;
      if (bits >= 8) { bits -= 8; out += String.fromCharCode((buf >> bits) & 255); }
    }
    return out;
  };

  function utf8Encode(str) {
    const out = [];
    for (let i = 0; i < str.length; i++) {
      let c = str.charCodeAt(i);
      if (c >= 0xd800 && c <= 0xdbff && i + 1 < str.length) {
        const d = str.charCodeAt(i + 1);
        if (d >= 0xdc00 && d <= 0xdfff) { c = 0x10000 + ((c - 0xd800) << 10) + (d - 0xdc00); i++; }
      }
      if (c < 0x80) out.push(c);
      else if (c < 0x800) out.push(0xc0 | (c >> 6), 0x80 | (c & 63));
      else if (c < 0x10000) out.push(0xe0 | (c >> 12), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
      else out.push(0xf0 | (c >> 18), 0x80 | ((c >> 12) & 63), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
    }
    return new Uint8Array(out);
  }
  function utf8Decode(u8) {
    let out = "";
    let i = 0;
    if (u8.length >= 3 && u8[0] === 0xef && u8[1] === 0xbb && u8[2] === 0xbf) i = 3;
    const parts = [];
    while (i < u8.length) {
      const a = u8[i++];
      let c;
      if (a < 0x80) c = a;
      else if (a >= 0xc0 && a < 0xe0) c = ((a & 31) << 6) | (u8[i++] & 63);
      else if (a >= 0xe0 && a < 0xf0) { c = ((a & 15) << 12) | ((u8[i] & 63) << 6) | (u8[i + 1] & 63); i += 2; }
      else if (a >= 0xf0) { c = ((a & 7) << 18) | ((u8[i] & 63) << 12) | ((u8[i + 1] & 63) << 6) | (u8[i + 2] & 63); i += 3; }
      else c = 0xfffd;
      if (c > 0xffff) { c -= 0x10000; parts.push(0xd800 + (c >> 10), 0xdc00 + (c & 1023)); }
      else parts.push(c);
      if (parts.length > 8192) { out += String.fromCharCode.apply(null, parts); parts.length = 0; }
    }
    out += String.fromCharCode.apply(null, parts);
    return out;
  }
  function toU8(data) {
    if (data == null) return new Uint8Array(0);
    if (data instanceof ArrayBuffer) return new Uint8Array(data);
    if (ArrayBuffer.isView(data)) return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
    if (typeof data === "string") return utf8Encode(data);
    throw new TypeError("expected BufferSource");
  }
  function toAB(data) { return toU8(data).slice().buffer; }
  class TextEncoder { get encoding() { return "utf-8"; } encode(s) { hc("TextEncoder.encode"); return utf8Encode(String(s === undefined ? "" : s)); } }
  class TextDecoder {
    constructor(label = "utf-8") { this._enc = String(label).toLowerCase(); }
    get encoding() { return this._enc; }
    decode(data) {
      hc("TextDecoder.decode");
      if (data === undefined) return "";
      const u8 = toU8(data);
      if (this._enc === "utf-16le" || this._enc === "utf-16") { let s = ""; for (let i = 0; i + 1 < u8.length; i += 2) s += String.fromCharCode(u8[i] | (u8[i + 1] << 8)); return s; }
      if (this._enc === "utf-16be") { let s = ""; for (let i = 0; i + 1 < u8.length; i += 2) s += String.fromCharCode((u8[i] << 8) | u8[i + 1]); return s; }
      if (this._enc === "ascii" || this._enc === "latin1" || this._enc === "iso-8859-1") { let s = ""; for (const b of u8) s += String.fromCharCode(b); return s; }
      return utf8Decode(u8);
    }
  }
  G.TextEncoder = TextEncoder;
  G.TextDecoder = TextDecoder;

  const blobRegistry = new Map();
  let blobSeq = 1;
  class Blob {
    constructor(parts, opts) {
      const chunks = (parts || []).map(p => p instanceof Blob ? p._u8 : toU8(p));
      const len = chunks.reduce((a, c) => a + c.length, 0);
      const u8 = new Uint8Array(len);
      let o = 0;
      for (const c of chunks) { u8.set(c, o); o += c.length; }
      this._u8 = u8;
      this.type = (opts && opts.type) || "";
    }
    get size() { return this._u8.length; }
    arrayBuffer() { return Promise.resolve(this._u8.slice().buffer); }
    text() { return Promise.resolve(utf8Decode(this._u8)); }
    slice(a, b, type) { const x = new Blob([], { type }); x._u8 = this._u8.slice(a, b); return x; }
  }
  G.Blob = Blob;

  function parseUrl(s) {
    const m = /^([a-zA-Z][a-zA-Z0-9+.-]*:)(?:\/\/(?:([^:@\/?#]*)(?::([^@\/?#]*))?@)?([^:\/?#]*)(?::(\d+))?)?([^?#]*)(\?[^#]*)?(#.*)?$/.exec(s);
    if (!m) return null;
    const hasAuthority = s.slice(m[1].length, m[1].length + 2) === "//";
    return { protocol: m[1].toLowerCase(), username: m[2] || "", password: m[3] || "", hostname: (m[4] || "").toLowerCase(), port: m[5] || "", pathname: m[6] || (hasAuthority ? "/" : ""), search: m[7] && m[7] !== "?" ? m[7] : "", hash: m[8] && m[8] !== "#" ? m[8] : "", hasAuthority };
  }
  function removeDots(path) {
    const out = [];
    const segs = path.split("/");
    for (let i = 0; i < segs.length; i++) {
      const s = segs[i];
      if (s === "..") { if (out.length > 1) out.pop(); if (i === segs.length - 1) out.push(""); }
      else if (s === ".") { if (i === segs.length - 1) out.push(""); }
      else out.push(s);
    }
    return out.join("/");
  }
  function resolveUrl(rel, base) {
    rel = String(rel).trim();
    if (/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(rel)) return rel;
    const b = parseUrl(base);
    if (!b) throw new TypeError("Invalid base URL: " + base);
    const origin = b.protocol + (b.hasAuthority ? "//" + b.hostname + (b.port ? ":" + b.port : "") : "");
    if (rel.startsWith("//")) return b.protocol + rel;
    if (rel.startsWith("/")) { const p = /^([^?#]*)(.*)$/.exec(rel); return origin + removeDots(p[1]) + p[2]; }
    if (rel === "") return origin + b.pathname + b.search;
    if (rel.startsWith("?")) return origin + b.pathname + rel;
    if (rel.startsWith("#")) return origin + b.pathname + b.search + rel;
    const dir = b.pathname.slice(0, b.pathname.lastIndexOf("/") + 1) || "/";
    const p = /^([^?#]*)(.*)$/.exec(rel);
    return origin + removeDots(dir + p[1]) + p[2];
  }
  class URLSearchParams {
    constructor(init) {
      this._l = [];
      if (typeof init === "string") {
        const s = init.startsWith("?") ? init.slice(1) : init;
        for (const part of s.split("&")) { if (!part) continue; const i = part.indexOf("="); const k = i < 0 ? part : part.slice(0, i); const v = i < 0 ? "" : part.slice(i + 1); this._l.push([decodeURIComponent(k.replace(/\+/g, " ")), decodeURIComponent(v.replace(/\+/g, " "))]); }
      } else if (init && typeof init === "object") {
        const entries = Array.isArray(init) ? init : Object.entries(init);
        for (const [k, v] of entries) this._l.push([String(k), String(v)]);
      }
    }
    get(k) { const e = this._l.find(x => x[0] === k); return e ? e[1] : null; }
    getAll(k) { return this._l.filter(x => x[0] === k).map(x => x[1]); }
    has(k) { return this._l.some(x => x[0] === k); }
    set(k, v) { const i = this._l.findIndex(x => x[0] === k); if (i < 0) this._l.push([k, String(v)]); else { this._l[i][1] = String(v); this._l = this._l.filter((x, j) => x[0] !== k || j === i); } if (this._u) this._u._sync(); }
    append(k, v) { this._l.push([k, String(v)]); if (this._u) this._u._sync(); }
    delete(k) { this._l = this._l.filter(x => x[0] !== k); if (this._u) this._u._sync(); }
    forEach(cb, t) { for (const [k, v] of this._l) cb.call(t, v, k, this); }
    entries() { return this._l[Symbol.iterator](); }
    keys() { return this._l.map(x => x[0])[Symbol.iterator](); }
    values() { return this._l.map(x => x[1])[Symbol.iterator](); }
    [Symbol.iterator]() { return this.entries(); }
    toString() { return this._l.map(([k, v]) => encodeURIComponent(k) + "=" + encodeURIComponent(v)).join("&"); }
  }
  G.URLSearchParams = URLSearchParams;
  class URL {
    constructor(url, base) {
      hc("URL.constructor");
      const abs = base !== undefined ? resolveUrl(String(url), String(base instanceof URL ? base.href : new URL(base).href)) : String(url);
      const p = parseUrl(abs);
      if (!p) throw new TypeError("Invalid URL: " + url);
      Object.assign(this, p);
      if (p.hasAuthority && p.pathname) this.pathname = removeDots(p.pathname);
      this._sp = null;
    }
    get host() { return this.hostname + (this.port ? ":" + this.port : ""); }
    get origin() { return this.hasAuthority ? this.protocol + "//" + this.host : "null"; }
    get href() { return this.protocol + (this.hasAuthority ? "//" + (this.username ? this.username + (this.password ? ":" + this.password : "") + "@" : "") + this.host : "") + this.pathname + this.search + this.hash; }
    set href(v) { Object.assign(this, new URL(v)); }
    get searchParams() { if (!this._sp) { this._sp = new URLSearchParams(this.search); this._sp._u = this; } return this._sp; }
    _sync() { const s = this._sp.toString(); this.search = s ? "?" + s : ""; }
    toString() { return this.href; }
    toJSON() { return this.href; }
    static createObjectURL(obj) { hc("URL.createObjectURL"); const u = "blob:" + ORIGIN + "/" + (blobSeq++); blobRegistry.set(u, obj); return u; }
    static revokeObjectURL(u) { hc("URL.revokeObjectURL"); blobRegistry.delete(u); }
    static canParse(u, b) { try { new URL(u, b); return true; } catch (e) { return false; } }
  }
  G.URL = URL;

  G.location = {
    get href() { return LOCATION_HREF; }, protocol: "app:", host: "bramley", hostname: "bramley", port: "", pathname: "/index.html", search: "", hash: "", origin: ORIGIN,
    assign() {}, replace() {}, reload() {}, toString() { return LOCATION_HREF; }
  };

  class Storage {
    constructor() { Object.defineProperty(this, "_m", { value: new Map() }); }
    get length() { return this._m.size; }
    key(i) { return Array.from(this._m.keys())[i] ?? null; }
    getItem(k) { hc("Storage.getItem"); return this._m.has(String(k)) ? this._m.get(String(k)) : null; }
    setItem(k, v) { hc("Storage.setItem"); this._m.set(String(k), String(v)); }
    removeItem(k) { hc("Storage.removeItem"); this._m.delete(String(k)); }
    clear() { this._m.clear(); }
  }
  G.Storage = Storage;
  G.localStorage = new Storage();
  G.sessionStorage = new Storage();

  G.crypto = {
    getRandomValues(arr) { hc("crypto.getRandomValues"); const u8 = new Uint8Array(OP.random(arr.byteLength)); new Uint8Array(arr.buffer, arr.byteOffset, arr.byteLength).set(u8); return arr; },
    randomUUID() { const b = new Uint8Array(OP.random(16)); b[6] = (b[6] & 15) | 64; b[8] = (b[8] & 63) | 128; const h = Array.from(b, x => x.toString(16).padStart(2, "0")).join(""); return h.slice(0, 8) + "-" + h.slice(8, 12) + "-" + h.slice(12, 16) + "-" + h.slice(16, 20) + "-" + h.slice(20); },
    subtle: undefined
  };

  const timeOrigin = Date.now() - now();
  G.performance = {
    now() { return now(); },
    timeOrigin,
    mark() {}, measure() {}, clearMarks() {}, clearMeasures() {},
    getEntriesByName() { return []; }, getEntriesByType() { return []; }, getEntries() { return []; }
  };

  class Node extends EventTarget {
    constructor() { super(); this.childNodes = []; this.parentNode = null; }
    get firstChild() { return this.childNodes[0] || null; }
    get lastChild() { return this.childNodes[this.childNodes.length - 1] || null; }
    get children() { return this.childNodes.filter(n => n instanceof Element); }
    get parentElement() { return this.parentNode instanceof Element ? this.parentNode : null; }
    get ownerDocument() { return document; }
    get isConnected() { let n = this; while (n) { if (n === document) return true; n = n.parentNode; } return false; }
    appendChild(c) { hc("Node.appendChild"); if (c instanceof DocumentFragment) { for (const k of c.childNodes.slice()) this.appendChild(k); return c; } if (c.parentNode) c.parentNode.removeChild(c); this.childNodes.push(c); c.parentNode = this; if (c.isConnected) connected(c); return c; }
    insertBefore(c, ref) { if (!ref) return this.appendChild(c); if (c instanceof DocumentFragment) { for (const k of c.childNodes.slice()) this.insertBefore(k, ref); return c; } if (c.parentNode) c.parentNode.removeChild(c); const i = this.childNodes.indexOf(ref); this.childNodes.splice(i < 0 ? this.childNodes.length : i, 0, c); c.parentNode = this; if (c.isConnected) connected(c); return c; }
    removeChild(c) { const i = this.childNodes.indexOf(c); if (i >= 0) this.childNodes.splice(i, 1); c.parentNode = null; return c; }
    remove() { if (this.parentNode) this.parentNode.removeChild(this); }
    contains(n) { while (n) { if (n === this) return true; n = n.parentNode; } return false; }
    get textContent() { return this._text || this.childNodes.map(c => c.textContent || "").join(""); }
    set textContent(v) { this._text = String(v); this.childNodes = []; }
  }
  Node.ELEMENT_NODE = 1; Node.TEXT_NODE = 3; Node.DOCUMENT_NODE = 9; Node.DOCUMENT_FRAGMENT_NODE = 11;
  G.Node = Node;
  class DocumentFragment extends Node {
    get nodeType() { return 11; }
    querySelector(sel) { const o = []; walk(this, o, sel, true); return o[0] || null; }
    querySelectorAll(sel) { const o = []; walk(this, o, sel, false); return o; }
  }
  G.DocumentFragment = DocumentFragment;
  function matches(el, sel) {
    sel = sel.trim();
    const m = /^([a-zA-Z0-9-]*|\*)?(?:#([\w-]+))?((?:\.[\w-]+)*)((?:\[[^\]]+\])*)$/.exec(sel);
    if (!m) return false;
    if (m[1] && m[1] !== "*" && el.tagName.toLowerCase() !== m[1].toLowerCase()) return false;
    if (m[2] && el.getAttribute("id") !== m[2]) return false;
    if (m[3]) for (const c of m[3].split(".").filter(Boolean)) if (!(el.getAttribute("class") || "").split(/\s+/).includes(c)) return false;
    if (m[4]) {
      const re = /\[([\w-]+)(?:([~|^$*]?=)["']?([^"'\]]*)["']?)?\]/g;
      let a;
      while ((a = re.exec(m[4]))) {
        const v = el.getAttribute(a[1]);
        if (v === null) return false;
        if (a[2] === "=" && v !== a[3]) return false;
      }
    }
    return true;
  }
  function walk(root, out, sel, firstOnly) {
    for (const c of root.childNodes) {
      if (c instanceof Element) {
        if (sel.split(",").some(s => matches(c, s))) { out.push(c); if (firstOnly) return true; }
        if (walk(c, out, sel, firstOnly)) return true;
      }
    }
    return false;
  }
  class Element extends Node {
    constructor(tag) { super(); this.tagName = String(tag).toUpperCase(); this._attrs = new Map(); this.style = {}; this.dataset = {}; const self = this; this.classList = { add(...c) { const s = new Set((self.getAttribute("class") || "").split(/\s+/).filter(Boolean)); c.forEach(x => s.add(x)); self.setAttribute("class", [...s].join(" ")); }, remove(...c) { const s = new Set((self.getAttribute("class") || "").split(/\s+/).filter(Boolean)); c.forEach(x => s.delete(x)); self.setAttribute("class", [...s].join(" ")); }, contains(c) { return (self.getAttribute("class") || "").split(/\s+/).includes(c); }, toggle(c) { if (this.contains(c)) { this.remove(c); return false; } this.add(c); return true; } }; }
    get nodeName() { return this.tagName; }
    get nodeType() { return 1; }
    get localName() { return this.tagName.toLowerCase(); }
    get id() { return this.getAttribute("id") || ""; }
    set id(v) { this.setAttribute("id", v); }
    get className() { return this.getAttribute("class") || ""; }
    set className(v) { this.setAttribute("class", v); }
    getAttribute(n) { n = String(n).toLowerCase(); return this._attrs.has(n) ? this._attrs.get(n) : null; }
    setAttribute(n, v) { this._attrs.set(String(n).toLowerCase(), String(v)); }
    removeAttribute(n) { this._attrs.delete(String(n).toLowerCase()); }
    hasAttribute(n) { return this._attrs.has(String(n).toLowerCase()); }
    get attributes() { return Array.from(this._attrs, ([name, value]) => ({ name, value })); }
    querySelector(sel) { hc("Element.querySelector"); const o = []; walk(this, o, sel, true); return o[0] || null; }
    querySelectorAll(sel) { hc("Element.querySelectorAll"); const o = []; walk(this, o, sel, false); return o; }
    getElementsByTagName(t) { const o = []; walk(this, o, t, false); return o; }
    getBoundingClientRect() { return { x: 0, y: 0, width: 0, height: 0, top: 0, left: 0, right: 0, bottom: 0 }; }
    get innerHTML() { return ""; }
    set innerHTML(v) { this.childNodes = []; }
    get outerHTML() { return "<" + this.localName + ">"; }
    focus() {} blur() {} click() { fire(this, "click"); }
    attachShadow() { return new Element("shadow-root"); }
  }
  G.Element = Element;
  class HTMLElement extends Element {}
  G.HTMLElement = HTMLElement;
  class HTMLDivElement extends HTMLElement {}
  G.HTMLDivElement = HTMLDivElement;
  class HTMLHeadElement extends HTMLElement {}
  class HTMLBodyElement extends HTMLElement {}
  class HTMLSourceElement extends HTMLElement {
    get src() { return this.getAttribute("src") || ""; }
    set src(v) { this.setAttribute("src", v); }
    get type() { return this.getAttribute("type") || ""; }
    set type(v) { this.setAttribute("type", v); }
  }
  G.HTMLSourceElement = HTMLSourceElement;
  class HTMLTrackElement extends HTMLElement {}
  G.HTMLTrackElement = HTMLTrackElement;
  class HTMLMetaElement extends HTMLElement { get name() { return this.getAttribute("name") || ""; } get content() { return this.getAttribute("content") || ""; } }
  G.HTMLMetaElement = HTMLMetaElement;

  const scriptPending = new Map();
  let scriptSeq = 1;
  class HTMLScriptElement extends HTMLElement {
    get src() { const s = this.getAttribute("src"); return s ? resolveUrl(s, LOCATION_HREF) : ""; }
    set src(v) { this.setAttribute("src", v); }
    get type() { return this.getAttribute("type") || ""; }
    set type(v) { this.setAttribute("type", v); }
    get async() { return true; } set async(v) {}
    get defer() { return false; } set defer(v) {}
    get crossOrigin() { return this.getAttribute("crossorigin"); } set crossOrigin(v) { this.setAttribute("crossorigin", v); }
  }
  G.HTMLScriptElement = HTMLScriptElement;
  function connected(el) {
    if (el instanceof HTMLScriptElement && el.getAttribute("src") && !el._started) {
      el._started = true;
      const id = scriptSeq++;
      scriptPending.set(id, el);
      hc("HTMLScriptElement.load");
      OP.load_script(id, el.src);
    }
    for (const c of el.childNodes) if (c instanceof Element) connected(c);
  }
  function dispatchScript(id, ok, err) {
    const el = scriptPending.get(id);
    scriptPending.delete(id);
    if (!el) return;
    if (ok) fire(el, "load");
    else { if (err) OP.print("warn", "script failed " + el.src + ": " + err); dispatch(el, new ErrorEvent("error", { message: err })); }
  }

  class TimeRanges {
    constructor(r) { this._r = r || []; }
    get length() { return this._r.length; }
    start(i) { if (i < 0 || i >= this._r.length) throw new DOMException("Index out of range", "IndexSizeError"); return this._r[i][0]; }
    end(i) { if (i < 0 || i >= this._r.length) throw new DOMException("Index out of range", "IndexSizeError"); return this._r[i][1]; }
  }
  G.TimeRanges = TimeRanges;
  class MediaError { constructor(code, message) { this.code = code; this.message = message || ""; } }
  MediaError.MEDIA_ERR_ABORTED = 1; MediaError.MEDIA_ERR_NETWORK = 2; MediaError.MEDIA_ERR_DECODE = 3; MediaError.MEDIA_ERR_SRC_NOT_SUPPORTED = 4;
  G.MediaError = MediaError;

  class TextTrackList extends EventTarget {
    constructor() { super(); this._t = []; }
    get length() { return this._t.length; }
    item(i) { return this._t[i] || null; }
    getTrackById(id) { return this._t.find(t => t.id === id) || null; }
    [Symbol.iterator]() { return this._t[Symbol.iterator](); }
  }
  class TextTrack extends EventTarget {
    constructor(kind, label, language) { super(); this.kind = kind || "subtitles"; this.label = label || ""; this.language = language || ""; this.id = ""; this.mode = "disabled"; this.cues = []; this.activeCues = []; this.oncuechange = null; }
    addCue(c) { this.cues.push(c); }
    removeCue(c) { const i = this.cues.indexOf(c); if (i >= 0) this.cues.splice(i, 1); }
  }
  G.TextTrack = TextTrack;
  G.TextTrackList = TextTrackList;

  const mediaById = new Map();
  const nativeReaper = new FinalizationRegistry(([kind, id]) => { if (kind === "media") mediaById.delete(id); else if (kind === "cdm") keysById.delete(id); OP.destroy(kind, id); });
  class HTMLMediaElement extends HTMLElement {
    constructor(tag) {
      super(tag);
      this._id = 0;
      this._src = "";
      this._ms = null;
      this._st = { currentTime: 0, readyState: 0, paused: true, seeking: false, ended: false };
      this._loop = false;
      this._volume = 1;
      this._muted = false;
      this._rate = 1;
      this._defaultRate = 1;
      this._mediaKeys = null;
      this._playPromises = [];
      this._loadGen = 0;
      this._error = null;
      this._autoplay = false;
      this._textTracks = new TextTrackList();
      this._networkState = 0;
      this.preload = "auto";
      this.crossOrigin = null;
      this.controls = false;
      this.disableRemotePlayback = false;
      this.preservesPitch = true;
    }
    _native() {
      if (!this._id) {
        this._id = OP.media_create();
        mediaById.set(this._id, new WeakRef(this));
        nativeReaper.register(this, ["media", this._id]);
        OP.media_set_loop(this._id, this._loop);
        if (this._muted || this._volume !== 1) this._applyGain();
        if (this._mediaKeys) OP.media_set_keys(this._id, this._mediaKeys._id);
      }
      return this._id;
    }
    _settlePlay(err) { const ps = this._playPromises; this._playPromises = []; for (const p of ps) err ? p.rej(err) : p.res(); }
    _applyGain() { if (this._id) OP.media_set_gain(this._id, this._muted ? 0 : this._volume); }
    _refresh() { if (this._id) { try { Object.assign(this._st, JSON.parse(OP.media_state(this._id))); } catch (e) {} } return this._st; }
    get src() { return this._src; }
    set src(v) { hc("HTMLMediaElement.src="); this._src = String(v); this.setAttribute("src", this._src); this._loadResource(); }
    get currentSrc() { return this._src; }
    removeAttribute(n) { hc("HTMLMediaElement.removeAttribute"); super.removeAttribute(n); if (String(n).toLowerCase() === "src") this._src = ""; }
    load() { hc("HTMLMediaElement.load"); this._loadResource(); }
    _loadResource() {
      this._loadGen++;
      if (this._id) OP.media_detach(this._id);
      if (this._ms) { const old = this._ms; this._ms = null; old._detach(); }
      const hadData = this._st.readyState > 0;
      this._error = null;
      this._st.readyState = 0; this._st.currentTime = 0; this._st.ended = false; this._st.seeking = false; this._st.paused = true;
      this._settlePlay(new DOMException("The play() request was interrupted by a new load request.", "AbortError"));
      if (hadData) queueTask(() => fire(this, "emptied"));
      let src = this._src;
      if (!src) { const s = this.querySelector("source"); if (s) src = s.src; }
      if (!src) { this._networkState = 3; return; }
      const obj = blobRegistry.get(src);
      this._networkState = 2;
      queueTask(() => fire(this, "loadstart"));
      if (obj instanceof MediaSource) {
        this._ms = obj;
        OP.media_attach(this._native(), obj._id);
        obj._attach(this);
      } else {
        const u = parseUrl(src);
        hc("HTMLMediaElement.unsupportedSrc:" + (u ? u.protocol + "//" + u.hostname : "?"));
        this._error = new MediaError(4, "Only MediaSource object URLs are supported in this runtime");
        queueTask(() => fire(this, "error"));
      }
    }
    get currentTime() { hc("HTMLMediaElement.currentTime"); return this._refresh().currentTime; }
    set currentTime(t) {
      hc("HTMLMediaElement.currentTime=");
      t = Number(t);
      if (!isFinite(t)) throw new TypeError("non-finite currentTime");
      const d = this.duration;
      t = Math.max(0, isFinite(d) ? Math.min(t, d) : t);
      this._st.currentTime = t;
      if (this._refresh().readyState > 0) this._st.seeking = true;
      if (this._id) OP.media_seek(this._id, t);
    }
    fastSeek(t) { this.currentTime = t; }
    get duration() { return this._ms ? this._ms._duration : NaN; }
    get paused() { return this._refresh().paused; }
    get ended() { return this._refresh().ended; }
    get seeking() { return this._refresh().seeking; }
    get readyState() { return this._refresh().readyState; }
    get networkState() { return this._networkState; }
    get buffered() { hc("HTMLMediaElement.buffered"); return this._ms ? this._ms._buffered() : new TimeRanges([]); }
    get seekable() { const d = this.duration; return new TimeRanges(isFinite(d) ? [[0, d]] : []); }
    get played() { return new TimeRanges([]); }
    get error() { return this._error; }
    get loop() { return this._loop; }
    set loop(v) { this._loop = !!v; if (this._id) OP.media_set_loop(this._id, this._loop); }
    get autoplay() { return this._autoplay; }
    set autoplay(v) { this._autoplay = !!v; }
    get volume() { return this._volume; }
    set volume(v) {
      v = Number(v);
      if (!isFinite(v)) throw new TypeError("non-finite volume");
      if (v < 0 || v > 1) throw new DOMException("The volume provided is outside the range [0, 1].", "IndexSizeError");
      if (v === this._volume) return;
      this._volume = v;
      this._applyGain();
      queueTask(() => fire(this, "volumechange"));
    }
    get muted() { return this._muted; }
    set muted(v) { v = !!v; if (v === this._muted) return; this._muted = v; this._applyGain(); queueTask(() => fire(this, "volumechange")); }
    get playbackRate() { return this._rate; }
    set playbackRate(v) { this._rate = Number(v); queueTask(() => fire(this, "ratechange")); }
    get defaultPlaybackRate() { return this._defaultRate; }
    set defaultPlaybackRate(v) { this._defaultRate = Number(v); }
    get textTracks() { return this._textTracks; }
    get audioTracks() { return undefined; }
    get videoTracks() { return undefined; }
    addTextTrack(kind, label, lang) { hc("HTMLMediaElement.addTextTrack"); const t = new TextTrack(kind, label, lang); this._textTracks._t.push(t); queueTask(() => dispatch(this._textTracks, Object.assign(new Event("addtrack"), { track: t }))); return t; }
    canPlayType(t) { hc("HTMLMediaElement.canPlayType:" + String(t).slice(0, 80)); return OP.can_play_type(String(t)); }
    play() {
      hc("HTMLMediaElement.play");
      if (this._error && this._error.code === MediaError.MEDIA_ERR_SRC_NOT_SUPPORTED) return Promise.reject(new DOMException("The element has no supported sources.", "NotSupportedError"));
      const p = new Promise((res, rej) => this._playPromises.push({ res, rej }));
      const st = this._refresh();
      if (!st.paused && st.readyState >= HTMLMediaElement.HAVE_FUTURE_DATA && !st.seeking) { const gen = this._loadGen; queueTask(() => { if (gen === this._loadGen) this._settlePlay(null); }); }
      st.paused = false;
      OP.media_play(this._native());
      return p;
    }
    pause() { hc("HTMLMediaElement.pause"); this._st.paused = true; if (this._id) OP.media_pause(this._id); }
    _mediaError(code) {
      if (this._refresh().readyState === HTMLMediaElement.HAVE_NOTHING) {
        this._error = new MediaError(MediaError.MEDIA_ERR_SRC_NOT_SUPPORTED, "The media resource could not be decoded");
        this._networkState = HTMLMediaElement.NETWORK_NO_SOURCE;
        this._settlePlay(new DOMException("The element has no supported sources.", "NotSupportedError"));
      } else {
        this._error = new MediaError(code, code === MediaError.MEDIA_ERR_DECODE ? "The media data is corrupted" : "The media data could not be fetched");
        this._networkState = HTMLMediaElement.NETWORK_IDLE;
      }
      queueTask(() => fire(this, "error"));
    }
    get mediaKeys() { return this._mediaKeys; }
    setMediaKeys(mk) {
      hc("HTMLMediaElement.setMediaKeys");
      return new Promise((res, rej) => {
        if (mk && !(mk instanceof MediaKeys)) { rej(new TypeError("not MediaKeys")); return; }
        this._mediaKeys = mk || null;
        if (this._id) OP.media_set_keys(this._id, mk ? mk._id : 0);
        queueTask(res);
      });
    }
    getStartDate() { return new Date(NaN); }
  }
  HTMLMediaElement.HAVE_NOTHING = 0; HTMLMediaElement.HAVE_METADATA = 1; HTMLMediaElement.HAVE_CURRENT_DATA = 2; HTMLMediaElement.HAVE_FUTURE_DATA = 3; HTMLMediaElement.HAVE_ENOUGH_DATA = 4;
  HTMLMediaElement.NETWORK_EMPTY = 0; HTMLMediaElement.NETWORK_IDLE = 1; HTMLMediaElement.NETWORK_LOADING = 2; HTMLMediaElement.NETWORK_NO_SOURCE = 3;
  G.HTMLMediaElement = HTMLMediaElement;
  class HTMLAudioElement extends HTMLMediaElement { constructor() { super("audio"); } }
  class HTMLVideoElement extends HTMLMediaElement { constructor() { super("video"); this.videoWidth = 0; this.videoHeight = 0; this.poster = ""; this.playsInline = false; } getVideoPlaybackQuality() { return { droppedVideoFrames: 0, totalVideoFrames: 0, corruptedVideoFrames: 0, creationTime: now() }; } }
  G.HTMLAudioElement = HTMLAudioElement;
  G.HTMLVideoElement = HTMLVideoElement;
  G.Audio = function (src) { const a = new HTMLAudioElement(); if (src) a.src = src; return a; };

  function dispatchMedia(id, payload) {
    const el = mediaById.get(id)?.deref();
    if (!el) return;
    const p = JSON.parse(payload);
    Object.assign(el._st, p.state);
    for (const type of p.events) {
      hc("event:" + type);
      if (type === "error") el._error = new MediaError(MediaError.MEDIA_ERR_DECODE, "Audio output failed");
      fire(el, type);
      if (type === "playing") el._settlePlay(null);
      else if (type === "pause") el._settlePlay(new DOMException("The play() request was interrupted by a call to pause().", "AbortError"));
    }
  }

  function mimeSupported(t) { return OP.type_supported(t); }

  class SourceBufferList extends EventTarget {
    constructor() { super(); this._l = []; }
    get length() { return this._l.length; }
    item(i) { return this._l[i] || null; }
    [Symbol.iterator]() { return this._l[Symbol.iterator](); }
  }
  G.SourceBufferList = SourceBufferList;
  function defineIndexes(list) {
    return new Proxy(list, { get(t, k, r) { if (typeof k === "string" && /^\d+$/.test(k)) return t._l[Number(k)]; const v = Reflect.get(t, k, t); return typeof v === "function" ? v.bind(t) : v; } });
  }

  class SourceBuffer extends EventTarget {
    constructor(ms, id, type) {
      super();
      this._ms = ms; this._id = id; this._type = type;
      this._updating = false;
      this._gen = 0;
      this.mode = "segments";
      this._timestampOffset = 0;
      this._appendWindow = [0, Infinity];
      this.onupdatestart = null; this.onupdate = null; this.onupdateend = null; this.onerror = null; this.onabort = null;
    }
    get updating() { return this._updating; }
    _checkSettable(what) {
      if (!this._ms) throw new DOMException("SourceBuffer has been removed", "InvalidStateError");
      if (this._updating) throw new DOMException("Cannot set " + what + " while the SourceBuffer is updating", "InvalidStateError");
    }
    get timestampOffset() { return this._timestampOffset; }
    set timestampOffset(v) {
      hc("SourceBuffer.timestampOffset=");
      v = Number(v);
      if (!isFinite(v)) throw new TypeError("timestampOffset must be finite");
      this._checkSettable("timestampOffset");
      this._ms._reopen();
      this._timestampOffset = v;
    }
    get appendWindowStart() { return this._appendWindow[0]; }
    set appendWindowStart(v) {
      hc("SourceBuffer.appendWindowStart=");
      this._checkSettable("appendWindowStart");
      v = Number(v);
      if (!(v >= 0) || v >= this._appendWindow[1]) throw new TypeError("appendWindowStart must be in [0, appendWindowEnd)");
      this._setAppendWindow(v, this._appendWindow[1]);
    }
    get appendWindowEnd() { return this._appendWindow[1]; }
    set appendWindowEnd(v) {
      hc("SourceBuffer.appendWindowEnd=");
      this._checkSettable("appendWindowEnd");
      v = Number(v);
      if (isNaN(v) || v <= this._appendWindow[0]) throw new TypeError("appendWindowEnd must be greater than appendWindowStart");
      this._setAppendWindow(this._appendWindow[0], v);
    }
    _setAppendWindow(start, end) { this._appendWindow = [start, end]; OP.sb_set_append_window(this._id, start, end); }
    get buffered() { hc("SourceBuffer.buffered"); return new TimeRanges(JSON.parse(OP.sb_buffered(this._id))); }
    get audioTracks() { return undefined; }
    appendBuffer(data) {
      hc("SourceBuffer.appendBuffer");
      if (this._updating) throw new DOMException("SourceBuffer is updating", "InvalidStateError");
      if (!this._ms || this._ms._readyState === "closed") throw new DOMException("MediaSource is closed", "InvalidStateError");
      this._ms._reopen();
      const ab = toAB(data);
      if (!OP.sb_prepare_append(this._id, ab.byteLength)) throw new DOMException("The SourceBuffer is full and cannot free space to append additional buffers.", "QuotaExceededError");
      this._updating = true;
      const gen = ++this._gen;
      queueTask(() => fire(this, "updatestart"));
      queueTask(() => {
        if (gen !== this._gen) return;
        const r = JSON.parse(OP.sb_append(this._id, ab, this.timestampOffset));
        if (r.error) {
          OP.print("warn", "appendBuffer error: " + r.error);
          this._updating = false;
          fire(this, "error");
          fire(this, "updateend");
          if (this._ms) this._ms.endOfStream("decode");
          return;
        }
        if (r.init && this._ms && isNaN(this._ms._duration)) this._ms._durationChange(r.initDuration === null ? Infinity : r.initDuration);
        if (r.frames && r.end !== null && this._ms && r.end > this._ms._duration) this._ms._durationChange(r.end);
        if (r.hasInitData) {
          const media = this._ms && this._ms._media;
          const initData = OP.sb_init_data(this._id);
          if (media && initData) queueTask(() => { hc("event:encrypted"); dispatch(media, new MediaEncryptedEvent("encrypted", { initDataType: "cenc", initData })); });
        }
        this._updating = false;
        fire(this, "update");
        fire(this, "updateend");
      });
    }
    remove(start, end) {
      hc("SourceBuffer.remove");
      if (!this._ms) throw new DOMException("SourceBuffer has been removed", "InvalidStateError");
      if (this._updating) throw new DOMException("SourceBuffer is updating", "InvalidStateError");
      start = Number(start); end = Number(end);
      const d = this._ms._duration;
      if (isNaN(d) || !(start >= 0) || start > d || !(end > start)) throw new TypeError("Invalid remove range");
      this._ms._reopen();
      this._updating = true;
      queueTask(() => {
        fire(this, "updatestart");
        OP.sb_remove(this._id, start, end);
        this._updating = false;
        fire(this, "update");
        fire(this, "updateend");
      });
    }
    abort() {
      hc("SourceBuffer.abort");
      this._gen++;
      OP.sb_abort(this._id);
      if (this._updating) { this._updating = false; queueTask(() => fire(this, "abort")); queueTask(() => fire(this, "updateend")); }
      this._setAppendWindow(0, Infinity);
    }
    changeType(type) {
      hc("SourceBuffer.changeType");
      if (!OP.sb_change_type(this._id, String(type))) throw new DOMException("Unsupported type " + type, "NotSupportedError");
      this._type = String(type);
    }
  }
  G.SourceBuffer = SourceBuffer;

  class MediaSource extends EventTarget {
    constructor() {
      super();
      hc("MediaSource.constructor");
      this._id = OP.ms_create();
      nativeReaper.register(this, ["ms", this._id]);
      this._readyState = "closed";
      this._duration = NaN;
      this._sbl = new SourceBufferList();
      this.sourceBuffers = defineIndexes(this._sbl);
      this.activeSourceBuffers = this.sourceBuffers;
      this._media = null;
      this.onsourceopen = null; this.onsourceended = null; this.onsourceclose = null;
    }
    static isTypeSupported(t) { hc("MediaSource.isTypeSupported"); return mimeSupported(String(t)); }
    get readyState() { return this._readyState; }
    get duration() { return this._duration; }
    set duration(d) {
      hc("MediaSource.duration=");
      if (this._readyState !== "open") throw new DOMException("MediaSource not open", "InvalidStateError");
      this._durationChange(Number(d));
    }
    _durationChange(d) {
      if (d === this._duration) return;
      this._duration = d;
      OP.ms_set_duration(this._id, d);
      if (this._media) { const m = this._media; queueTask(() => fire(m, "durationchange")); }
    }
    addSourceBuffer(type) {
      hc("MediaSource.addSourceBuffer");
      if (!mimeSupported(String(type))) throw new DOMException("Unsupported type " + type, "NotSupportedError");
      if (this._readyState !== "open") throw new DOMException("MediaSource not open", "InvalidStateError");
      const id = OP.sb_add(this._id, String(type));
      if (id < 0) throw new DOMException("Unsupported type " + type, "NotSupportedError");
      const sb = new SourceBuffer(this, id, String(type));
      this._sbl._l.push(sb);
      queueTask(() => fire(this._sbl, "addsourcebuffer"));
      return sb;
    }
    removeSourceBuffer(sb) {
      hc("MediaSource.removeSourceBuffer");
      const i = this._sbl._l.indexOf(sb);
      if (i < 0) throw new DOMException("not found", "NotFoundError");
      if (sb._updating) sb.abort();
      this._sbl._l.splice(i, 1);
      OP.sb_remove_buffer(this._id, sb._id);
      sb._ms = null;
      queueTask(() => fire(this._sbl, "removesourcebuffer"));
    }
    endOfStream(err) {
      hc("MediaSource.endOfStream");
      if (err !== undefined && err !== "network" && err !== "decode") throw new TypeError("Invalid endOfStream error " + err);
      if (this._readyState !== "open") throw new DOMException("MediaSource not open", "InvalidStateError");
      if (this._sbl._l.some(s => s._updating)) throw new DOMException("SourceBuffer updating", "InvalidStateError");
      this._readyState = "ended";
      if (err) { if (this._media) this._media._mediaError(err === "decode" ? MediaError.MEDIA_ERR_DECODE : MediaError.MEDIA_ERR_NETWORK); }
      else {
        OP.ms_end(this._id, true);
        let maxEnd = 0;
        for (const sb of this._sbl._l) { const r = JSON.parse(OP.sb_buffered(sb._id)); if (r.length) maxEnd = Math.max(maxEnd, r[r.length - 1][1]); }
        if (maxEnd > 0 && (!(this._duration >= maxEnd) || Math.abs(this._duration - maxEnd) > 0.001)) { this._duration = maxEnd; OP.ms_set_duration(this._id, maxEnd); if (this._media) { const m = this._media; queueTask(() => fire(m, "durationchange")); } }
      }
      queueTask(() => fire(this, "sourceended"));
    }
    setLiveSeekableRange() {}
    clearLiveSeekableRange() {}
    _reopen() { if (this._readyState === "ended") { this._readyState = "open"; OP.ms_end(this._id, false); queueTask(() => fire(this, "sourceopen")); } }
    _attach(media) { this._media = media; queueTask(() => { this._readyState = "open"; fire(this, "sourceopen"); }); }
    _detach() {
      this._media = null;
      this._readyState = "closed";
      for (const sb of this._sbl._l.splice(0)) {
        sb._gen++; sb._updating = false; sb._ms = null;
        OP.sb_remove_buffer(this._id, sb._id);
        queueTask(() => fire(this._sbl, "removesourcebuffer"));
      }
      queueTask(() => fire(this, "sourceclose"));
    }
    _buffered() {
      let ranges = null;
      for (const sb of this._sbl._l) {
        const r = JSON.parse(OP.sb_buffered(sb._id));
        if (ranges === null) ranges = r;
        else { const out = []; for (const [a, b] of ranges) for (const [c, d] of r) { const s = Math.max(a, c), e = Math.min(b, d); if (e > s) out.push([s, e]); } ranges = out; }
      }
      return new TimeRanges(ranges || []);
    }
  }
  MediaSource.canConstructInDedicatedWorker = false;
  G.MediaSource = MediaSource;

  const KEY_STATUS = ["usable", "internal-error", "expired", "output-restricted", "output-downscaled", "status-pending", "released", "usable-in-future"];
  const MESSAGE_TYPE = ["license-request", "license-renewal", "license-release", "individualization-request"];
  const EXCEPTION = ["TypeError", "NotSupportedError", "InvalidStateError", "QuotaExceededError"];
  const eme = { accessRequests: 0, accessGranted: 0, sessions: 0, messages: 0, updates: 0, keyStatusEvents: [], usableSeen: false, rejects: [] };
  const pendingCdmInit = new Map();
  const keysById = new Map();
  const promises = new Map();
  let promiseSeq = 100;
  function hexToBytes(h) { const u = new Uint8Array(h.length / 2); for (let i = 0; i < u.length; i++) u[i] = parseInt(h.substr(i * 2, 2), 16); return u; }

  class MediaKeyStatusMap {
    constructor() { this._m = new Map(); }
    get size() { return this._m.size; }
    _hex(kid) { return Array.from(toU8(kid), b => b.toString(16).padStart(2, "0")).join(""); }
    has(kid) { return this._m.has(this._hex(kid)); }
    get(kid) { const e = this._m.get(this._hex(kid)); return e ? e.status : undefined; }
    forEach(cb, thisArg) { for (const e of this._m.values()) cb.call(thisArg, e.status, e.kid, this); }
    entries() { return Array.from(this._m.values(), e => [e.kid, e.status])[Symbol.iterator](); }
    keys() { return Array.from(this._m.values(), e => e.kid)[Symbol.iterator](); }
    values() { return Array.from(this._m.values(), e => e.status)[Symbol.iterator](); }
    [Symbol.iterator]() { return this.entries(); }
  }
  G.MediaKeyStatusMap = MediaKeyStatusMap;

  class MediaKeySystemAccess {
    constructor(keySystem, cfg) { this.keySystem = keySystem; this._cfg = cfg; }
    getConfiguration() { hc("MediaKeySystemAccess.getConfiguration"); return JSON.parse(JSON.stringify(this._cfg)); }
    createMediaKeys() {
      hc("MediaKeySystemAccess.createMediaKeys");
      return new Promise((res, rej) => {
        const id = OP.cdm_create(this._cfg.persistentState === "required");
        if (id < 0) { rej(new DOMException("CDM unavailable", "InvalidStateError")); return; }
        pendingCdmInit.set(id, { res: () => res(new MediaKeys(id)), rej });
      });
    }
  }
  G.MediaKeySystemAccess = MediaKeySystemAccess;

  class MediaKeys {
    constructor(id) { this._id = id; this._sessions = new Map(); keysById.set(id, new WeakRef(this)); nativeReaper.register(this, ["cdm", id]); }
    createSession(type) {
      hc("MediaKeys.createSession");
      type = type || "temporary";
      if (type !== "temporary") throw new DOMException("Only temporary sessions", "NotSupportedError");
      eme.sessions++;
      return new MediaKeySession(this, type);
    }
    setServerCertificate(cert) {
      hc("MediaKeys.setServerCertificate");
      return new Promise((res, rej) => {
        if (!(cert instanceof ArrayBuffer || ArrayBuffer.isView(cert))) throw new TypeError("The serverCertificate is not a BufferSource.");
        const bytes = toAB(cert);
        if (bytes.byteLength === 0) throw new TypeError("The serverCertificate is empty.");
        const pid = promiseSeq++;
        promises.set(pid, { res: () => res(true), rej });
        OP.cdm_set_cert(this._id, pid, bytes);
      });
    }
    getStatusForPolicy() { return Promise.reject(new DOMException("not supported", "NotSupportedError")); }
  }
  G.MediaKeys = MediaKeys;

  class MediaKeySession extends EventTarget {
    constructor(keys, type) {
      super();
      this._keys = keys;
      this._type = type;
      this.sessionId = "";
      this.expiration = NaN;
      this._ks = new MediaKeyStatusMap();
      this._state = "new";
      this.closed = new Promise(r => { this._closedResolve = r; });
      this.onmessage = null; this.onkeystatuseschange = null;
    }
    get keyStatuses() { return this._ks; }
    generateRequest(initDataType, initData) {
      hc("MediaKeySession.generateRequest");
      const t = { cenc: 0, keyids: 1, webm: 2 }[initDataType];
      if (this._state !== "new") return Promise.reject(new DOMException("The session is already initialized or closed.", "InvalidStateError"));
      if (t === undefined) return Promise.reject(new DOMException("Unsupported initDataType " + initDataType, "NotSupportedError"));
      this._state = "pending";
      return new Promise((res, rej) => {
        const pid = promiseSeq++;
        promises.set(pid, { res, rej: e => { this._state = "new"; rej(e); }, session: this });
        OP.cdm_generate(this._keys._id, pid, t, toAB(initData));
      });
    }
    load() { return Promise.reject(new DOMException("persistent sessions unsupported", "NotSupportedError")); }
    update(response) {
      hc("MediaKeySession.update");
      if (this._state !== "open") return Promise.reject(new DOMException("The session is not callable.", "InvalidStateError"));
      if (!response || toU8(response).length === 0) return Promise.reject(new TypeError("The response is empty."));
      eme.updates++;
      return new Promise((res, rej) => { const pid = promiseSeq++; promises.set(pid, { res: () => res(), rej }); OP.cdm_update(this._keys._id, pid, this.sessionId, toAB(response)); });
    }
    close() {
      hc("MediaKeySession.close");
      if (this._state === "closing" || this._state === "closed") return Promise.resolve();
      if (this._state !== "open") return Promise.reject(new DOMException("The session is not callable.", "InvalidStateError"));
      this._state = "closing";
      return new Promise((res, rej) => { const pid = promiseSeq++; promises.set(pid, { res: () => res(), rej }); OP.cdm_close(this._keys._id, pid, this.sessionId); });
    }
    remove() {
      hc("MediaKeySession.remove");
      if (this._state !== "open") return Promise.reject(new DOMException("The session is not callable.", "InvalidStateError"));
      return new Promise((res, rej) => { const pid = promiseSeq++; promises.set(pid, { res: () => res(), rej }); OP.cdm_remove(this._keys._id, pid, this.sessionId); });
    }
  }
  G.MediaKeySession = MediaKeySession;

  const pendingBySid = new Map();
  function deliverMessage(session, kind, body) {
    eme.messages++;
    queueTask(() => { hc("event:message"); dispatch(session, new MediaKeyMessageEvent("message", { messageType: MESSAGE_TYPE[kind] || "license-request", message: body })); });
  }
  function dispatchCdm(keysId, metaJson, body) {
    const m = JSON.parse(metaJson);
    const keys = keysById.get(keysId)?.deref();
    switch (m.type) {
      case "initialized": {
        const p = pendingCdmInit.get(keysId);
        pendingCdmInit.delete(keysId);
        if (p && m.ok) p.res();
        else if (p) { OP.destroy("cdm", keysId); p.rej(new DOMException("CDM initialization failed", "InvalidStateError")); }
        break;
      }
      case "newSession": {
        const p = promises.get(m.promise);
        promises.delete(m.promise);
        if (p && p.session) {
          p.session.sessionId = m.sid;
          p.session._state = "open";
          keys._sessions.set(m.sid, p.session);
          p.res();
          const pend = pendingBySid.get(m.sid) || [];
          pendingBySid.delete(m.sid);
          for (const [k, b] of pend) deliverMessage(p.session, k, b);
        }
        break;
      }
      case "resolve": { const p = promises.get(m.promise); promises.delete(m.promise); if (p) p.res(); break; }
      case "reject": {
        const p = promises.get(m.promise);
        promises.delete(m.promise);
        eme.rejects.push(m.message);
        if (p) p.rej(m.exception === 0 ? new TypeError(m.message) : new DOMException(m.message, EXCEPTION[m.exception] || "InvalidStateError"));
        break;
      }
      case "message": {
        const s = keys && keys._sessions.get(m.sid);
        if (s) deliverMessage(s, m.kind, body);
        else { if (!pendingBySid.has(m.sid)) pendingBySid.set(m.sid, []); pendingBySid.get(m.sid).push([m.kind, body]); }
        break;
      }
      case "keys": {
        const s = keys && keys._sessions.get(m.sid);
        if (!s) break;
        s._ks._m.clear();
        const seen = [];
        for (const k of m.keys) {
          const status = KEY_STATUS[k.status] || "internal-error";
          s._ks._m.set(k.kid, { kid: hexToBytes(k.kid).buffer, status });
          seen.push(k.kid.slice(0, 8) + ":" + status);
          if (status === "usable") eme.usableSeen = true;
        }
        if (eme.keyStatusEvents.length < 20) eme.keyStatusEvents.push(seen);
        queueTask(() => { hc("event:keystatuseschange"); fire(s, "keystatuseschange"); });
        break;
      }
      case "expiration": { const s = keys && keys._sessions.get(m.sid); if (s) s.expiration = m.expiry > 0 ? m.expiry * 1000 : NaN; break; }
      case "closed": {
        const s = keys && keys._sessions.get(m.sid);
        if (!s || s._state === "closed") break;
        s._state = "closed";
        keys._sessions.delete(m.sid);
        if (s._ks._m.size) { s._ks._m.clear(); queueTask(() => fire(s, "keystatuseschange")); }
        s.expiration = NaN;
        s._closedResolve();
        break;
      }
    }
  }

  const httpPending = new Map();
  let httpSeq = 1;
  const xhrStats = { opened: 0, completed: 0, errors: 0, posts: 0, statuses: {}, endpoints: {} };
  function noteEndpoint(method, url, outcome) {
    const u = parseUrl(url);
    const web = u && (u.protocol === "https:" || u.protocol === "http:");
    const key = method + " " + (web ? u.protocol + "//" + u.hostname + u.pathname.replace(/[^/]{40,}/g, "…") : u ? u.protocol : "?");
    if (!(key in xhrStats.endpoints) && Object.keys(xhrStats.endpoints).length >= 100) return;
    const e = xhrStats.endpoints[key] || (xhrStats.endpoints[key] = { sent: 0, statuses: {} });
    if (outcome === undefined) e.sent++;
    else e.statuses[outcome] = (e.statuses[outcome] || 0) + 1;
  }
  function dataUrl(url) {
    const m = /^data:([^,]*?)(;base64)?,(.*)$/s.exec(url);
    if (!m) return null;
    const body = m[2] ? toAB(Uint8Array.from(atob(m[3]), c => c.charCodeAt(0))) : toAB(decodeURIComponent(m[3]));
    return { meta: { status: 200, statusText: "OK", url, headers: [["content-type", m[1] || "text/plain"]], error: null }, body };
  }
  function parseHeaders(list) { const h = new Headers(); for (const [k, v] of list || []) h.append(k, v); return h; }

  class Headers {
    constructor(init) {
      this._m = new Map();
      if (init instanceof Headers) init.forEach((v, k) => this.append(k, v));
      else if (Array.isArray(init)) for (const [k, v] of init) this.append(k, v);
      else if (init && typeof init === "object") for (const k of Object.keys(init)) this.append(k, init[k]);
    }
    append(k, v) { k = String(k).toLowerCase(); v = String(v); this._m.set(k, this._m.has(k) ? this._m.get(k) + ", " + v : v); }
    set(k, v) { this._m.set(String(k).toLowerCase(), String(v)); }
    get(k) { const v = this._m.get(String(k).toLowerCase()); return v === undefined ? null : v; }
    has(k) { return this._m.has(String(k).toLowerCase()); }
    delete(k) { this._m.delete(String(k).toLowerCase()); }
    forEach(cb, t) { for (const [k, v] of this._m) cb.call(t, v, k, this); }
    entries() { return this._m.entries(); }
    keys() { return this._m.keys(); }
    values() { return this._m.values(); }
    [Symbol.iterator]() { return this._m.entries(); }
  }
  G.Headers = Headers;

  class AbortSignal extends EventTarget {
    constructor() { super(); this.aborted = false; this.reason = undefined; this.onabort = null; }
    throwIfAborted() { if (this.aborted) throw this.reason; }
    static abort(reason) { const c = new AbortController(); c.abort(reason); return c.signal; }
    static timeout(ms) { const c = new AbortController(); setTimeoutImpl(() => c.abort(new DOMException("timeout", "TimeoutError")), ms, [], false); return c.signal; }
  }
  class AbortController {
    constructor() { this.signal = new AbortSignal(); }
    abort(reason) { if (this.signal.aborted) return; this.signal.aborted = true; this.signal.reason = reason === undefined ? new DOMException("aborted", "AbortError") : reason; fire(this.signal, "abort"); }
  }
  G.AbortSignal = AbortSignal;
  G.AbortController = AbortController;

  class Request {
    constructor(input, init) {
      init = init || {};
      if (input instanceof Request) { this.url = input.url; this.method = input.method; this.headers = new Headers(input.headers); this._body = input._body; this.signal = input.signal; }
      else { this.url = resolveUrl(String(input), LOCATION_HREF); this.method = "GET"; this.headers = new Headers(); this._body = null; this.signal = null; }
      if (init.method) this.method = String(init.method).toUpperCase();
      if (init.headers) this.headers = new Headers(init.headers);
      if (init.body !== undefined) this._body = init.body;
      if (init.signal) this.signal = init.signal;
      this.credentials = init.credentials || "same-origin";
      this.mode = init.mode || "cors";
      this.cache = init.cache || "default";
    }
    clone() { return new Request(this); }
  }
  G.Request = Request;
  class Response {
    constructor(body, init) {
      init = init || {};
      this._u8 = body == null ? new Uint8Array(0) : body instanceof Blob ? body._u8 : toU8(body);
      this.status = init.status === undefined ? 200 : init.status;
      this.statusText = init.statusText || "";
      this.headers = init.headers instanceof Headers ? init.headers : new Headers(init.headers);
      this.url = init.url || "";
      this.redirected = false;
      this.type = "basic";
      this.bodyUsed = false;
      this.body = null;
    }
    get ok() { return this.status >= 200 && this.status < 300; }
    arrayBuffer() { this.bodyUsed = true; return Promise.resolve(this._u8.slice().buffer); }
    text() { this.bodyUsed = true; return Promise.resolve(utf8Decode(this._u8)); }
    json() { return this.text().then(JSON.parse); }
    blob() { this.bodyUsed = true; return Promise.resolve(new Blob([this._u8], { type: this.headers.get("content-type") || "" })); }
    clone() { return new Response(this._u8.slice(), { status: this.status, statusText: this.statusText, headers: new Headers(this.headers), url: this.url }); }
  }
  G.Response = Response;

  G.fetch = function (input, init) {
    hc("fetch");
    return new Promise((res, rej) => {
      let req;
      try { req = new Request(input, init); } catch (e) { rej(new TypeError(String(e))); return; }
      if (req.signal && req.signal.aborted) { rej(req.signal.reason); return; }
      const id = httpSeq++;
      httpPending.set(id, { kind: "fetch", res, rej, req });
      if (req.signal) req.signal.addEventListener("abort", () => { if (httpPending.has(id)) { httpPending.delete(id); OP.http_abort(id); rej(req.signal.reason); } });
      const headers = []; req.headers.forEach((v, k) => headers.push([k, v]));
      xhrStats.opened++;
      noteEndpoint(req.method, req.url);
      const d = dataUrl(req.url);
      if (d) { hc("fetch:data-url"); queueTask(() => dispatchHttp(id, JSON.stringify(d.meta), d.body)); return; }
      OP.http(id, req.method, req.url, JSON.stringify(headers), req._body == null ? undefined : toAB(req._body), 0, req.cache);
    });
  };

  class XMLHttpRequestEventTarget extends EventTarget {}
  class XMLHttpRequest extends XMLHttpRequestEventTarget {
    constructor() {
      super();
      this.readyState = 0; this.status = 0; this.statusText = ""; this.responseType = ""; this._response = null; this.responseURL = ""; this.timeout = 0; this.withCredentials = false;
      this.onreadystatechange = null; this.onload = null; this.onerror = null; this.ontimeout = null; this.onabort = null; this.onprogress = null; this.onloadstart = null; this.onloadend = null;
      this._headers = []; this._respHeaders = new Headers(); this._id = 0; this._timer = 0;
      this.upload = new XMLHttpRequestEventTarget();
    }
    open(method, url) { hc("XMLHttpRequest.open"); this._method = String(method).toUpperCase(); this._url = resolveUrl(String(url), LOCATION_HREF); this._headers = []; this.readyState = 1; this._response = null; this.status = 0; fire(this, "readystatechange"); }
    setRequestHeader(k, v) { hc("XMLHttpRequest.setRequestHeader"); this._headers.push([String(k), String(v)]); }
    overrideMimeType() {}
    send(body) {
      hc("XMLHttpRequest.send");
      if (this.readyState !== 1) throw new DOMException("not opened", "InvalidStateError");
      const id = this._id = httpSeq++;
      httpPending.set(id, { kind: "xhr", xhr: this });
      xhrStats.opened++;
      noteEndpoint(this._method, this._url);
      if (this._method === "POST") xhrStats.posts++;
      if (this.timeout > 0) this._timer = setTimeoutImpl(() => { if (httpPending.has(id)) { httpPending.delete(id); OP.http_abort(id); this.readyState = 4; fire(this, "readystatechange"); dispatch(this, new ProgressEvent("timeout")); dispatch(this, new ProgressEvent("loadend")); } }, this.timeout, [], false);
      const d = dataUrl(this._url);
      if (d) { hc("XMLHttpRequest:data-url"); queueTask(() => dispatchHttp(id, JSON.stringify(d.meta), d.body)); }
      else OP.http(id, this._method, this._url, JSON.stringify(this._headers), body == null ? undefined : toAB(body), this.timeout, "default");
      dispatch(this, new ProgressEvent("loadstart"));
    }
    abort() {
      hc("XMLHttpRequest.abort");
      if (this._id && httpPending.has(this._id)) { httpPending.delete(this._id); OP.http_abort(this._id); timers.delete(this._timer); this.readyState = 4; fire(this, "readystatechange"); dispatch(this, new ProgressEvent("abort")); dispatch(this, new ProgressEvent("loadend")); }
      this.readyState = 0;
    }
    getResponseHeader(k) { return this.readyState >= 2 ? this._respHeaders.get(k) : null; }
    getAllResponseHeaders() { let s = ""; this._respHeaders.forEach((v, k) => { s += k + ": " + v + "\r\n"; }); return s; }
    get response() { return this._response; }
    get responseText() { if (this.responseType !== "" && this.responseType !== "text") throw new DOMException("responseText unavailable", "InvalidStateError"); return this._response == null ? "" : this._response; }
    get responseXML() { return null; }
    _complete(meta, body) {
      timers.delete(this._timer);
      if (meta.error) {
        xhrStats.errors++;
        this.readyState = 4; this.status = 0;
        fire(this, "readystatechange");
        dispatch(this, new ProgressEvent("error"));
        dispatch(this, new ProgressEvent("loadend"));
        return;
      }
      xhrStats.completed++;
      xhrStats.statuses[meta.status] = (xhrStats.statuses[meta.status] || 0) + 1;
      this.status = meta.status; this.statusText = meta.statusText; this.responseURL = meta.url;
      this._respHeaders = parseHeaders(meta.headers);
      this.readyState = 2; fire(this, "readystatechange");
      this.readyState = 3; fire(this, "readystatechange");
      const len = body ? body.byteLength : 0;
      dispatch(this, new ProgressEvent("progress", { lengthComputable: true, loaded: len, total: len }));
      const u8 = body ? new Uint8Array(body) : new Uint8Array(0);
      switch (this.responseType) {
        case "arraybuffer": this._response = body || new ArrayBuffer(0); break;
        case "json": try { this._response = JSON.parse(utf8Decode(u8)); } catch (e) { this._response = null; } break;
        case "blob": this._response = new Blob([u8], { type: this._respHeaders.get("content-type") || "" }); break;
        default: this._response = utf8Decode(u8);
      }
      this.readyState = 4; fire(this, "readystatechange");
      dispatch(this, new ProgressEvent("load", { lengthComputable: true, loaded: len, total: len }));
      dispatch(this, new ProgressEvent("loadend", { lengthComputable: true, loaded: len, total: len }));
    }
  }
  XMLHttpRequest.UNSENT = 0; XMLHttpRequest.OPENED = 1; XMLHttpRequest.HEADERS_RECEIVED = 2; XMLHttpRequest.LOADING = 3; XMLHttpRequest.DONE = 4;
  G.XMLHttpRequest = XMLHttpRequest;
  G.XMLHttpRequestEventTarget = XMLHttpRequestEventTarget;

  function dispatchHttp(id, metaJson, body) {
    const p = httpPending.get(id);
    if (!p) return;
    httpPending.delete(id);
    const meta = JSON.parse(metaJson);
    const outcome = meta.error ? "error" : meta.status;
    if (p.kind === "xhr") noteEndpoint(p.xhr._method, p.xhr._url, outcome);
    else noteEndpoint(p.req.method, p.req.url, outcome);
    if (p.kind === "xhr") { p.xhr._complete(meta, body); return; }
    if (meta.error) { xhrStats.errors++; p.rej(new TypeError("Failed to fetch: " + meta.error)); return; }
    xhrStats.completed++;
    xhrStats.statuses[meta.status] = (xhrStats.statuses[meta.status] || 0) + 1;
    p.res(new Response(body ? new Uint8Array(body) : null, { status: meta.status, statusText: meta.statusText, headers: parseHeaders(meta.headers), url: meta.url }));
  }

  G.navigator = {
    userAgent: USER_AGENT,
    appName: "Bramley", appVersion: USER_AGENT.slice(USER_AGENT.indexOf("/") + 1), vendor: "", platform: "Linux x86_64", product: "Bramley",
    language: "en-US", languages: ["en-US", "en"], onLine: true, cookieEnabled: false, hardwareConcurrency: 4, maxTouchPoints: 0,
    requestMediaKeySystemAccess(keySystem, configs) {
      hc("navigator.requestMediaKeySystemAccess");
      eme.accessRequests++;
      return new Promise((res, rej) => {
        let r = "";
        try { r = OP.eme_check(String(keySystem), JSON.stringify(configs || [])); } catch (e) { r = ""; }
        if (!r) { rej(new DOMException("Unsupported keySystem or supportedConfigurations.", "NotSupportedError")); return; }
        eme.accessGranted++;
        res(new MediaKeySystemAccess(String(keySystem), JSON.parse(r)));
      });
    }
  };

  class Document extends Node {
    constructor() {
      super();
      this.readyState = "complete";
      this.visibilityState = "visible";
      this.hidden = false;
      this.cookie = "";
      this.referrer = "";
      this.title = "";
      this.characterSet = "UTF-8";
    }
    get nodeType() { return 9; }
    get documentElement() { return this._html; }
    get head() { return this._head; }
    get body() { return this._body; }
    get location() { return G.location; }
    get defaultView() { return G; }
    get currentScript() { return null; }
    createElement(tag) {
      hc("document.createElement:" + String(tag).toLowerCase());
      const t = String(tag).toLowerCase();
      switch (t) {
        case "audio": return new HTMLAudioElement();
        case "video": return new HTMLVideoElement();
        case "script": return new HTMLScriptElement("script");
        case "source": return new HTMLSourceElement("source");
        case "track": return new HTMLTrackElement("track");
        case "meta": return new HTMLMetaElement("meta");
        case "div": return new HTMLDivElement("div");
        default: return new HTMLElement(t);
      }
    }
    createElementNS(ns, tag) { return this.createElement(tag); }
    createDocumentFragment() { hc("document.createDocumentFragment"); return new DocumentFragment(); }
    createTextNode(t) { const n = new Node(); n._text = String(t); return n; }
    createEvent(type) { return new Event(""); }
    querySelector(sel) { hc("document.querySelector"); return this._html.querySelector(sel); }
    querySelectorAll(sel) { hc("document.querySelectorAll"); return this._html.querySelectorAll(sel); }
    getElementById(id) { return this._html.querySelector("#" + id); }
    getElementsByTagName(t) { return this._html.getElementsByTagName(t); }
    hasFocus() { return true; }
  }
  G.Document = Document;
  const document = new Document();
  document._html = new HTMLElement("html");
  document._head = new HTMLHeadElement("head");
  document._body = new HTMLBodyElement("body");
  document.childNodes.push(document._html);
  document._html.parentNode = document;
  document._html.appendChild(document._head);
  document._html.appendChild(document._body);
  G.document = document;
  G.HTMLDocument = Document;
  G.screen = { width: 1920, height: 1080, availWidth: 1920, availHeight: 1080, colorDepth: 24, pixelDepth: 24 };
  G.devicePixelRatio = 1;
  G.innerWidth = 1280; G.innerHeight = 720;
  G.matchMedia = function (q) { return { matches: false, media: q, addListener() {}, removeListener() {}, addEventListener() {}, removeEventListener() {} }; };
  G.getComputedStyle = function () { return {}; };
  G.isSecureContext = true;
  G.origin = ORIGIN;
  G.name = "";

  const statusExtras = {};
  function statusJson() {
    return JSON.stringify({ hostCalls, ...statusExtras, uncaught, eme, xhr: xhrStats, pendingHttp: httpPending.size, timers: timers.size, tasks: tasks.length });
  }

  Object.defineProperty(G, "__platform", { value: { runTimers, dispatchHttp, dispatchScript, dispatchCdm, dispatchMedia, statusJson, statusExtras }, enumerable: false });
})();
