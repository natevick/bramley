(function () {
  const G = globalThis;
  const print = G.__host_print;
  const t0 = G.__host_t0;
  const now = () => Date.now() - t0;
  const touched = new Map();
  let order = 0;
  const OBJPROTO = new Set(Object.getOwnPropertyNames(Object.prototype));
  function note(path, kind) {
    let e = touched.get(path);
    if (!e) { e = { first: order++, count: 0, kinds: {} }; touched.set(path, e); }
    e.count++;
    e.kinds[kind] = (e.kinds[kind] || 0) + 1;
  }
  const fetches = [];
  const storageKeys = [];
  const createdTags = [];
  const scriptLoads = [];
  const consoleLines = [];
  const spike = { fetches, scriptLoads, consoleLines, point: null, configure: null, errors: [] };
  G.__spike = spike;
  function markPoint(why) {
    if (spike.point) return;
    spike.point = { why, at: now() };
    print("POINT " + JSON.stringify(spike.point));
  }
  spike.markPoint = markPoint;

  const proxyCache = new WeakMap();
  function logged(path, impl) {
    if (G.__spike_mode === "plain") return impl;
    if (impl === null || (typeof impl !== "object" && typeof impl !== "function")) return impl;
    let byPath = proxyCache.get(impl);
    if (!byPath) { byPath = new Map(); proxyCache.set(impl, byPath); }
    if (byPath.has(path)) return byPath.get(path);
    const p = new Proxy(impl, {
      get(t, k, r) {
        if (typeof k === "symbol") return Reflect.get(t, k, t);
        if (k === "prototype") return Reflect.get(t, k, t);
        const sub = path + "." + k;
        const present = k in t;
        note(sub, present ? "get" : "get-missing");
        const v = Reflect.get(t, k, t);
        if (typeof v === "function") {
          if (v.__noWrap) return v;
          return wrapFn(sub, v, t);
        }
        if (v && typeof v === "object" && !v.__noWrap) return logged(sub, v);
        return v;
      },
      has(t, k) {
        if (typeof k === "symbol") return k in t;
        const present = k in t;
        note(path + "." + k, present ? "has" : "probe-missing");
        return present;
      },
      set(t, k, v) {
        if (typeof k !== "symbol") note(path + "." + k, "set");
        t[k] = v;
        return true;
      },
      apply(t, thisArg, args) { note(path, "call"); return Reflect.apply(t, thisArg, args); },
      construct(t, args, nt) { note(path, "construct"); return Reflect.construct(t, args); }
    });
    byPath.set(path, p);
    return p;
  }
  const fnCache = new WeakMap();
  function wrapFn(path, fn, self) {
    let byPath = fnCache.get(fn);
    if (!byPath) { byPath = new Map(); fnCache.set(fn, byPath); }
    const key = path;
    if (byPath.has(key)) return byPath.get(key);
    const w = new Proxy(fn, {
      apply(t, thisArg, args) { note(path, "call"); return Reflect.apply(t, self, args); },
      construct(t, args) { note(path, "construct"); return Reflect.construct(t, args); }
    });
    byPath.set(key, w);
    return w;
  }

  let timerSeq = 1;
  const timers = new Map();
  function setTimeoutImpl(fn, ms, ...args) {
    const id = timerSeq++;
    timers.set(id, { fn, due: now() + (ms > 0 ? ms : 0), every: null, args });
    return id;
  }
  function setIntervalImpl(fn, ms, ...args) {
    const id = timerSeq++;
    const every = ms > 0 ? ms : 1;
    timers.set(id, { fn, due: now() + every, every, args });
    return id;
  }
  function clearTimerImpl(id) { timers.delete(id); }
  G.__host_tick = function () {
    const t = now();
    for (const [id, tm] of [...timers]) {
      if (!timers.has(id) || tm.due > t) continue;
      if (tm.every) tm.due = t + tm.every; else timers.delete(id);
      try { typeof tm.fn === "function" ? tm.fn(...tm.args) : (0, eval)(String(tm.fn)); }
      catch (e) { spike.errors.push("timer: " + (e && e.stack || e)); }
    }
    return timers.size;
  };

  class EventImpl {
    constructor(type, init) { this.type = String(type); this.bubbles = !!(init && init.bubbles); this.cancelable = !!(init && init.cancelable); this.defaultPrevented = false; this.target = null; this.currentTarget = null; this.timeStamp = now(); }
    preventDefault() { if (this.cancelable) this.defaultPrevented = true; }
    stopPropagation() {}
    stopImmediatePropagation() { this.__stop = true; }
  }
  class CustomEventImpl extends EventImpl {
    constructor(type, init) { super(type, init); this.detail = init && "detail" in init ? init.detail : null; }
  }
  class EventTargetImpl {
    constructor() { Object.defineProperty(this, "__listeners", { value: new Map(), enumerable: false }); }
    addEventListener(type, fn, opts) {
      if (!fn) return;
      const l = this.__listeners.get(type) || [];
      if (!l.some(x => x.fn === fn)) l.push({ fn, once: !!(opts && typeof opts === "object" && opts.once) });
      this.__listeners.set(type, l);
    }
    removeEventListener(type, fn) {
      const l = this.__listeners.get(type);
      if (l) this.__listeners.set(type, l.filter(x => x.fn !== fn));
    }
    dispatchEvent(ev) {
      try { ev.target = this; ev.currentTarget = this; } catch (e) {}
      const l = (this.__listeners.get(ev.type) || []).slice();
      for (const x of l) {
        if (x.once) this.removeEventListener(ev.type, x.fn);
        try { typeof x.fn === "function" ? x.fn.call(this, ev) : x.fn.handleEvent(ev); }
        catch (e) { spike.errors.push("listener " + ev.type + ": " + (e && e.stack || e)); }
        if (ev.__stop) break;
      }
      const h = this["on" + ev.type];
      if (typeof h === "function") { try { h.call(this, ev); } catch (e) { spike.errors.push("on" + ev.type + ": " + e); } }
      return !ev.defaultPrevented;
    }
  }

  class StorageImpl {
    constructor(name) { Object.defineProperty(this, "__m", { value: new Map(), enumerable: false }); Object.defineProperty(this, "__name", { value: name, enumerable: false }); }
    get length() { return this.__m.size; }
    key(i) { return [...this.__m.keys()][i] ?? null; }
    getItem(k) { k = String(k); storageKeys.push("get " + this.__name + " " + k); return this.__m.has(k) ? this.__m.get(k) : null; }
    setItem(k, v) { storageKeys.push("set " + this.__name + " " + String(k)); this.__m.set(String(k), String(v)); }
    removeItem(k) { storageKeys.push("remove " + this.__name + " " + String(k)); this.__m.delete(String(k)); }
    clear() { this.__m.clear(); }
  }

  const URL_RE = /^([a-zA-Z][a-zA-Z0-9+.-]*:)\/\/(?:[^@\/?#]*@)?([^\/?#:]*)(?::(\d+))?([^?#]*)(\?[^#]*)?(#.*)?$/;
  class URLSearchParamsImpl {
    constructor(init) {
      this.__l = [];
      if (typeof init === "string") {
        const s = init.startsWith("?") ? init.slice(1) : init;
        for (const part of s.split("&")) { if (!part) continue; const i = part.indexOf("="); const k = i < 0 ? part : part.slice(0, i); const v = i < 0 ? "" : part.slice(i + 1); this.__l.push([decodeURIComponent(k.replace(/\+/g, " ")), decodeURIComponent(v.replace(/\+/g, " "))]); }
      } else if (init && typeof init === "object") {
        for (const [k, v] of (Symbol.iterator in init ? init : Object.entries(init))) this.__l.push([String(k), String(v)]);
      }
    }
    get(k) { const e = this.__l.find(x => x[0] === k); return e ? e[1] : null; }
    getAll(k) { return this.__l.filter(x => x[0] === k).map(x => x[1]); }
    has(k) { return this.__l.some(x => x[0] === k); }
    set(k, v) { const i = this.__l.findIndex(x => x[0] === k); if (i < 0) this.__l.push([k, String(v)]); else { this.__l[i][1] = String(v); this.__l = this.__l.filter((x, j) => j <= i || x[0] !== k); } }
    append(k, v) { this.__l.push([String(k), String(v)]); }
    delete(k) { this.__l = this.__l.filter(x => x[0] !== k); }
    forEach(fn) { for (const [k, v] of this.__l) fn(v, k, this); }
    entries() { return this.__l.map(x => [x[0], x[1]])[Symbol.iterator](); }
    keys() { return this.__l.map(x => x[0])[Symbol.iterator](); }
    values() { return this.__l.map(x => x[1])[Symbol.iterator](); }
    [Symbol.iterator]() { return this.entries(); }
    toString() { return this.__l.map(([k, v]) => encodeURIComponent(k) + "=" + encodeURIComponent(v)).join("&"); }
  }
  class URLImpl {
    constructor(u, base) {
      u = String(u);
      if (!URL_RE.test(u)) {
        if (base === undefined) throw new TypeError("Invalid URL: " + u);
        const b = new URLImpl(base);
        if (u.startsWith("//")) u = b.protocol + u;
        else if (u.startsWith("/")) u = b.origin + u;
        else if (u.startsWith("?")) u = b.origin + b.pathname + u;
        else if (u.startsWith("#")) u = b.origin + b.pathname + b.search + u;
        else u = b.origin + b.pathname.replace(/[^\/]*$/, "") + u;
      }
      const m = URL_RE.exec(u);
      if (!m) throw new TypeError("Invalid URL: " + u);
      this.protocol = m[1].toLowerCase(); this.hostname = m[2].toLowerCase(); this.port = m[3] || "";
      this.pathname = m[4] || "/"; this.hash = m[6] || "";
      this.searchParams = new URLSearchParamsImpl(m[5] || "");
    }
    get search() { const s = this.searchParams.toString(); return s ? "?" + s : ""; }
    set search(v) { this.searchParams = new URLSearchParamsImpl(String(v)); }
    get host() { return this.hostname + (this.port ? ":" + this.port : ""); }
    get origin() { return this.protocol + "//" + this.host; }
    get href() { return this.origin + this.pathname + this.search + this.hash; }
    toString() { return this.href; }
    toJSON() { return this.href; }
  }

  class HeadersImpl {
    constructor(init) { this.__m = new Map(); if (init) { const it = init instanceof HeadersImpl ? init.entries() : (Symbol.iterator in Object(init) ? init : Object.entries(init)); for (const [k, v] of it) this.append(k, v); } }
    append(k, v) { k = String(k).toLowerCase(); const c = this.__m.get(k); this.__m.set(k, c === undefined ? String(v) : c + ", " + v); }
    set(k, v) { this.__m.set(String(k).toLowerCase(), String(v)); }
    get(k) { const v = this.__m.get(String(k).toLowerCase()); return v === undefined ? null : v; }
    has(k) { return this.__m.has(String(k).toLowerCase()); }
    delete(k) { this.__m.delete(String(k).toLowerCase()); }
    forEach(fn) { for (const [k, v] of this.__m) fn(v, k, this); }
    entries() { return this.__m.entries(); }
    keys() { return this.__m.keys(); }
    values() { return this.__m.values(); }
    [Symbol.iterator]() { return this.__m.entries(); }
  }
  class RequestImpl {
    constructor(input, init) {
      init = init || {};
      this.url = String(input instanceof RequestImpl ? input.url : input);
      this.method = (init.method || (input instanceof RequestImpl ? input.method : "GET")).toUpperCase();
      this.headers = new HeadersImpl(init.headers || (input instanceof RequestImpl ? input.headers : undefined));
      this.body = init.body ?? null;
      this.credentials = init.credentials || "same-origin";
      this.mode = init.mode || "cors";
    }
  }
  function fetchImpl(input, init) {
    const req = input instanceof RequestImpl ? input : new RequestImpl(input, init);
    const headerNames = [...req.headers.keys()];
    const rec = { at: now(), method: req.method, url: req.url, headerNames };
    fetches.push(rec);
    print("FETCH " + JSON.stringify(rec));
    markPoint("first-fetch");
    return Promise.reject(new TypeError("spike: network disabled"));
  }

  function b64tab() { return "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"; }
  function btoaImpl(s) {
    s = String(s); const t = b64tab(); let o = "";
    for (let i = 0; i < s.length; i += 3) {
      const a = s.charCodeAt(i), b = s.charCodeAt(i + 1), c = s.charCodeAt(i + 2);
      if (a > 255 || b > 255 || c > 255) throw new Error("InvalidCharacterError");
      const n = (a << 16) | ((b || 0) << 8) | (c || 0);
      o += t[(n >> 18) & 63] + t[(n >> 12) & 63] + (i + 1 < s.length ? t[(n >> 6) & 63] : "=") + (i + 2 < s.length ? t[n & 63] : "=");
    }
    return o;
  }
  function atobImpl(s) {
    s = String(s).replace(/[\s=]/g, "").replace(/-/g, "+").replace(/_/g, "/"); const t = b64tab(); let o = "";
    for (let i = 0; i < s.length; i += 4) {
      const n = (t.indexOf(s[i]) << 18) | (t.indexOf(s[i + 1]) << 12) | ((t.indexOf(s[i + 2]) & 63) << 6) | (t.indexOf(s[i + 3]) & 63);
      o += String.fromCharCode((n >> 16) & 255);
      if (i + 2 < s.length) o += String.fromCharCode((n >> 8) & 255);
      if (i + 3 < s.length) o += String.fromCharCode(n & 255);
    }
    return o;
  }
  class TextDecoderImpl {
    constructor(label) { this.encoding = (label || "utf-8").toLowerCase(); }
    decode(buf) {
      if (!buf) return "";
      const u = buf instanceof Uint8Array ? buf : new Uint8Array(buf.buffer || buf, buf.byteOffset || 0, buf.byteLength);
      let s = "", i = 0;
      while (i < u.length) {
        const c = u[i++];
        if (c < 128) s += String.fromCharCode(c);
        else if (c < 224) s += String.fromCharCode(((c & 31) << 6) | (u[i++] & 63));
        else if (c < 240) s += String.fromCharCode(((c & 15) << 12) | ((u[i++] & 63) << 6) | (u[i++] & 63));
        else { const cp = (((c & 7) << 18) | ((u[i++] & 63) << 12) | ((u[i++] & 63) << 6) | (u[i++] & 63)) - 65536; s += String.fromCharCode(55296 + (cp >> 10), 56320 + (cp & 1023)); }
      }
      return s;
    }
  }
  class TextEncoderImpl {
    get encoding() { return "utf-8"; }
    encode(s) { s = unescape(encodeURIComponent(String(s ?? ""))); const u = new Uint8Array(s.length); for (let i = 0; i < s.length; i++) u[i] = s.charCodeAt(i); return u; }
  }

  class NodeImpl extends EventTargetImpl {}
  class ElementImpl extends NodeImpl {}
  class HTMLElementImpl extends ElementImpl {}
  class DocumentImpl extends NodeImpl {}
  function makeElement(tag) {
    const el = new HTMLElementImpl();
    const attrs = new Map();
    el.tagName = String(tag).toUpperCase();
    el.nodeName = el.tagName;
    el.style = {};
    el.dataset = {};
    el.childNodes = [];
    el.children = [];
    el.setAttribute = (k, v) => { attrs.set(String(k), String(v)); };
    el.getAttribute = k => attrs.has(String(k)) ? attrs.get(String(k)) : null;
    el.hasAttribute = k => attrs.has(String(k));
    el.removeAttribute = k => { attrs.delete(String(k)); };
    el.appendChild = c => { el.childNodes.push(c); el.children.push(c); return c; };
    el.removeChild = c => { el.childNodes = el.childNodes.filter(x => x !== c); el.children = el.children.filter(x => x !== c); return c; };
    el.querySelector = () => null;
    el.querySelectorAll = () => [];
    return el;
  }
  const docTarget = new DocumentImpl();
  const head = makeElement("head");
  const body = makeElement("body");
  head.appendChild = c => {
    if (c && c.tagName === "SCRIPT") { const rec = { at: now(), src: c.src || (c.getAttribute && c.getAttribute("src")), type: c.type || (c.getAttribute && c.getAttribute("type")) || null }; scriptLoads.push(rec); print("SCRIPT " + JSON.stringify(rec)); }
    return c;
  };
  const documentImpl = Object.assign(docTarget, {
    readyState: "complete",
    head, body,
    documentElement: makeElement("html"),
    cookie: "",
    referrer: "",
    visibilityState: "visible",
    hidden: false,
    createElement: tag => { createdTags.push(String(tag).toLowerCase()); return logged("element." + String(tag).toLowerCase(), makeElement(tag)); },
    querySelector: () => null,
    querySelectorAll: () => [],
    getElementsByTagName: () => [],
    getElementById: () => null
  });
  const locationImpl = { href: "http://localhost/", origin: "http://localhost", protocol: "http:", host: "localhost", hostname: "localhost", port: "", pathname: "/", search: "", hash: "", toString() { return this.href; } };
  const navigatorImpl = { userAgent: "MusicClientSpike/0.1 (X11; Linux x86_64) QuickJS-or-V8-host", language: "en-US", languages: ["en-US", "en"], platform: "Linux x86_64", vendor: "", onLine: true, cookieEnabled: false };
  const consoleImpl = {};
  for (const lvl of ["log", "info", "warn", "error", "debug", "trace"]) consoleImpl[lvl] = (...a) => { const line = lvl + ": " + a.map(x => { try { return typeof x === "string" ? x : (x && x.stack) || JSON.stringify(x); } catch (e) { return String(x); } }).join(" "); if (consoleLines.length < 200) consoleLines.push(line.slice(0, 600)); };

  const hostGlobals = {
    window: G, self: G, top: G, parent: G,
    document: documentImpl, navigator: navigatorImpl, location: locationImpl,
    localStorage: new StorageImpl("local"), sessionStorage: new StorageImpl("session"),
    console: consoleImpl,
    fetch: fetchImpl, Headers: HeadersImpl, Request: RequestImpl,
    URL: URLImpl, URLSearchParams: URLSearchParamsImpl,
    setTimeout: setTimeoutImpl, clearTimeout: clearTimerImpl, setInterval: setIntervalImpl, clearInterval: clearTimerImpl,
    queueMicrotask: fn => { Promise.resolve().then(fn); },
    Event: EventImpl, CustomEvent: CustomEventImpl, EventTarget: EventTargetImpl,
    Document: DocumentImpl, HTMLElement: HTMLElementImpl,
    atob: atobImpl, btoa: btoaImpl, TextDecoder: TextDecoderImpl, TextEncoder: TextEncoderImpl,
    devicePixelRatio: 1,
    addEventListener: (t, f, o) => docTarget.addEventListener.call(winTarget, t, f, o),
    removeEventListener: (t, f) => docTarget.removeEventListener.call(winTarget, t, f),
    dispatchEvent: ev => docTarget.dispatchEvent.call(winTarget, ev)
  };
  const winTarget = new EventTargetImpl();
  const engineOwn = [];
  for (const k of Object.keys(hostGlobals).concat(["Deno", "__bootstrap"])) {
    if (Object.prototype.hasOwnProperty.call(G, k)) {
      engineOwn.push(k);
      if (k in hostGlobals) { try { delete G[k]; } catch (e) {} }
    }
  }
  spike.engineOwnHostGlobals = engineOwn;
  const PLAIN = G.__spike_mode === "plain";
  spike.mode = PLAIN ? "plain" : "logged";
  if (PLAIN) {
    for (const [k, v] of Object.entries(hostGlobals)) G[k] = v;
    spike.report = function () {
      return JSON.stringify({ mode: "plain", touched: [], fetches, scriptLoads, point: spike.point, configure: spike.configure, mk: spike.mk, apiProbe: spike.apiProbe, instState: spike.instState, refCheck: spike.refCheck, configureCalledAtMs: spike.configureCalledAtMs, storageKeys: [...new Set(storageKeys)], createdTags, errors: spike.errors.slice(0, 50), consoleLines: consoleLines.slice(0, 80), pendingTimers: timers.size, engineOwnHostGlobals: engineOwn });
    };
    spike.note = note;
    spike.now = now;
    return;
  }
  const target = Object.create(Object.prototype);
  for (const [k, v] of Object.entries(hostGlobals)) target[k] = v;
  const globalProxy = new Proxy(target, {
    has(t, k) {
      if (typeof k === "symbol") return k in t;
      if (OBJPROTO.has(k)) return true;
      const present = Object.prototype.hasOwnProperty.call(t, k);
      note("global." + k, present ? "has" : "probe-missing");
      return present;
    },
    get(t, k, r) {
      if (typeof k === "symbol" || OBJPROTO.has(k)) return Reflect.get(t, k, r);
      const present = Object.prototype.hasOwnProperty.call(t, k);
      note("global." + k, present ? "get" : "get-missing");
      const v = t[k];
      if (v === G) return v;
      if (typeof v === "function") return logged("global." + k, v);
      if (v && typeof v === "object") return logged("global." + k, v);
      return v;
    },
    set(t, k, v, r) {
      if (typeof k !== "symbol") note("global." + k, "set");
      t[k] = v;
      return true;
    }
  });
  Object.setPrototypeOf(G, globalProxy);

  spike.report = function () {
    const list = [...touched.entries()].sort((a, b) => a[1].first - b[1].first).map(([path, e]) => ({ path, count: e.count, kinds: e.kinds }));
    return JSON.stringify({ mode: "logged", touched: list, fetches, scriptLoads, point: spike.point, configure: spike.configure, mk: spike.mk, apiProbe: spike.apiProbe, instState: spike.instState, refCheck: spike.refCheck, configureCalledAtMs: spike.configureCalledAtMs, storageKeys: [...new Set(storageKeys)], createdTags, errors: spike.errors.slice(0, 50), consoleLines: consoleLines.slice(0, 80), pendingTimers: timers.size, engineOwnHostGlobals: engineOwn });
  };
  spike.note = note;
  spike.now = now;
})();
