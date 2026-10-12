(function () {
  "use strict";
  const C = globalThis.__bramley_player;
  delete globalThis.__bramley_player;
  const S = { phase: "start", errors: [], events: [], steps: [], signInNeeded: null, playbackError: null, applied: 0, background: 0 };
  const describe = e => e && (e.name || "") + " " + (e.message || e.description || e.errorCode || String(e));
  const err = (where, e) => {
    S.errors.push(where + ": " + describe(e) + (e && e.stack ? " @ " + e.stack : ""));
    if (S.errors.length > 30) S.errors.shift();
  };
  function seedUserTokenIntoMusicKitStorage(developerToken, userToken) {
    const claims = JSON.parse(atob(developerToken.split(".")[1].replace(/-/g, "+").replace(/_/g, "/")));
    const key = ("music." + claims.iss).toLowerCase() + ".media-user-token";
    if (userToken) localStorage.setItem(key, userToken);
    else localStorage.removeItem(key);
  }
  seedUserTokenIntoMusicKitStorage(C.developerToken, C.userToken);

  async function requireSignIn() {
    if (m.isAuthorized !== true) throw new Error("isAuthorized is false after seeding the user token");
    await m.api.music("/v1/me/storefront");
  }

  let m = null;
  let signedIn;
  const ready = new Promise(resolve => { signedIn = resolve; });
  document.addEventListener("musickitloaded", async () => {
    S.phase = "loaded";
    try {
      await MusicKit.configure({ developerToken: C.developerToken, app: { name: "Bramley", build: C.build } });
      S.phase = "configured";
    } catch (e) { err("configure", e); S.phase = "configure-failed"; return; }
    m = MusicKit.getInstance();
    m.bitrate = MusicKit.PlaybackBitrate.HIGH;
    try {
      await requireSignIn();
    } catch (e) {
      S.signInNeeded = String(e && (e.message || e.errorCode || e.name) || e);
      S.phase = "sign-in-needed";
      return;
    }
    for (const ev of ["playbackStateDidChange", "nowPlayingItemDidChange", "mediaPlaybackError", "authorizationStatusDidChange"]) {
      m.addEventListener(ev, e => { if (S.events.length < 200) S.events.push([Math.round(performance.now()), ev, String(e && (e.state ?? e.name ?? (e.item && e.item.id) ?? ""))]); });
    }
    m.addEventListener("mediaPlaybackError", e => { S.playbackError = describe(e); });
    m.addEventListener("authorizationStatusDidChange", () => signInLost());
    S.phase = "ready";
    signedIn();
  });

  const script = document.createElement("script");
  script.src = C.musickitUrl;
  script.onerror = () => { S.phase = "musickit-load-failed"; };
  S.phase = "loading-musickit";
  document.head.appendChild(script);

  function signInLost() {
    if (S.signInNeeded) return true;
    if (m.isAuthorized === true && m.previewOnly !== true) return false;
    S.signInNeeded = m.previewOnly === true ? "previewOnly is true" : "isAuthorized became false";
    S.phase = "sign-in-needed";
    Promise.resolve().then(() => m.stop()).catch(e => err("stop", e));
    return true;
  }

  let last = Promise.resolve(), seq = 0, started = 0, queueGen = 0, queueFailed = false, adding = Promise.resolve();
  const settled = p => p.then(() => {}, () => {});
  const within = (p, ms) => Promise.race([settled(p), new Promise(r => setTimeout(r, ms))]);
  const turnOf = p => within(p, C.commandWaitMs);
  const add = p => { adding = settled(p); return p; };
  const note = (name, what) => { S.steps.push([Math.round(performance.now()), name, what]); if (S.steps.length > 20) S.steps.shift(); };
  function enqueue(name, f, needsQueue) {
    const n = ++seq, prev = last;
    last = ready.then(() => turnOf(prev)).then(async () => {
      S.applied = Math.max(S.applied, n - 1);
      started = n;
      if (S.signInNeeded ? name !== "pause" : needsQueue && (queueFailed || signInLost())) return note(name, "skip");
      note(name, "start");
      try {
        if ((await f()) === false) return note(name, "superseded");
        if (started === n) { S.playbackError = null; if (name === "setQueue") queueFailed = false; }
        note(name, "done");
      } catch (e) {
        err(name, e);
        if (started === n) { S.playbackError = name + ": " + describe(e); if (name === "setQueue") queueFailed = true; }
        note(name, "fail");
      }
    }).then(() => { S.applied = Math.max(S.applied, n); });
  }
  const g = f => { try { return f(); } catch (e) { return "ERR " + e.message; } };
  const state = () => {
    const r = { phase: S.phase, errors: S.errors.slice(), events: S.events.slice(-40), steps: S.steps.slice(), applied: S.applied, signInNeeded: S.signInNeeded, playbackError: S.playbackError, musickitDefined: typeof MusicKit !== "undefined" };
    if (!m) return r;
    r.isAuthorized = g(() => m.isAuthorized);
    r.storefrontId = g(() => m.storefrontId);
    r.previewOnly = g(() => m.previewOnly);
    r.playbackState = g(() => m.playbackState);
    r.currentPlaybackTime = g(() => m.currentPlaybackTime);
    r.currentPlaybackDuration = g(() => m.currentPlaybackDuration);
    r.queuePosition = g(() => m.queue.position);
    r.queue = g(() => {
      const q = m.queue, at = Math.max(q.position, 0);
      const slot = it => it && typeof it.id === "string" ? { id: it.id, title: it.title, durationInMillis: it.attributes && it.attributes.durationInMillis } : { id: null };
      return { position: q.position, length: q.length, items: q.items.slice(at, at + C.queueShown).map(slot) };
    });
    r.background = S.background;
    r.item = g(() => m.nowPlayingItem && { id: m.nowPlayingItem.id, title: m.nowPlayingItem.title, artistName: m.nowPlayingItem.artistName, durationInMillis: m.nowPlayingItem.attributes && m.nowPlayingItem.attributes.durationInMillis });
    return r;
  };
  Object.defineProperty(globalThis, "__player", {
    value: {
      get applied() { return S.applied; },
      state,
      setQueue: options => {
        const gen = ++queueGen;
        enqueue("setQueue", async () => {
          await within(adding, 4 * C.commandWaitMs);
          return gen === queueGen ? m.setQueue(options) : false;
        }, false);
      },
      play: () => enqueue("play", () => m.play(), true),
      pause: () => enqueue("pause", () => m.pause(), false),
      seek: t => enqueue("seek", () => m.seekToTime(t), true),
      next: () => enqueue("next", () => m.skipToNextItem(), true),
      previous: () => enqueue("previous", () => m.skipToPreviousItem(), true),
      volume: v => enqueue("volume", () => { m.volume = v; }, false),
      playLater: options => enqueue("playLater", () => add(m.playLater(options)), true),
      topUp: options => ready.then(() => S.signInNeeded || queueFailed ? null : add(m.playLater(options)))
        .catch(e => err("topUp", e)).then(() => { S.background++; }),
      shuffle: on => enqueue("shuffle", () => { m.shuffleMode = on ? 1 : 0; }, true),
      playNext: options => enqueue("playNext", () => m.playNext(options), true),
      jumpTo: ([index, id]) => enqueue("jumpTo", () => (m.queue.items[index] || {}).id === id ? m.changeToMediaAtIndex(index) : false, true),
    },
  });
})();
