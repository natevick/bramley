import json, statistics, sys

SONG, NEXT = "6781024437", "6781079518"
PSS_PASS, PSS_MARGINAL = 112, 125
LAND = 0.5

m = json.load(open(sys.argv[1]))
st = json.load(open(sys.argv[2]))
try:
    rt = json.load(open(sys.argv[3]))
except Exception as e:
    rt = {"error": str(e), "streams": [], "targets": []}
before, after, sink = sys.argv[4], sys.argv[5], sys.argv[6]
warmup, sample = float(sys.argv[7]), float(sys.argv[8])

player = st.get("player") or {}
controls = st.get("controls") or {}
steps = {s["step"]: s for s in controls.get("steps", [])}
trace = controls.get("trace", [])
bc = controls.get("beforeControls") or {}
endpoints = (st.get("platform") or {}).get("xhr", {}).get("endpoints", {})
scripts = [s for s in st.get("scripts", []) if s.get("phase") != "fetching"]

musickit_loaded = bool(player.get("musickitDefined")) and any(s.get("phase") == "evaluated" for s in scripts)
at_pause = (steps.get("pause") or {}).get("js") or player
samples = [s for s in st.get("samples", []) if s[3] == SONG and (not steps.get("pause") or s[0] < steps["pause"]["wallSec"])]
cpts = [(s[0], s[1]) for s in samples if isinstance(s[1], (int, float))]
max_cpt = max((c for _, c in cpts), default=0)
last = cpts[-1] if cpts else (0, 0)
ten_back = [c for t, c in cpts if t <= last[0] - 10]
advancing = bool(ten_back) and last[1] - ten_back[-1] >= 8
medias = list((bc.get("medias") or {}).values())
native = max(medias, key=lambda x: x.get("decodedDecryptedSeconds", 0)) if medias else {}
schemes = sorted({(sb.get("init") or {}).get("protected", {}).get("scheme") for sb in (bc.get("sourceBuffers") or {}).values() if (sb.get("init") or {}).get("protected")})
licenses = {k: v for k, v in endpoints.items() if "acquireWebPlaybackLicense" in k}
license_hosts = sorted({k.split("//")[1].split("/")[0] for k in licenses})
apple_rejections = {k: v["statuses"] for k, v in endpoints.items() if "play.itunes.apple.com" in k and any(str(code) == "error" or not str(code).startswith("2") for code in v.get("statuses", {}))}


def first(after_step, pred):
    t0 = (steps.get(after_step) or {}).get("wallSec")
    return next((t for t in trace if t0 is not None and t[0] > t0 and pred(t)), None)


def near(a, b):
    return isinstance(a, (int, float)) and isinstance(b, (int, float)) and abs(a - b) <= LAND


paused = first("pause", lambda t: t[2] == 3)
resumed = first("seeked-120", lambda t: t[2] == 2 and t[3] == SONG)
skipped = first("resumed-5s", lambda t: t[2] == 2 and t[3] == NEXT)
nt = lambda name: (steps.get(name) or {}).get("nativeTime")
c6 = {
    "pauseHolds": bool(paused) and near(nt("paused-4s"), paused[4]),
    "seekLandsOn120": near(nt("seeked-120"), 120),
    "resumeStartsAt120": bool(resumed) and near(resumed[4], 120),
    "resumeAdvances": isinstance(nt("resumed-5s"), (int, float)) and nt("resumed-5s") >= 124,
    "skipStartsAtZero": bool(skipped) and near(skipped[4], 0),
    "skipPlaysNext": (steps.get("next-8s") or {}).get("js", {}).get("item", {}).get("id") == NEXT and (steps["next-8s"]["js"].get("currentPlaybackTime") or 0) >= 7,
    "noCrash": bool(controls.get("done")) and m.get("exitedEarly") in (None, 0),
}
checks = {
    "c1_authorizedUs": at_pause.get("isAuthorized") is True and at_pause.get("storefrontId") == "us",
    "c2_jsPast35AndAdvancing": max_cpt > 35 and advancing,
    "c2_nativeDecryptedOver35": native.get("decodedDecryptedSeconds", 0) > 35,
    "c3_usableKey": bool((bc.get("platform") or {}).get("eme", {}).get("usableSeen")) and bool(license_hosts) and bool(schemes),
    "c4_nullSinkOnly": len(rt.get("streams", [])) > 0 and rt.get("targets") == [sink] and before == after,
    "c6_controls": all(c6.values()),
}
window = [s for s in m.get("samplesRaw", []) if warmup <= s["t"] <= warmup + sample]
pause_wall = (steps.get("pause") or {}).get("wallSec", 1e9)
pss = round(statistics.median(s["pssKiB"] for s in window) / 1024, 1) if window else None
steady = len(window) >= 25 and pause_wall > warmup + sample
checks["c5_steadyWindow"] = steady
host_calls = (st.get("platform") or {}).get("hostCalls", {})
preview_refused = any(k.startswith("HTMLMediaElement.unsupportedSrc:https://audio-ssl.itunes.apple.com") for k in host_calls)
if not musickit_loaded or not st.get("samples"):
    verdict, reason = "VOID", "musickit.js did not load"
elif not all(checks.values()):
    verdict = "FAIL"
    if player.get("signInNeeded"):
        reason = "FAIL: sign-in needed (" + player["signInNeeded"] + "); no playback, so the 30 s preview guard was not exercised"
    elif preview_refused:
        reason = "FAIL: no playback (preview path unsupported); the 30 s preview guard was not exercised"
    else:
        reason = "FAIL: " + ", ".join(k for k, v in checks.items() if not v)
else:
    verdict = "PASS" if pss <= PSS_PASS else "MARGINAL" if pss <= PSS_MARGINAL else "FAIL"
    reason = verdict + ": all checks met, PSS median " + str(pss) + " MiB"
codecs = sorted({(i.get("codec"), i.get("sampleRate")) for i in ((sb.get("init") or {}) for sb in (bc.get("sourceBuffers") or {}).values()) if i})
print(json.dumps({
    "name": m["name"], "verdict": verdict, "reason": reason, "pssMedMiB": pss, "pssMaxMiB": round(max(s["pssKiB"] for s in window) / 1024, 1) if window else None, "windowSamples": len(window),
    "checks": checks, "c6": c6, "appleRejections": apple_rejections,
    "detail": {
        "phase": player.get("phase"), "errors": player.get("errors"), "isAuthorized": at_pause.get("isAuthorized"), "storefront": at_pause.get("storefrontId"), "item": at_pause.get("item"), "codecs": codecs, "signInNeeded": player.get("signInNeeded"),
        "maxCurrentPlaybackTime": max_cpt, "advanceLast10s": round(last[1] - ten_back[-1], 2) if ten_back else None,
        "native": {k: native.get(k) for k in ("decodedDecryptedSeconds", "decodedClearSeconds", "decryptOk", "decryptFail", "decodeErr", "underruns", "rate")},
        "licenseHosts": license_hosts, "schemes": schemes, "endpoints": endpoints,
        "landings": {"pausedAt": paused and paused[4], "after4s": nt("paused-4s"), "seeked": nt("seeked-120"), "resumedAt": resumed and resumed[4], "resumed5s": nt("resumed-5s"), "nextAt": skipped and skipped[4], "next8s": nt("next-8s")},
        "scripts": [{k: s.get(k) for k in ("url", "phase", "fetchedBytes", "evaluatedBytes", "fetchedSha256", "evaluatedSha256", "error")} for s in scripts],
        "exitedEarly": m.get("exitedEarly"), "pauseWallSec": steps.get("pause", {}).get("wallSec"),
    },
    "routing": rt,
}, indent=1))
