import json, os, statistics, subprocess, sys, threading, time

HERE = os.path.dirname(os.path.abspath(__file__))
MK = os.path.join(HERE, "..", "02-musickit-surface", "vendor-cache", "musickit.js")
BAR_PSS_MIB = 40.0


def kids_map():
    m = {}
    for d in os.listdir("/proc"):
        if not d.isdigit():
            continue
        try:
            with open(f"/proc/{d}/stat") as f:
                s = f.read()
            ppid = int(s[s.rindex(")") + 2:].split()[1])
            m.setdefault(ppid, []).append(int(d))
        except Exception:
            pass
    return m


def tree(root):
    m = kids_map()
    out, stack = [], [root]
    while stack:
        p = stack.pop()
        if p in out:
            continue
        out.append(p)
        stack.extend(m.get(p, []))
    return out


def rollup(pid):
    r = {"Rss": 0, "Pss": 0, "Private_Clean": 0, "Private_Dirty": 0}
    with open(f"/proc/{pid}/smaps_rollup") as f:
        for line in f:
            k = line.split(":")[0]
            if k in r:
                r[k] = int(line.split()[1])
    with open(f"/proc/{pid}/status") as f:
        th = next((int(l.split()[1]) for l in f if l.startswith("Threads:")), 0)
    return r, th


def sample(root):
    pss = rss = uss = 0
    procs = 0
    threads = 0
    for p in tree(root):
        try:
            r, th = rollup(p)
        except Exception:
            continue
        procs += 1
        threads += th
        pss += r["Pss"]; rss += r["Rss"]; uss += r["Private_Clean"] + r["Private_Dirty"]
    return {"pssKb": pss, "rssKb": rss, "ussKb": uss, "procs": procs, "threads": threads}


def main():
    eng, mode, name = sys.argv[1], sys.argv[2], sys.argv[3]
    extra = sys.argv[4:]
    binary = os.path.join(HERE, eng, "target", "release", f"spike-{eng}")
    with open(MK, "rb") as f:
        import hashlib
        sha = hashlib.sha256(f.read()).hexdigest()
    cmd = [binary, "--musickit", MK, "--mode", mode, "--hold", "34", "--max-wait", "20"] + extra
    t_launch = time.time()
    proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, bufsize=1)
    lines = []
    point = threading.Event()

    def reader():
        for line in proc.stdout:
            lines.append(line.rstrip("\n"))
            if line.startswith("POINT "):
                point.set()
    th = threading.Thread(target=reader, daemon=True)
    th.start()
    samples = []
    got_point = point.wait(25)
    if got_point:
        t0 = time.time()
        while time.time() - t0 < 30.0 and proc.poll() is None:
            samples.append(sample(proc.pid))
            time.sleep(2.0)
    proc.wait(timeout=90)
    th.join(timeout=5)

    def grab(prefix):
        for l in lines:
            if l.startswith(prefix + " "):
                return json.loads(l[len(prefix) + 1:])
        return None
    report = grab("REPORT") or {}
    mk = grab("MK") or report.get("mk")
    timing = grab("TIMING")
    pointrec = grab("POINT")
    fetches = [json.loads(l[6:]) for l in lines if l.startswith("FETCH ")]
    med = lambda k: round(statistics.median(s[k] for s in samples) / 1024, 1) if samples else None
    mx = lambda k: round(max(s[k] for s in samples) / 1024, 1) if samples else None
    mk_defined = bool(mk and mk.get("typeofMusicKit") == "object")
    configure_reached = report.get("configure") not in (None, "not-reached")
    did_run = mk_defined and configure_reached and got_point and len(samples) >= 10
    summary = {
        "pssMedMiB": med("pssKb"), "pssMaxMiB": mx("pssKb"),
        "rssMedMiB": med("rssKb"), "rssMaxMiB": mx("rssKb"),
        "ussMedMiB": med("ussKb"), "ussMaxMiB": mx("ussKb"),
        "procs": samples[-1]["procs"] if samples else None,
        "threads": samples[-1]["threads"] if samples else None,
        "samples": len(samples),
    }
    if not did_run:
        verdict = "VOID"
    else:
        verdict = "PASS" if summary["pssMedMiB"] <= BAR_PSS_MIB else "FAIL"
    res = {
        "name": name, "engine": eng, "mode": mode, "cmd": cmd, "musickitSha256": sha,
        "binaryBytes": os.path.getsize(binary),
        "verdict": verdict,
        "checks": {"mkDefined": mk_defined, "configureReached": configure_reached, "pointSeen": got_point, "samplesOk": len(samples) >= 10},
        "mk": mk, "timing": timing, "point": pointrec, "configure": report.get("configure"),
        "apiProbe": report.get("apiProbe"), "refCheck": report.get("refCheck"),
        "fetches": fetches, "scriptLoads": report.get("scriptLoads"),
        "errors": report.get("errors"), "consoleLines": report.get("consoleLines"),
        "engineOwnHostGlobals": report.get("engineOwnHostGlobals"),
        "summary": summary, "samplesRaw": samples,
        "touched": report.get("touched"), "storageKeys": report.get("storageKeys"), "createdTags": report.get("createdTags"),
        "unparsedLines": [l for l in lines if not l.split(" ")[0] in ("PID", "SRC", "MK", "TIMING", "POINT", "FETCH", "REPORT")][:40],
    }
    os.makedirs(os.path.join(HERE, "results"), exist_ok=True)
    with open(os.path.join(HERE, "results", f"{name}.json"), "w") as f:
        json.dump(res, f, indent=1)
    short = {k: res[k] for k in ("name", "verdict", "checks", "configure", "apiProbe", "refCheck", "summary", "binaryBytes")}
    short["mkDefinedAtMs"] = mk.get("definedAtMs") if mk else None
    short["timing"] = timing
    short["firstFetch"] = fetches[0]["url"] if fetches else None
    print(json.dumps(short))


main()
