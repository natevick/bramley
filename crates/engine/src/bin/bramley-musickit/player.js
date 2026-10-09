(function () {
  "use strict";
  const C = globalThis.__bramley_musickit;
  delete globalThis.__bramley_musickit;
  const S = { phase: "start", errors: [], events: [], musickitVersion: null };
  const err = (where, e) => {
    S.errors.push((where + ": " + (e && (e.name || "") + " " + (e.message || e.description || e.errorCode || String(e)) + (e && e.stack ? " @ " + e.stack : ""))).slice(0, 600));
    if (S.errors.length > 30) S.errors.shift();
  };
  const claims = JSON.parse(atob(C.developerToken.split(".")[1].replace(/-/g, "+").replace(/_/g, "/")));
  const userTokenKey = ("music." + claims.iss).toLowerCase() + ".media-user-token";
  if (C.userToken) localStorage.setItem(userTokenKey, C.userToken);
  else localStorage.removeItem(userTokenKey);

  let m = null;
  document.addEventListener("musickitloaded", async () => {
    S.phase = "loaded";
    S.musickitVersion = MusicKit.version;
    try {
      await MusicKit.configure({ developerToken: C.developerToken, app: { name: "Bramley", build: C.build } });
      S.phase = "configured";
    } catch (e) { err("configure", e); S.phase = "configure-failed"; return; }
    m = MusicKit.getInstance();
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
    const r = { phase: S.phase, errors: S.errors.slice(), events: S.events.slice(-40), musickitVersion: S.musickitVersion, musickitDefined: typeof MusicKit !== "undefined" };
    if (!m) return r;
    r.isAuthorized = g(() => m.isAuthorized);
    r.storefrontId = g(() => m.storefrontId);
    r.previewOnly = g(() => m.previewOnly);
    r.playbackState = g(() => m.playbackState);
    r.currentPlaybackTime = g(() => m.currentPlaybackTime);
    r.currentPlaybackDuration = g(() => m.currentPlaybackDuration);
    r.queuePosition = g(() => m.queue.position);
    r.item = g(() => m.nowPlayingItem && { id: m.nowPlayingItem.id, durationInMillis: m.nowPlayingItem.attributes && m.nowPlayingItem.attributes.durationInMillis, isPreview: m.nowPlayingItem.isPreview, flavor: m.nowPlayingItem.flavor });
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
