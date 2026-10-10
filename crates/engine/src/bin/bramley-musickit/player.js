(function () {
  "use strict";
  const C = globalThis.__bramley_musickit;
  delete globalThis.__bramley_musickit;
  const S = { phase: "start", errors: [], events: [], signInNeeded: null };
  const err = (where, e) => {
    S.errors.push((where + ": " + (e && (e.name || "") + " " + (e.message || e.description || e.errorCode || String(e)) + (e && e.stack ? " @ " + e.stack : ""))));
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
    try {
      await m.setQueue({ songs: C.songs });
      S.phase = "queued";
      await m.play();
      S.phase = "play-called";
    } catch (e) { err("play", e); S.phase = "play-failed"; }
  });

  const script = document.createElement("script");
  script.src = C.musickitUrl;
  script.onerror = () => { S.phase = "musickit-load-failed"; };
  S.phase = "loading-musickit";
  document.head.appendChild(script);

  const g = f => { try { return f(); } catch (e) { return "ERR " + e.message; } };
  const act = (name, f) => { try { Promise.resolve(f()).catch(e => err(name, e)); } catch (e) { err(name, e); } };
  const state = () => {
    const r = { phase: S.phase, errors: S.errors.slice(), events: S.events.slice(-40), signInNeeded: S.signInNeeded, musickitDefined: typeof MusicKit !== "undefined" };
    if (!m) return r;
    r.isAuthorized = g(() => m.isAuthorized);
    r.storefrontId = g(() => m.storefrontId);
    r.previewOnly = g(() => m.previewOnly);
    r.playbackState = g(() => m.playbackState);
    r.currentPlaybackTime = g(() => m.currentPlaybackTime);
    r.currentPlaybackDuration = g(() => m.currentPlaybackDuration);
    r.queuePosition = g(() => m.queue.position);
    r.item = g(() => m.nowPlayingItem && { id: m.nowPlayingItem.id, durationInMillis: m.nowPlayingItem.attributes && m.nowPlayingItem.attributes.durationInMillis });
    return r;
  };
  Object.defineProperty(globalThis, "__player", {
    value: {
      state,
      pause: () => act("pause", () => m.pause()),
      play: () => act("play", () => m.play()),
      seek: t => act("seek", () => m.seekToTime(t)),
      next: () => act("next", () => m.skipToNextItem()),
    },
  });
})();
