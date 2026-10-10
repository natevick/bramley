(function () {
  "use strict";
  const C = globalThis.__bramley_player;
  delete globalThis.__bramley_player;
  const S = { phase: "start", errors: [], events: [], signInNeeded: null, playbackError: null };
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
  let queued = ready;
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
    m.addEventListener("mediaPlaybackError", e => { S.playbackError = S.playbackError || describe(e); });
    S.phase = "ready";
    signedIn();
  });

  const script = document.createElement("script");
  script.src = C.musickitUrl;
  script.onerror = () => { S.phase = "musickit-load-failed"; };
  S.phase = "loading-musickit";
  document.head.appendChild(script);

  const fail = name => e => { err(name, e); S.playbackError = S.playbackError || name + ": " + describe(e); };
  const act = (name, f) => { queued.then(f).catch(fail(name)); };
  const g = f => { try { return f(); } catch (e) { return "ERR " + e.message; } };
  const state = () => {
    const r = { phase: S.phase, errors: S.errors.slice(), events: S.events.slice(-40), signInNeeded: S.signInNeeded, playbackError: S.playbackError, musickitDefined: typeof MusicKit !== "undefined" };
    if (!m) return r;
    r.isAuthorized = g(() => m.isAuthorized);
    r.storefrontId = g(() => m.storefrontId);
    r.previewOnly = g(() => m.previewOnly);
    r.playbackState = g(() => m.playbackState);
    r.currentPlaybackTime = g(() => m.currentPlaybackTime);
    r.currentPlaybackDuration = g(() => m.currentPlaybackDuration);
    r.queuePosition = g(() => m.queue.position);
    r.item = g(() => m.nowPlayingItem && { id: m.nowPlayingItem.id, title: m.nowPlayingItem.title, durationInMillis: m.nowPlayingItem.attributes && m.nowPlayingItem.attributes.durationInMillis });
    return r;
  };
  Object.defineProperty(globalThis, "__player", {
    value: {
      state,
      setQueue: options => {
        S.playbackError = null;
        queued = ready.then(() => m.setQueue(options)).catch(fail("setQueue"));
      },
      play: () => act("play", () => m.play()),
      pause: () => act("pause", () => m.pause()),
      seek: t => act("seek", () => m.seekToTime(t)),
      next: () => act("next", () => m.skipToNextItem()),
      previous: () => act("previous", () => m.skipToPreviousItem()),
      volume: v => act("volume", () => { m.volume = v; }),
    },
  });
})();
