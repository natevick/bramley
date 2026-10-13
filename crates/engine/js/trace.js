(function () {
  "use strict";
  const G = globalThis;
  const ids = new WeakMap();
  let next = 0;
  const id = o => {
    if (!o || (typeof o !== "object" && typeof o !== "function")) return String(o);
    let v = ids.get(o);
    if (!v) { v = (o.constructor && o.constructor.name || "Obj") + "#" + ++next; ids.set(o, v); }
    return v;
  };
  const show = v => {
    if (v === null || v === undefined || typeof v === "number" || typeof v === "boolean") return String(v);
    if (typeof v === "string") return v.length > 40 ? "str" + v.length : JSON.stringify(v);
    if (v instanceof ArrayBuffer) return "ab" + v.byteLength;
    if (ArrayBuffer.isView(v)) return "u8:" + v.byteLength;
    if (v instanceof G.TimeRanges) { const r = []; for (let i = 0; i < v.length; i++) r.push(v.start(i).toFixed(3) + "-" + v.end(i).toFixed(3)); return "[" + r.join(",") + "]"; }
    if (typeof v === "function") return "fn";
    if (typeof v.then === "function") return "promise";
    return id(v);
  };
  const log = (...parts) => console.log("TRACE " + G.performance.now().toFixed(3) + " " + parts.join(" "));
  const host = u => { const m = /^[a-z]+:\/\/([^/]+)/i.exec(String(u)); return (m ? m[1] : "rel") + "~" + hash(String(u)); };
  const hash = t => { let h = 0x811c9dc5; for (let i = 0; i < t.length; i++) h = Math.imul(h ^ t.charCodeAt(i), 0x01000193) >>> 0; return h.toString(16); };
  const CACHE_HEADERS = ["cache-control", "expires", "age", "etag", "last-modified", "content-type", "content-length", "vary", "pragma"];
  const cacheInfo = get => CACHE_HEADERS.map(h => [h, get(h)]).filter(([, v]) => v !== null && v !== undefined).map(([h, v]) => h + "=" + JSON.stringify(String(v).slice(0, 60))).join(" ");

  function wrapMethods(C, names) {
    for (const name of names) {
      const f = C.prototype[name];
      if (typeof f !== "function") continue;
      C.prototype[name] = function (...a) {
        log("call", id(this), name, a.map(show).join(","));
        try {
          const r = f.apply(this, a);
          const me = id(this);
          log("ret", me, name, show(r));
          if (r && typeof r.then === "function") r.then(v => log("settled", me, name, "ok", show(v)), e => log("settled", me, name, "rejected", e && e.name, e && String(e.message).slice(0, 80)));
          return r;
        } catch (e) { log("throw", id(this), name, e && e.name); throw e; }
      };
    }
  }
  function wrapAccessors(C, names) {
    for (const name of names) {
      let proto = C.prototype, d;
      while (proto && !(d = Object.getOwnPropertyDescriptor(proto, name))) proto = Object.getPrototypeOf(proto);
      if (!d || !(d.get || d.set)) continue;
      Object.defineProperty(C.prototype, name, {
        configurable: true,
        get: d.get && function () { const v = d.get.call(this); log("get", id(this), name, show(v)); return v; },
        set: d.set && function (v) { log("set", id(this), name, show(v)); try { d.set.call(this, v); } catch (e) { log("throw", id(this), name + "=", e && e.name); throw e; } },
      });
    }
  }
  const handlers = new WeakMap();
  function wrapHandlerProps(C, types) {
    for (const t of types) {
      const key = "on" + t;
      Object.defineProperty(C.prototype, key, {
        configurable: true,
        get() { const m = handlers.get(this); return m ? m.get(key) || null : null; },
        set(fn) {
          let m = handlers.get(this);
          if (!m) { m = new Map(); handlers.set(this, m); }
          if (typeof fn !== "function") { if (fn !== null && fn !== undefined) log("set", id(this), key, show(fn)); m.set(key, null); return; }
          const target = this;
          log("set", id(this), key, "fn");
          m.set(key, function (ev) { log("evt", id(target), t, "via", key); try { return fn.call(this, ev); } finally { log("evt-end", id(target), t); } });
        },
      });
    }
  }
  const wrapped = new WeakMap();
  const traced = [];
  const origAdd = G.EventTarget.prototype.addEventListener, origRemove = G.EventTarget.prototype.removeEventListener;
  G.EventTarget.prototype.addEventListener = function (type, cb, opts) {
    if (!traced.some(C => this instanceof C) || !cb) return origAdd.call(this, type, cb, opts);
    let per = wrapped.get(cb);
    if (!per) { per = new Map(); wrapped.set(cb, per); }
    const k = id(this) + "/" + type;
    let w = per.get(k);
    if (!w) {
      const target = this;
      w = function (ev) { log("evt", id(target), type); try { return typeof cb === "function" ? cb.call(this, ev) : cb.handleEvent(ev); } finally { log("evt-end", id(target), type); } };
      per.set(k, w);
    }
    log("listen", id(this), type);
    return origAdd.call(this, type, w, opts);
  };
  G.EventTarget.prototype.removeEventListener = function (type, cb, opts) {
    const per = cb && wrapped.get(cb);
    const w = per && per.get(id(this) + "/" + type);
    return origRemove.call(this, type, w || cb, opts);
  };

  const MS = G.MediaSource, SB = G.SourceBuffer, ME = G.HTMLMediaElement, XHR = G.XMLHttpRequest;
  traced.push(MS, SB, ME, XHR, G.SourceBufferList);
  wrapMethods(MS, ["addSourceBuffer", "removeSourceBuffer", "endOfStream", "setLiveSeekableRange", "clearLiveSeekableRange"]);
  wrapAccessors(MS, ["readyState", "duration", "sourceBuffers", "activeSourceBuffers"]);
  wrapHandlerProps(MS, ["sourceopen", "sourceended", "sourceclose"]);
  wrapMethods(SB, ["appendBuffer", "remove", "abort", "changeType"]);
  wrapAccessors(SB, ["updating", "buffered", "timestampOffset", "appendWindowStart", "appendWindowEnd", "mode"]);
  wrapHandlerProps(SB, ["updatestart", "update", "updateend", "error", "abort"]);
  wrapMethods(ME, ["play", "pause", "load", "setMediaKeys", "removeAttribute"]);
  wrapAccessors(ME, ["src", "currentTime", "readyState", "paused", "seeking", "ended", "buffered", "duration", "error", "networkState"]);
  wrapHandlerProps(ME, ["play", "playing", "pause", "waiting", "seeking", "seeked", "ended", "error", "canplay", "canplaythrough", "loadedmetadata", "loadeddata", "durationchange", "emptied", "stalled", "suspend", "progress", "ratechange", "volumechange", "encrypted", "waitingforkey"]);
  wrapHandlerProps(XHR, ["readystatechange", "load", "loadend", "loadstart", "error", "abort", "timeout", "progress"]);
  const MK = G.MediaKeys, MKS = G.MediaKeySession, MKSA = G.MediaKeySystemAccess;
  traced.push(MKS);
  wrapMethods(MKSA, ["createMediaKeys"]);
  wrapMethods(MK, ["createSession", "setServerCertificate"]);
  wrapMethods(MKS, ["generateRequest", "update", "close", "remove", "load"]);
  wrapHandlerProps(MKS, ["message", "keystatuseschange"]);
  const rmksa = G.navigator.requestMediaKeySystemAccess;
  G.navigator.requestMediaKeySystemAccess = function (...a) {
    log("call", "navigator", "requestMediaKeySystemAccess", show(a[0]));
    const r = rmksa.apply(this, a);
    r.then(v => log("settled", "navigator", "requestMediaKeySystemAccess", "ok", show(v)), e => log("settled", "navigator", "requestMediaKeySystemAccess", "rejected", e && e.name));
    return r;
  };
  const xopen = XHR.prototype.open, xsend = XHR.prototype.send, xabort = XHR.prototype.abort;
  XHR.prototype.open = function (m, u, ...r) { log("call", id(this), "open", m, host(u)); return xopen.call(this, m, u, ...r); };
  XHR.prototype.send = function (...a) {
    const x = this;
    log("call", id(x), "send");
    origAdd.call(x, "load", () => log("done", id(x), x.status, cacheInfo(h => x.getResponseHeader(h))));
    return xsend.apply(this, a);
  };
  XHR.prototype.abort = function (...a) { log("call", id(this), "abort"); return xabort.apply(this, a); };

  let fetches = 0;
  const realFetch = G.fetch;
  G.fetch = function (input, init) {
    const n = "fetch#" + ++fetches, url = typeof input === "string" ? input : input && input.url;
    log("call", n, "fetch", (init && init.method) || "GET", host(url));
    return realFetch.call(this, input, init).then(r => { log("resolve", n, r.status, id(r), cacheInfo(h => r.headers.get(h))); return r; }, e => { log("reject", n, e && e.name); throw e; });
  };
  for (const name of ["arrayBuffer", "json", "text"]) {
    const f = G.Response.prototype[name];
    G.Response.prototype[name] = function () {
      const me = id(this);
      log("call", me, name);
      return f.call(this).then(v => { log("resolve", me, name); return v; });
    };
  }
  log("trace installed");
})();
