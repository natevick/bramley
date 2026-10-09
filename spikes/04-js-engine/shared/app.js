(function () {
  const S = globalThis.__spike;
  S.refCheck = (() => { try { __definitely_missing_global__; return "no-throw"; } catch (e) { return e.name; } })();
  const mkType = typeof MusicKit;
  S.mk = { definedAtMs: S.now(), typeofMusicKit: mkType, version: mkType === "object" && MusicKit ? String(MusicKit.version) : null, keys: mkType === "object" && MusicKit ? Object.keys(MusicKit).length : 0 };
  globalThis.__host_print("MK " + JSON.stringify(S.mk));
  if (mkType !== "object" || !MusicKit || typeof MusicKit.configure !== "function") { S.configure = "not-reached"; return; }
  const b64u = s => btoa(s).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
  const iat = Math.floor(Date.now() / 1000);
  const dummy = b64u(JSON.stringify({ alg: "ES256", kid: "SPIKEDUMMY", typ: "JWT" })) + "." + b64u(JSON.stringify({ iss: "SPIKEDUMMYTEAM", iat, exp: iat + 3600 })) + ".c3Bpa2UtZHVtbXktc2lnbmF0dXJl";
  S.configure = "called";
  S.configureCalledAtMs = S.now();
  let p;
  try { p = MusicKit.configure({ developerToken: dummy, app: { name: "spike", build: "0.0.1" } }); }
  catch (e) { S.configure = "threw: " + (e && (e.name + ": " + e.message)); S.markPoint("configure-threw"); return; }
  Promise.resolve(p).then(
    inst => {
      S.configure = "resolved";
      S.configureInstanceKeys = inst ? Object.keys(inst).slice(0, 40) : null;
      S.markPoint("configure-resolved");
      try { S.instState = { isAuthorized: inst.isAuthorized, storefrontId: inst.storefrontId, playbackState: inst.playbackState }; } catch (e) { S.instState = "threw: " + e; }
      S.apiProbe = "called";
      try {
        Promise.resolve(inst.api.music("/v1/catalog/us/songs/1440833098")).then(
          () => { S.apiProbe = "resolved"; },
          e => { S.apiProbe = "rejected: " + (e && ((e.name || "") + ": " + (e.message || e.errorCode || ""))); }
        );
      } catch (e) { S.apiProbe = "threw: " + (e && e.message); }
    },
    e => { S.configure = "rejected: " + (e && ((e.name || "") + ": " + (e.message || e.errorCode || JSON.stringify(e)))); S.markPoint("configure-rejected"); }
  );
})();
