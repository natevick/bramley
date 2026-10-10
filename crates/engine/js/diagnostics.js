(function () {
  "use strict";
  const G = globalThis;
  const unknown = Object.create(null);
  let unknownKeys = 0;
  const IGNORED_MISSES = new Set(["then", "toJSON", "constructor"]);
  function missLogger(owner) {
    const note = (t, k, r) => {
      if (typeof k !== "string" || IGNORED_MISSES.has(k) || Reflect.has(t, k)) return;
      const name = (typeof owner === "string" ? owner : (r && r.constructor && r.constructor.name) || "?") + "." + k;
      if (name in unknown) unknown[name]++;
      else if (unknownKeys < 400) { unknownKeys++; unknown[name] = 1; }
    };
    return new Proxy(Object.prototype, {
      get(t, k, r) { note(t, k, r); return Reflect.get(t, k, r); },
      has(t, k) { note(t, k, null); return Reflect.has(t, k); },
    });
  }
  Object.setPrototypeOf(G, missLogger("window"));
  Object.setPrototypeOf(G.navigator, missLogger("navigator"));
  Object.setPrototypeOf(G.EventTarget.prototype, missLogger(null));
  G.__platform.statusExtras.unknown = unknown;
})();
