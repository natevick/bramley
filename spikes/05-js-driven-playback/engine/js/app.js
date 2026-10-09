(function () {
  const C = globalThis.__spike_config;
  const S = { phase: "start", manifest: C.manifest, licenseUrl: C.licenseUrl, hlsUrl: C.hlsUrl, hlsVersion: null, hlsSupported: null, errors: [], hlsEvents: {}, keyLoaded: 0, maxCurrentTime: 0, lastCurrentTime: 0, timeupdates: 0, mediaEvents: {}, firstAudioMs: null, firstKeyLoadedMs: null };
  Object.defineProperty(globalThis, "__app", { value: { statusJson: () => JSON.stringify(S) }, enumerable: false });

  const audio = document.createElement("audio");
  document.body.appendChild(audio);
  audio.loop = !!C.loop;
  for (const t of ["loadstart", "play", "playing", "pause", "waiting", "seeking", "seeked", "ended", "error", "loadedmetadata", "loadeddata", "canplay", "canplaythrough", "durationchange", "encrypted", "waitingforkey", "emptied"]) {
    audio.addEventListener(t, () => { S.mediaEvents[t] = (S.mediaEvents[t] || 0) + 1; });
  }
  audio.addEventListener("timeupdate", () => {
    S.timeupdates++;
    const t = audio.currentTime;
    S.lastCurrentTime = t;
    if (t > S.maxCurrentTime) S.maxCurrentTime = t;
    if (t > 0 && S.firstAudioMs === null) S.firstAudioMs = performance.now();
  });

  const script = document.createElement("script");
  script.src = C.hlsUrl;
  script.onload = () => { S.phase = "hls-loaded"; try { start(); } catch (e) { S.phase = "start-threw"; S.errors.push(String(e && e.stack || e)); } };
  script.onerror = () => { S.phase = "hls-load-failed"; };
  S.phase = "loading-hls";
  document.head.appendChild(script);

  function start() {
    S.hlsVersion = Hls.version;
    S.hlsSupported = Hls.isSupported();
    const hls = new Hls({
      enableWorker: false,
      emeEnabled: true,
      drmSystems: { "com.widevine.alpha": { licenseUrl: C.licenseUrl } },
      debug: C.hlsDebug ? { log: (...a) => console.log(...a), info: (...a) => console.info(...a), warn: (...a) => console.warn(...a), error: (...a) => console.error(...a), debug: (...a) => console.debug(...a), trace: () => {} } : false,
      startPosition: 0,
      backBufferLength: Infinity
    });
    const E = Hls.Events;
    for (const k of ["MEDIA_ATTACHED", "MANIFEST_LOADED", "MANIFEST_PARSED", "LEVEL_LOADED", "KEY_LOADING", "KEY_LOADED", "FRAG_LOADED", "FRAG_DECRYPTED", "FRAG_PARSED", "FRAG_BUFFERED", "BUFFER_CREATED", "BUFFER_APPENDED", "BUFFER_EOS", "BUFFER_FLUSHED"]) {
      hls.on(E[k], () => { S.hlsEvents[k] = (S.hlsEvents[k] || 0) + 1; });
    }
    hls.on(E.KEY_LOADED, () => { S.keyLoaded++; if (S.firstKeyLoadedMs === null) S.firstKeyLoadedMs = performance.now(); });
    hls.on(E.ERROR, (ev, d) => {
      S.errors.push({ type: d.type, details: d.details, fatal: d.fatal, reason: d.reason || (d.error && String(d.error.message || d.error)) || null, code: d.response ? d.response.code : null });
      if (S.errors.length > 40) S.errors.shift();
      if (d.fatal) S.phase = "fatal:" + d.details;
    });
    hls.on(E.MANIFEST_PARSED, () => {
      S.phase = "manifest-parsed";
      const p = audio.play();
      if (p && p.then) p.then(() => { if (S.phase === "manifest-parsed") S.phase = "play-requested"; }, e => S.errors.push("play rejected: " + e));
    });
    audio.addEventListener("playing", () => { if (!S.phase.startsWith("fatal")) S.phase = "playing"; });
    hls.attachMedia(audio);
    let src = C.manifest;
    if (C.audioMasterCodecs) {
      const master = "#EXTM3U\n#EXT-X-VERSION:6\n#EXT-X-STREAM-INF:BANDWIDTH=140000,CODECS=\"" + C.audioMasterCodecs + "\"\n" + C.manifest + "\n";
      src = "data:application/vnd.apple.mpegurl;base64," + btoa(master);
      S.masterPlaylist = master;
    }
    hls.loadSource(src);
    S.phase = "loading-manifest";
  }
})();
