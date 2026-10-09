(() => {
  if (window.__s2i) return;
  const E = Error, M = Map, OGOPN = Object.getOwnPropertyNames, OGOPD = Object.getOwnPropertyDescriptor, ODP = Object.defineProperty, now = Date.now;
  const rec = new M();
  const I = { rec, net: [], m3u8: [], eme: { access: [], sessions: 0, messages: [], statuses: {}, everUsable: false, firstUsableEpochMs: null, initData: [] }, mse: { types: [], schm: [], frma: [], appends: 0 }, media: null };
  ODP(window, '__s2i', { value: I, enumerable: false });
  const who = () => { const s = new E().stack || ''; return /musickit\/v3/.test(s) ? 'mk' : /hls\.js|hls\.min|\/hls\//.test(s) ? 'hls' : 'other'; };
  const note = key => {
    let r = rec.get(key);
    if (!r) { r = { n: 0, mk: 0, hls: 0, other: 0 }; rec.set(key, r); }
    r.n++;
    if (r.n <= 40 || (r.mk + r.hls === 0 && r.n <= 3000)) r[who()]++;
  };
  const SKIP = new Set(['Object', 'Function', 'Array', 'Number', 'parseFloat', 'parseInt', 'Infinity', 'NaN', 'undefined', 'Boolean', 'String', 'Symbol', 'Date', 'Promise', 'RegExp', 'Error', 'AggregateError', 'EvalError', 'RangeError', 'ReferenceError', 'SyntaxError', 'TypeError', 'URIError', 'globalThis', 'JSON', 'Math', 'Intl', 'ArrayBuffer', 'Atomics', 'Uint8Array', 'Int8Array', 'Uint16Array', 'Int16Array', 'Uint32Array', 'Int32Array', 'BigUint64Array', 'BigInt64Array', 'Float16Array', 'Float32Array', 'Float64Array', 'Uint8ClampedArray', 'BigInt', 'Map', 'Set', 'WeakMap', 'WeakSet', 'WeakRef', 'FinalizationRegistry', 'Proxy', 'Reflect', 'DataView', 'decodeURI', 'decodeURIComponent', 'encodeURI', 'encodeURIComponent', 'escape', 'unescape', 'eval', 'isFinite', 'isNaN', 'Iterator', 'SharedArrayBuffer', 'WebAssembly', 'console']);
  const wrapMembers = (label, obj) => {
    if (!obj) return;
    for (const name of OGOPN(obj)) {
      if (name === 'constructor' || name === 'prototype' || name === 'length' || name === 'name' || name.startsWith('__s2')) continue;
      if (obj === window && SKIP.has(name)) continue;
      const d = OGOPD(obj, name);
      if (!d || !d.configurable) continue;
      const key = label + '.' + name;
      if (d.get || d.set) {
        ODP(obj, name, { configurable: true, enumerable: d.enumerable, get: d.get && function () { note(key); return d.get.call(this); }, set: d.set && function (v) { note(key + '='); return d.set.call(this, v); } });
      } else if (obj === window) {
        let v = d.value;
        ODP(window, name, { configurable: true, enumerable: d.enumerable, get() { note(key); return v; }, set(nv) { v = nv; } });
      } else if (typeof d.value === 'function') {
        const f = d.value;
        const w = function (...a) { note(key + '()'); return new.target ? Reflect.construct(f, a, new.target) : f.apply(this, a); };
        ODP(w, 'name', { value: f.name }); ODP(w, 'length', { value: f.length });
        ODP(obj, name, { ...d, value: w });
      }
    }
  };
  const hex = b => Array.from(new Uint8Array(b instanceof ArrayBuffer ? b : b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength)), x => x.toString(16).padStart(2, '0')).join('');
  const bytes = b => b instanceof ArrayBuffer ? new Uint8Array(b) : new Uint8Array(b.buffer, b.byteOffset, b.byteLength);
  const fourcc = (u, i) => String.fromCharCode(u[i], u[i + 1], u[i + 2], u[i + 3]);
  const scanBoxes = u => {
    for (let i = 4; i + 12 < u.length; i++) {
      if (u[i] === 0x73 && u[i + 1] === 0x63 && u[i + 2] === 0x68 && u[i + 3] === 0x6d) { const s = fourcc(u, i + 8); if (!I.mse.schm.includes(s)) I.mse.schm.push(s); }
      if (u[i] === 0x66 && u[i + 1] === 0x72 && u[i + 2] === 0x6d && u[i + 3] === 0x61) { const s = fourcc(u, i + 4); if (!I.mse.frma.includes(s)) I.mse.frma.push(s); }
    }
  };
  const pathOf = u => { try { const x = new URL(u, location.href); return { host: x.host, path: x.pathname.length > 90 ? x.pathname.slice(0, 90) + '…' : x.pathname }; } catch { return { host: '?', path: '?' }; } };
  const scanM3u8 = (where, text) => {
    const keys = [...text.matchAll(/#EXT-X-(?:SESSION-)?KEY:([^\n]*)/g)].map(m => { const a = m[1]; return { method: (a.match(/METHOD=([^,]+)/) || [])[1], keyformat: (a.match(/KEYFORMAT="([^"]+)"/) || [])[1] }; });
    const codecs = [...text.matchAll(/CODECS="([^"]+)"/g)].map(m => m[1]);
    const groups = [...text.matchAll(/#EXT-X-MEDIA:[^\n]*GROUP-ID="([^"]+)"/g)].map(m => m[1]);
    I.m3u8.push({ where, keys: keys.slice(0, 6), codecs: [...new Set(codecs)].slice(0, 12), groups: [...new Set(groups)].slice(0, 12), bytes: text.length });
  };
  const scanJson = (p, text) => {
    if (!/webPlayback|play\/assets/i.test(p.path)) return;
    try {
      const j = JSON.parse(text);
      const flavors = [];
      const walk = o => { if (Array.isArray(o)) o.forEach(walk); else if (o && typeof o === 'object') { if (typeof o.flavor === 'string') flavors.push(o.flavor); Object.values(o).forEach(walk); } };
      walk(j);
      I.flavors = [...new Set(flavors)];
      I.webPlaybackKeys = Object.keys(j.songList?.[0] || j).slice(0, 30);
    } catch {}
  };
  const oFetch = window.fetch;
  window.fetch = function (input, init) {
    const url = typeof input === 'string' ? input : input && input.url;
    const p = pathOf(url);
    const entry = { t: now(), kind: 'fetch', method: (init && init.method) || (input && input.method) || 'GET', ...p, by: who() };
    I.net.push(entry);
    return oFetch.apply(this, arguments).then(r => {
      entry.status = r.status;
      if (/\.m3u8$/i.test(p.path)) r.clone().text().then(t => scanM3u8(p.path.split('/').pop(), t)).catch(() => {});
      if (/webPlayback|play\/assets/i.test(p.path)) r.clone().text().then(t => scanJson(p, t)).catch(() => {});
      return r;
    }, e => { entry.error = String(e).slice(0, 120); throw e; });
  };
  const oOpen = XMLHttpRequest.prototype.open;
  XMLHttpRequest.prototype.open = function (method, url) {
    const p = pathOf(url);
    const entry = { t: now(), kind: 'xhr', method, ...p, by: who() };
    I.net.push(entry);
    this.addEventListener('load', () => {
      entry.status = this.status;
      try { if (/\.m3u8$/i.test(p.path) && (this.responseType === '' || this.responseType === 'text')) scanM3u8(p.path.split('/').pop(), this.responseText); } catch {}
      try { if (/webPlayback|play\/assets/i.test(p.path) && (this.responseType === '' || this.responseType === 'text' || this.responseType === 'json')) scanJson(p, this.responseType === 'json' ? JSON.stringify(this.response) : this.responseText); } catch {}
    });
    return oOpen.apply(this, arguments);
  };
  const oRMKSA = Navigator.prototype.requestMediaKeySystemAccess;
  Navigator.prototype.requestMediaKeySystemAccess = function (ks, configs) {
    const entry = { keySystem: ks, configs: JSON.parse(JSON.stringify(configs || [])) };
    I.eme.access.push(entry);
    return oRMKSA.apply(this, arguments).then(a => { entry.granted = true; try { entry.granted = a.getConfiguration(); } catch {} return a; }, e => { entry.error = String(e).slice(0, 160); throw e; });
  };
  const oCreate = MediaKeys.prototype.createSession;
  MediaKeys.prototype.createSession = function () {
    const s = oCreate.apply(this, arguments);
    I.eme.sessions++;
    s.addEventListener('keystatuseschange', () => {
      s.keyStatuses.forEach((st, kid) => { I.eme.statuses[hex(kid)] = st; if (st === 'usable' && !I.eme.everUsable) { I.eme.everUsable = true; I.eme.firstUsableEpochMs = now(); } });
    });
    s.addEventListener('message', ev => I.eme.messages.push({ t: now(), type: ev.messageType, bytes: ev.message.byteLength }));
    return s;
  };
  const oGen = MediaKeySession.prototype.generateRequest;
  MediaKeySession.prototype.generateRequest = function (type, data) {
    try {
      const u = bytes(data);
      const sys = [];
      for (let i = 4; i + 28 < u.length; i++) if (u[i] === 0x70 && u[i + 1] === 0x73 && u[i + 2] === 0x73 && u[i + 3] === 0x68) sys.push(hex(u.slice(i + 8, i + 24)));
      I.eme.initData.push({ type, bytes: u.length, psshSystems: sys });
    } catch {}
    return oGen.apply(this, arguments);
  };
  const oAdd = MediaSource.prototype.addSourceBuffer;
  MediaSource.prototype.addSourceBuffer = function (type) { I.mse.types.push(type); return oAdd.apply(this, arguments); };
  const oAppend = SourceBuffer.prototype.appendBuffer;
  SourceBuffer.prototype.appendBuffer = function (data) { I.mse.appends++; if (I.mse.appends <= 6) { try { scanBoxes(bytes(data)); } catch {} } return oAppend.apply(this, arguments); };
  const oPlay = HTMLMediaElement.prototype.play;
  HTMLMediaElement.prototype.play = function () { I.media = this; return oPlay.apply(this, arguments); };
  if (window.__S2_NO_CONTRACT) return;
  const protos = ['EventTarget', 'Node', 'Document', 'Element', 'HTMLElement', 'HTMLMediaElement', 'HTMLAudioElement', 'HTMLVideoElement', 'HTMLScriptElement', 'HTMLLinkElement', 'HTMLHeadElement', 'HTMLIFrameElement', 'HTMLStyleElement', 'Event', 'CustomEvent', 'Navigator', 'MediaSource', 'ManagedMediaSource', 'SourceBuffer', 'SourceBufferList', 'TimeRanges', 'MediaKeys', 'MediaKeySession', 'MediaKeySystemAccess', 'MediaKeyStatusMap', 'Storage', 'Location', 'History', 'Performance', 'Response', 'Request', 'Headers', 'Blob', 'URL', 'URLSearchParams', 'MediaSession', 'MediaMetadata', 'TextTrack', 'TextTrackList', 'AudioTrackList', 'XMLHttpRequest', 'Crypto', 'DOMParser', 'MutationObserver', 'AbortController', 'AbortSignal', 'MediaCapabilities', 'Screen', 'VisibilityStateEntry', 'NetworkInformation', 'DOMTokenList', 'CSSStyleDeclaration', 'TextDecoder', 'TextEncoder'];
  for (const n of protos) { const C = window[n]; if (C && C.prototype) { wrapMembers(n, C.prototype); wrapMembers(n + '(static)', C); } }
  wrapMembers('window', window);
})();
