import glob, json, math, os, statistics

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))
REFERENCE = {'binary': os.path.join(ROOT, 'target', 'reference', 'bramley-main-4961e27'), 'sha256': '1a72eb4b2880ac5a', 'pss': 88.6}
R1_BAR, R2_BAR, R3_BAR, R4_BAR, R5_BAR, DRIFT_BAR, BALLAST_OVER, LANDING_BAND = 200.0, 100.0, 10.0, 5.0, 1.0, 5.0, 15.0, 5.0
COVERS, ADVANCE_S, MIN_WINDOW_POLLS, RISE_TOLERANCE, REPS = 50, 50.0, 30, 3.0, 5
F1_MS, F3_MS, F4_MS, F5_MS, F4_PLAYED_S, FULL_SONG_S, R3_TRACKS = 500, 2000, 3000, 500, 35.0, 300.0, 8
REFERENCE_RUNS = ('reference', 'reference-after')
R1_SAMPLES, R2_SAMPLES, REF_SAMPLES, CPU_SAMPLES, CPU_BIN_S = 60, 20, 20, 30, 10
F5_COMMANDS, EVERY_REP = ('pause', 'seek', 'resume', 'next', 'prev'), ('pause', 'resume')
ADVANCE_EPS_S, F5_SEEK_S, F5_POSITION_S, F5_ADVANCING_MS = 0.05, 120.0, 0.1, 3000
DECRYPTED_MS, SKIPPED_MS, PACE_S = 35000, 100, 1.0
F6_SENT_SLACK_S, F6_SKIPPED_S, F6_STALL_S, F6_LAND_S, F6_WINDOW_S = 3.0, 0.1, 1.0, 1.0, 5.0
BAD_TOKEN_SEARCH, BAD_TOKEN_PLAY, NO_OUTPUT = 'developer token was rejected', 'SignInNeeded', 'the audio output could not be opened'
TOOLS = sorted('search play pause resume next previous seek queue_add now_playing library_playlists'.split())
CHECKS = ('F1', 'F2', 'F3', 'F4', 'F5', 'F6', 'F7', 'F8', 'F9', 'F10', 'R1', 'R2', 'R3', 'R4', 'R5', 'R6')
EVIDENCE = {'F1': ('f1',), 'F2': ('f2',), 'F3': ('f3',), 'F4': ('f4',), 'F5': ('f5',), 'F6': ('f6',), 'F7': ('f7',), 'F8': ('f8',), 'F9': ('f9',),
            'F10': ('f10-badid', 'f10-net'), 'R3': ('r3',), 'R4': ('r4',), 'R5': ('r5',)}


STANDING_GPU_CLIENTS, GPU_LIBS = ('gnome-shell', 'Xvfb', 'grok-bot'), ('libgallium', 'radeonsi', 'libLLVM')


def gpu_processes():
    found = {}
    for d in glob.glob('/proc/[0-9]*'):
        try:
            comm = open(f'{d}/comm').read().strip()
        except OSError:
            continue
        try:
            libs = sorted({line.split()[-1].rsplit('/', 1)[-1] for line in open(f'{d}/maps') if any(lib in line for lib in GPU_LIBS)})
        except OSError:
            libs = []
        found[int(d[6:])] = (comm, libs)
    return found


def preflight(exclude=lambda pid: False, standing=None):
    found = []
    for pid, (comm, libs) in gpu_processes().items():
        allowed = pid in standing if standing is not None else comm in STANDING_GPU_CLIENTS
        if pid != os.getpid() and not exclude(pid) and (comm in ('weston', 'bramley') or (libs and not allowed)):
            found.append(f'{pid} {comm} {libs}')
    return found


def load(out, name):
    try:
        with open(os.path.join(out, name)) as f:
            return json.load(f)
    except (OSError, ValueError):
        return None


def save(out, name, data):
    with open(os.path.join(out, name), 'w') as f:
        json.dump(data, f, indent=1)


def median(values):
    values = [v for v in values if v is not None]
    return round(statistics.median(values), 1) if values else None


def medians(rows):
    return {k: median(r[k] for r in rows) for k in ('pss', 'uss', 'rss')} if rows else {}


def renderer_voids(renderers, what):
    bad = sorted({str(g) for g in renderers if 'radeonsi' not in str(g) or 'llvmpipe' in str(g).lower()})
    return [f'{what} renderer is not radeonsi: {bad}'] if bad or not renderers else []


def weston_renderer(out):
    try:
        with open(os.path.join(out, 'weston.log')) as f:
            return [line.split('GL renderer:', 1)[1].strip() for line in f if 'GL renderer:' in line]
    except OSError:
        return None


def session_problems(out):
    missing, fails, voids = [], [], []
    build, sess, weston = load(out, 'build.json'), load(out, 'session.verdict.json'), weston_renderer(out)
    if not build:
        missing.append('missing evidence: build.json')
    else:
        if build.get('dirty') is not False:
            fails.append('built from a dirty tree')
        if build.get('shippedFeaturesExit') != 0:
            fails.append(f'shipped binary failed check-shipped-features (exit {build.get("shippedFeaturesExit")})')
    if not sess:
        missing.append('missing evidence: session.verdict.json')
    else:
        if None in (sess.get('defaultAtStart'), sess.get('defaultAtEnd')) or sess['defaultAtStart'] != sess['defaultAtEnd'] or 'defaultRestored' in sess:
            voids.append(f'default sink unreadable or changed: {sess.get("defaultAtStart")} -> {sess.get("defaultAtEnd")}, restored {sess.get("defaultRestored")}')
        if not sess.get('sinkGone'):
            voids.append('null sink left behind')
    if weston is None:
        missing.append('missing evidence: weston.log')
    else:
        voids += renderer_voids(weston, 'weston')
    cotenants = load(out, 'cotenants.json')
    if not cotenants or not cotenants.get('polls'):
        missing.append('missing evidence: cotenants.json')
    elif cotenants.get('seen'):
        voids.append(f'another weston, bramley or GPU client ran during the run: {len(cotenants["seen"])} polls, first {cotenants["seen"][0]}')
    return missing, fails, voids


def reference_problems(out):
    missing, voids, drifts = [], [], []
    for name in REFERENCE_RUNS:
        ref = load(out, f'{name}.json')
        if not ref or not ref.get('samples'):
            missing.append(f'missing evidence: {name}.json')
            continue
        if ref.get('sha256') != REFERENCE['sha256']:
            voids.append(f'{name} sha {ref.get("sha256")} is not the recorded {REFERENCE["sha256"]}')
        if any(r.get('pid') != ref.get('pid') for r in ref['samples']):
            voids.append(f'{name} snapshot from another process')
        voids += renderer_voids([r.get('glRenderer') for r in ref['samples']], name)
        drifts.append(round(medians(ref['samples'])['pss'] - REFERENCE['pss'], 1))
    return missing, voids, drifts


def guard_voids(out, ev, sink, started):
    voids, missing, window = [], [], []
    for session in ev.get('sessions') or [ev.get('session')]:
        guard = load(out, f'{session}.guard.json')
        if guard is None:
            missing.append(f'missing evidence: {session}.guard.json')
            continue
        voids += [f'guard killed the app: {guard["killed"]}'] if guard.get('killed') else []
        window += [targets for t, targets in guard.get('log', []) if ev.get('t0', 0) <= t <= ev.get('t1', 0)]
    if started and not missing and (any(targets not in ([], [sink]) for targets in window) or [sink] not in window):
        voids.append('playback started but no stream was seen linked to the null sink only')
    return voids, missing


def drift_voids(zone, outcome, drifts):
    if zone != 3:
        return []
    suspect = {'FAIL': [d for d in drifts if d > DRIFT_BAR], 'PASS': [d for d in drifts if d < -DRIFT_BAR]}.get(outcome, [])
    return [f'reference drifted {d:+.1f} MiB from {REFERENCE["pss"]} (bar {DRIFT_BAR}), which voids a {outcome}' for d in suspect]


def zone_of(m, bar, fails):
    if not m or m.get('uss') is None:
        return None
    zone = 1 if m['uss'] > bar else 2 if m['rss'] <= bar else 3
    if zone == 1:
        fails.append(f'USS median {m["uss"]} > {bar}')
    if zone == 3 and m['pss'] > bar:
        fails.append(f'PSS median {m["pss"]} > {bar}')
    return zone


def memory_check(out, name, kind, sink):
    missing, fails, voids = session_problems(out)
    ref_missing, ref_voids, drifts = reference_problems(out)
    ev, guard = load(out, f'{name}.{kind}.json'), load(out, f'{name}.guard.json')
    missing += ref_missing + [f'missing evidence: {f}' for f, d in ((f'{name}.{kind}.json', ev), (f'{name}.guard.json', guard)) if d is None]
    if missing:
        return {'verdict': 'FAIL', 'reasons': missing + fails + voids + ref_voids}
    voids += ref_voids
    rows = ev.get('samples') or []
    m = medians(rows)
    voids += renderer_voids([r.get('glRenderer') for r in rows], name) if rows else []
    if guard.get('killed'):
        voids.append(f'guard killed the app: {guard["killed"]}')
    if any(r.get('pid') != ev.get('pid') for r in rows):
        fails.append('a snapshot came from another process')
    if kind == 'r2':
        bar, want = R2_BAR, R2_SAMPLES
        if any(r.get('engineStarted') for r in rows):
            fails.append('engine started before the first play')
    else:
        bar, want = R1_BAR, R1_SAMPLES
        if (ev.get('prePlay') or {}).get('default') is None:
            voids.append('default sink unreadable before play')
        positions = [r.get('position') or 0 for r in rows]
        advanced = positions[-1] - positions[0] if rows else 0
        if advanced >= ADVANCE_S:
            window = [targets for t, targets in guard.get('log', []) if rows[0]['t'] <= t <= rows[-1]['t']]
            off = sum(1 for targets in window if targets != [sink])
            if len(window) < MIN_WINDOW_POLLS:
                voids.append(f'guard observed {len(window)} polls in the R1 window < {MIN_WINDOW_POLLS}')
            elif off:
                voids.append(f'{off} of {len(window)} polls in the R1 window not linked to the null sink only')
        if not ev.get('started'):
            fails.append('playback never started')
        if advanced < ADVANCE_S:
            fails.append(f'position advanced {advanced:.1f} s < {ADVANCE_S}')
        if any(r.get('state') != 'Playing' or r.get('banner') is not None for r in rows):
            fails.append('not Playing with no banner in every sample')
    if len(rows) < want:
        fails.append(f'{len(rows)} samples < {want}')
    if any((r.get('texturesUploaded') or 0) < COVERS for r in rows):
        fails.append(f'fewer than {COVERS} covers in some sample')
    if any(r.get('procs') != 1 for r in rows):
        fails.append('not exactly 1 process in every sample')
    zone = zone_of(m, bar, fails)
    outcome = 'VOID' if voids else 'FAIL' if fails else 'PASS'
    voids += drift_voids(zone, outcome, drifts)
    return dict(m, verdict='VOID' if voids else outcome, reasons=voids + fails, bar=bar, zone=zone, referenceDrift=max(drifts, key=abs))


def growth(rows, lo, hi):
    pick = lambda a, b: [r for r in rows if a <= r['m'] < b]
    start, end = medians(pick(lo, lo + 1)), medians(pick(hi - 1, hi + 0.01))
    return {k: round(end[k] - start[k], 1) for k in start} if start and end else {}


def lags(rows, seeks):
    lag = lambda a, b: round((b['t'] - a['t']) - (b['position'] - a['position']), 2)
    judged = [(a, b) for a, b in zip(rows, rows[1:]) if a.get('itemId') == b.get('itemId') and a.get('state') == b.get('state') == 'Playing'
              and not any(a['t'] - 1 <= t <= b['t'] + 1 for t in seeks)]
    return [lag(a, b) for a, b in judged if lag(a, b) > PACE_S]


def r3_rules(ev, fails, sink):
    rows = ev.get('samples') or []
    g = growth(rows, 5, 30)
    played = len({r.get('itemId') for r in rows if r.get('itemId')})
    if not rows or rows[-1]['m'] < 30:
        fails.append(f'soak ended at minute {rows[-1]["m"] if rows else 0} < 30')
    if any(r.get('pid') != ev.get('pid') or r.get('procs') != 1 for r in rows):
        fails.append('the app crashed, restarted or forked during the soak')
    if played < R3_TRACKS:
        fails.append(f'{played} tracks played < {R3_TRACKS}')
    if any((r.get('texturesUploaded') or 0) < COVERS for r in rows):
        fails.append(f'fewer than {COVERS} covers in some sample')
    if any(r.get('state') != 'Playing' or r.get('banner') is not None for r in rows):
        fails.append('not Playing with no banner in every sample')
    underruns = (rows[-1].get('underruns') or 0) - (rows[0].get('underruns') or 0) if rows and rows[-1].get('underruns') is not None else None
    if underruns != 0:
        fails.append(f'underruns during the soak: {underruns}')
    stalls = lags(rows, ev.get('seekTimes') or [])
    if stalls:
        fails.append(f'position fell behind the wall clock by more than {PACE_S} s: {stalls}')
    if g.get('uss') is not None:
        if g['uss'] > R3_BAR:
            fails.append(f'USS growth {g["uss"]} > {R3_BAR}')
        elif g['pss'] > R3_BAR:
            fails.append(f'PSS growth {g["pss"]} > {R3_BAR}')
    elif rows:
        fails.append('no samples at minute 5 or minute 30')
    return dict(g, zone=None if g.get('uss') is None else 1 if g['uss'] > R3_BAR else 3, tracks=played,
                changesAfter5=sum(1 for a, b in zip(rows, rows[1:]) if b['m'] >= 5 and a.get('itemId') != b.get('itemId')))


def delta(a, b, key):
    return (b.get(key) or 0) - (a.get(key) or 0)


def heard_play(j):
    base, heard, end = j.get('base') or {}, j.get('heard') or {}, j.get('end') or {}
    why = []
    if delta(base, end, 'decryptedMs') < DECRYPTED_MS:
        why.append(f'decrypted {delta(base, end, "decryptedMs")} ms < {DECRYPTED_MS}')
    if delta(base, end, 'skippedMs') > SKIPPED_MS:
        why.append(f'skipped {delta(base, end, "skippedMs")} ms > {SKIPPED_MS}')
    if delta(base, end, 'decryptFails') != 0:
        why.append(f'decrypt failures {delta(base, end, "decryptFails")}')
    if abs(delta(heard, end, 'position') - delta(heard, end, 't')) > PACE_S:
        why.append(f'position moved {delta(heard, end, "position"):.2f} s in {delta(heard, end, "t"):.2f} s of wall clock (tolerance {PACE_S})')
    if (end.get('position') or 0) <= F4_PLAYED_S or j.get('itemId') != j.get('song') or j.get('state') != 'Playing' or j.get('banner') is not None:
        why.append(f'not Playing past {F4_PLAYED_S} s of the asked song with no banner: {j.get("state")}, {end.get("position")}, {j.get("banner")}')
    if (j.get('duration') or 0) < FULL_SONG_S:
        why.append(f'duration {j.get("duration")} < {FULL_SONG_S}')
    return why


def audible(ev, fails, plays):
    reps = [r.get('audioMs') for r in ev.get('reps') or []]
    if not ev.get('started'):
        fails.append('playback never started')
    if len(reps) < REPS or None in reps:
        fails.append(f'audio started in {sum(r is not None for r in reps)} of {REPS} plays')
    elif median(reps) > F4_MS:
        fails.append(f'audio median {median(reps)} ms > {F4_MS}')
    judged = ev.get('judged') or []
    if len(judged) < plays:
        fails.append(f'{len(judged)} judged plays < {plays}')
    for i, j in enumerate(judged):
        fails += [f'judged play {i}: {w}' for w in heard_play(dict(j, song=ev.get('song')))]
    return {'audioMedianMs': median(reps), 'decryptedMs': [delta(j.get('base') or {}, j.get('end') or {}, 'decryptedMs') for j in judged]}


def f6_move(t):
    base, flip, end = t.get('base'), t.get('flip'), t.get('end')
    if not (base and flip and end):
        return {'pass': False, 'why': 'no applied seek, flip or window end'}
    if end['t'] < flip['t'] + F6_WINDOW_S:
        return {'pass': False, 'why': f'window ended {end["t"] - flip["t"]:.2f} s after the flip < {F6_WINDOW_S}'}
    window = [(ts, p) for ts, p in t.get('samples') or [] if abs(ts - flip['t']) <= F6_WINDOW_S]
    stall, moved = 0.0, window[0][0] if window else 0.0
    for (t0, p0), (t1, p1) in zip(window, window[1:]):
        moved = t1 if p1 > p0 else moved
        stall = max(stall, t1 - moved)
    m = {'drained': delta(base, end, 'drainsAcked') >= 1 and delta(base, end, 'drainTimeouts') == 0,
         'sentS': delta(base, flip, 'sentMs') / 1000, 'skippedS': delta(base, end, 'skippedMs') / 1000, 'stallS': round(stall, 2),
         'landedOff': round((base.get('position') or 0) - t.get('start', 0) - t.get('target', 0), 2), 'cut': t.get('cut')}
    m['gapless'] = m['sentS'] >= t['duration'] - t['target'] - F6_SENT_SLACK_S and m['skippedS'] <= F6_SKIPPED_S and m['stallS'] <= F6_STALL_S
    m['pass'] = abs(m['landedOff']) <= F6_LAND_S and (m['drained'] or m['gapless'])
    return m


def f_rules(check, ev, fails):
    if check == 'F1':
        starts = ev.get('starts') or []
        if len(starts) < REPS or any(s.get('ms') is None for s in starts):
            fails.append(f'{sum(s.get("ms") is not None for s in starts)} of {REPS} starts reached a first frame')
        elif max(s['ms'] for s in starts) > F1_MS:
            fails.append(f'slowest first frame {max(s["ms"] for s in starts)} ms > {F1_MS}')
        if any(not s.get('playlists') or s.get('route') != 'Home' for s in starts):
            fails.append('a start did not show the library sidebar on Home')
        return {'medianMs': median(s.get('ms') for s in starts)}
    if check == 'F2':
        if ev.get('banner') != 'SignInNeeded':
            fails.append(f'banner {ev.get("banner")} is not SignInNeeded')
        if ev.get('playExit') != 5 or ev.get('played') or ev.get('streams'):
            fails.append(f'a signed-out play was not refused cleanly: exit {ev.get("playExit")}, played {ev.get("played")}, streams {ev.get("streams")}')
        return {}
    if check == 'F3':
        reps = ev.get('reps') or []
        if len(reps) < REPS or any(r.get('exit') != 0 or not r.get('songs') for r in reps):
            fails.append('a search did not return songs')
        if len(reps) < REPS or any(r.get('uiMs') is None for r in reps):
            fails.append('the Search view did not show the results every time')
        for key in ('wallMs', 'uiMs'):
            if reps and (median(r.get(key) for r in reps) or F3_MS + 1) > F3_MS:
                fails.append(f'{key} median {median(r.get(key) for r in reps)} > {F3_MS}')
        return {'wallMedianMs': median(r.get('wallMs') for r in reps), 'uiMedianMs': median(r.get('uiMs') for r in reps)}
    if check in ('F4', 'F9'):
        extra = audible(ev, fails, REPS if check == 'F4' else 1)
        if check == 'F4' and any(r.get('exit') != 0 for r in ev.get('reps') or []):
            fails.append('a play command did not exit 0')
        if check == 'F9' and any(r.get('error') is not False for r in ev.get('reps') or []):
            fails.append('an MCP play returned an error')
        if check == 'F9':
            if ev.get('tools') != TOOLS:
                fails.append(f'tools/list gave {ev.get("tools")}')
            if not ev.get('searchHasSong'):
                fails.append('MCP search did not return the song')
        return extra
    if check == 'F5':
        rounds, out = ev.get('rounds') or [], {}
        if len(rounds) < REPS:
            fails.append(f'{len(rounds)} rounds < {REPS}')
        for cmd in F5_COMMANDS:
            seen = [r.get(cmd) or {} for r in rounds]
            ms = [s.get('effectMs') for s in seen]
            if any(s.get('exit') != 0 or s.get('appliedMs') is None or s.get('effectMs') is None for s in seen):
                fails.append(f'{cmd}: a command failed, or its snapshot never showed applied >= seq with the effect')
            elif cmd in EVERY_REP and rounds and max(ms) > F5_MS:
                fails.append(f'{cmd}: slowest {max(ms)} ms > {F5_MS}')
            elif rounds and median(ms) > F5_MS:
                fails.append(f'{cmd}: median {median(ms)} ms > {F5_MS}')
            if cmd in ('next', 'prev'):
                heard = [s.get('advancingMs') for s in seen]
                if None in heard:
                    fails.append(f'{cmd}: the new item never advanced')
                elif rounds and median(heard) > F5_ADVANCING_MS:
                    fails.append(f'{cmd}: new item advancing median {median(heard)} ms > {F5_ADVANCING_MS}')
            out[cmd] = median(ms)
        return out
    if check == 'F6':
        moves = [f6_move(t) for t in ev.get('transitions') or []]
        if len(moves) < 3 or not all(m['pass'] for m in moves):
            fails.append(f'{sum(m["pass"] for m in moves)} of 3 transitions passed')
        return {'transitions': moves}
    if check == 'F7':
        if not (ev.get('mcpTotal') or 0) >= 1:
            fails.append('MCP library_playlists listed no playlists')
        if (ev.get('largestTracks') or 0) < 100:
            fails.append(f'largest playlist has {ev.get("largestTracks")} tracks < 100')
        if not ev.get('endReached') or not ev.get('artTextures'):
            fails.append(f'the playlist view did not reach its end with covers: end {ev.get("endReached")}, art {ev.get("artTextures")}')
        return {'driver': ev.get('driver')}
    if check == 'F8':
        if not ev.get('expected') or ev.get('seen') != ev.get('expected') or any(ev.get('exits') or [1]):
            fails.append(f'played {ev.get("seen")}, expected {ev.get("expected")}, exits {ev.get("exits")}')
        return {}
    if check == 'F10':
        bad, net = ev['f10-badid'], ev['f10-net']
        if not (bad.get('alive') and bad.get('error')):
            fails.append(f'a bad id did not show an error with the app alive: {bad}')
        if not (net.get('downError') and net.get('downUi') == 'error' and net.get('alive')):
            fails.append(f'network down did not give an error with the app alive: {net}')
        if net.get('coversRecoveredS') is None or not net.get('searchAfter') or net.get('uiAfter') != 'results':
            fails.append(f'no automatic recovery after the network returned: {net}')
        return {}
    if check in ('R4', 'R5'):
        bar, state = (R4_BAR, 'Playing') if check == 'R4' else (R5_BAR, 'Paused')
        cpu = ev.get('cpu') or []
        if len(cpu) < CPU_SAMPLES:
            fails.append(f'{len(cpu)} CPU samples < {CPU_SAMPLES}')
        if ev.get('binS') != CPU_BIN_S or any(p != 1 for p in ev.get('procs') or [None]):
            fails.append(f'bins of {ev.get("binS")} s (want {CPU_BIN_S}) or not exactly 1 process: {sorted(set(ev.get("procs") or []))}')
        if ev.get('before') != state or ev.get('after') != state:
            fails.append(f'not {state} across the window: {ev.get("before")} -> {ev.get("after")}')
        if cpu and median(cpu) > bar:
            fails.append(f'CPU median {median(cpu)} % > {bar}')
        return {'cpuMedian': median(cpu), 'cpuMean': ev.get('mean'), 'bar': bar}
    return {}


PLAYS = ('F4', 'F5', 'F6', 'F8', 'F9', 'R3', 'R4')


def check(out, name, pre=''):
    missing, fails, voids = session_problems(out)
    files = {f: load(out, f'{pre}{f}.json') for f in EVIDENCE[name]}
    missing += [f'missing evidence: {pre}{f}.json' for f, d in files.items() if d is None]
    sink = (load(out, 'session.verdict.json') or {}).get('sink')
    drifts, zone = [], None
    if name == 'R3':
        ref_missing, ref_voids, drifts = reference_problems(out)
        missing += ref_missing
        voids += ref_voids
    for ev in files.values():
        if ev is None:
            continue
        if 'session' in ev or 'sessions' in ev:
            more_voids, more_missing = guard_voids(out, ev, sink, name in PLAYS and ev.get('started'))
            voids, missing = voids + more_voids, missing + more_missing
        voids += renderer_voids(ev.get('renderers') or [], name)
    if missing:
        return {'verdict': 'FAIL', 'reasons': missing + fails + voids}
    ev = files[EVIDENCE[name][0]] if len(files) == 1 else files
    extra = r3_rules(ev, fails, sink) if name == 'R3' else f_rules(name, ev, fails)
    outcome = 'VOID' if voids else 'FAIL' if fails else 'PASS'
    if name == 'R3':
        voids += drift_voids(extra.get('zone'), outcome, drifts)
    return dict(extra, verdict='VOID' if voids else outcome, reasons=voids + fails)


def checks(out, pre=''):
    v = {c: check(out, c, pre) for c in CHECKS if c in EVIDENCE}
    v['R1'], v['R2'] = (memory_check(out, f'{pre}shipped', k, (load(out, 'session.verdict.json') or {}).get('sink')) for k in ('r1', 'r2'))
    size = (load(out, 'build.json') or {}).get('shippedBytes')
    v['R6'] = {'verdict': 'PASS' if size else 'FAIL', 'bytes': size, 'reasons': [] if size else ['missing evidence: shippedBytes in build.json']}
    return {c: v[c] for c in CHECKS}


def ballast_size(out):
    rows = (load(out, 'shipped.r1.json') or {}).get('samples')
    return max(0, math.ceil(R1_BAR + BALLAST_OVER - medians(rows)['uss'])) if rows else None


def ballast(out, build):
    sink = (load(out, 'session.verdict.json') or {}).get('sink')
    b = memory_check(out, 'ballast', 'r1', sink)
    shipped, ev = load(out, 'shipped.r1.json') or {}, load(out, 'ballast.r1.json') or {}
    shipped_uss = medians(shipped.get('samples')).get('uss')
    rise = round(b['uss'] - shipped_uss, 1) if b.get('uss') is not None and shipped_uss is not None else None
    kept = ((ev.get('samples') or [{}])[-1].get('decodedBytesKept') or 0) / 2**20
    expected = ev['ballastMiB'] + kept if ev.get('ballastMiB') is not None else None
    threshold_only = b['verdict'] == 'FAIL' and [r[:10] for r in b['reasons']] == ['USS median']
    landed = b.get('uss') is not None and abs(b['uss'] - (R1_BAR + BALLAST_OVER)) <= LANDING_BAND
    flagged = build.get('ballastFeaturesExit') == 1 and 'app/gate-sabotage' in (build.get('ballastFeaturesOutput') or '')
    rose = rise is not None and expected is not None and abs(rise - expected) <= RISE_TOLERANCE
    return dict(b, ballastMiB=ev.get('ballastMiB'), keptMiB=round(kept, 2), ussRise=rise, holds=threshold_only and landed and flagged and rose,
                thresholdOnly=threshold_only, landedWithinBand=landed, failsShippedCheck=flagged, ussRoseByBallast=rose)


def real_fail(c, *reasons):
    return c['verdict'] == 'FAIL' and bool(reasons) and all(any(want in r for r in c['reasons']) for want in reasons)


def banners(out, name):
    return [str(r.get('banner')) for r in (load(out, name) or {}).get('reps') or []]


def controls(out, build):
    sink, c, empty = (load(out, 'session.verdict.json') or {}).get('sink'), {}, os.path.join(out, 'empty')
    os.makedirs(empty, exist_ok=True)
    seen = lambda pre, names: {n: memory_check(out, f'{pre}shipped', 'r1', sink) if n == 'R1' else check(out, n, pre) for n in names}
    flagged = build.get('ballastFeaturesExit') == 1 and 'app/gate-sabotage' in (build.get('ballastFeaturesOutput') or '')
    nouser, badtoken, nostream = seen('nouser.', ('F2', 'F4', 'F5', 'F6', 'F9', 'R1')), seen('badtoken.', ('F3', 'F4')), seen('nostream.', ('F4',))
    leak = check(out, 'R3', 'leak.')
    want = {'F4': ('playback never started',), 'F5': ('0 rounds',), 'F6': ('0 of 3 transitions',), 'F9': ('playback never started',), 'R1': ('playback never started',)}
    c['noUserToken'] = dict({n: v['verdict'] for n, v in nouser.items()}, holds=nouser['F2']['verdict'] == 'PASS' and all(real_fail(v, *want[n]) for n, v in nouser.items() if n != 'F2')
                            and all('SignInNeeded' in b for b in banners(out, 'nouser.f4.json')))
    f3_errors = [str(r.get('error')) for r in (load(out, 'badtoken.f3.json') or {}).get('reps') or []]
    c['badDeveloperToken'] = dict({n: v['verdict'] for n, v in badtoken.items()}, holds=real_fail(badtoken['F3'], 'did not return songs') and real_fail(badtoken['F4'], 'playback never started')
                                  and bool(f3_errors) and all(BAD_TOKEN_SEARCH in e for e in f3_errors) and all(BAD_TOKEN_PLAY in b for b in banners(out, 'badtoken.f4.json')))
    c['noStream'] = {'F4': nostream['F4']['verdict'], 'holds': real_fail(nostream['F4'], 'playback never started') and all(NO_OUTPUT in b for b in banners(out, 'nostream.f4.json'))}
    cut, moves = check(out, 'F6', 'cut.'), [f6_move(t) for t in (load(out, 'cut.f6.json') or {}).get('transitions') or []]
    caught = [m for m in moves if m.get('cut') and not m['pass'] and m['skippedS'] > F6_SKIPPED_S and abs(m['landedOff']) <= F6_LAND_S]
    c['cutTail'] = {'F6': cut['verdict'], 'holds': real_fail(cut, '2 of 3 transitions') and len(caught) == 1}
    c['leak'] = {'R3': leak['verdict'], 'zone': leak.get('zone'), 'uss': leak.get('uss'), 'changesAfter5': leak.get('changesAfter5'),
                 'holds': real_fail(leak, 'USS growth') and (leak.get('changesAfter5') or 0) >= 4 and flagged}
    stall = load(out, 'stall.stall.json') or {}
    pair = [dict(stall[k], state='Playing') for k in ('before', 'after') if stall.get(k)]
    caught = lags(pair, []) if len(pair) == 2 else []
    c['stall'] = {'lagS': caught, 'underruns': delta(*pair, 'underruns') if len(pair) == 2 else None, 'stoppedS': stall.get('stoppedS'), 'holds': bool(caught)}
    g = load(out, 'guardtest.json') or {}
    c['guardKill'] = {k: g.get(k) for k in ('killed', 'appGone', 'wrongSinkPresent', 'wrongSinkSeen')}
    c['guardKill']['holds'] = bool(g.get('wrongSinkPresent') and g.get('appGone') and g.get('wrongSinkSeen') and 'stream linked to' in str(g.get('killed'))
                                   and g.get('defaultBefore') and g.get('defaultBefore') == g.get('defaultAfter'))
    empty_checks = checks(empty)
    c['missingArtifact'] = dict(found := {n: v['verdict'] for n, v in empty_checks.items()},
                                holds=all(v['verdict'] == 'FAIL' and v['reasons'] and all(r.startswith('missing evidence') for r in v['reasons'][:1]) for v in empty_checks.values()))
    c['holds'] = all(x['holds'] for x in c.values())
    return c


def verdict(out):
    build = load(out, 'build.json') or {}
    v = {'commit': build.get('commit'), 'dirty': build.get('dirty'), 'mode': build.get('mode'), 'binaries': build.get('sha256'),
         'reference': dict({n: medians((load(out, f'{n}.json') or {}).get('samples')) for n in REFERENCE_RUNS}, recordedPss=REFERENCE['pss']),
         'weston': weston_renderer(out), 'errors': load(out, 'errors.json') or {}, 'screenshots': (load(out, 'screenshots.json') or {})}
    real, leaks = load(out, 'realstate.json') or {}, load(out, 'leakgrep.json') or {}
    v['realStateUnchanged'] = bool(real) and real.get('before', {}).get('keyringItems') is not None and real.get('before') == real.get('after')
    v['leakGrep'] = dict(leaks, clean=leaks.get('hits') == {'repo': 0, 'run': 0} and leaks.get('plantedCaught') is True and (leaks.get('runFiles') or 0) > 0)
    v['phaseErrors'] = {k: e for k, e in v['errors'].items() if not e.startswith('RuntimeError: skipped: the guard fired')}
    safe = v['realStateUnchanged'] and v['leakGrep']['clean'] and not v['phaseErrors']
    if build.get('mode') == 'controls':
        v['controls'] = controls(out, build)
        v['gate'] = 'PASS' if safe and v['controls']['holds'] else 'FAIL'
        return v
    v['checks'] = checks(out)
    if build.get('mode') == 'llvmpipe':
        renderers = {str(r.get('glRenderer')) for r in (load(out, 'shipped.r2.json') or {}).get('samples', [])}
        named = ('F1', 'F4', 'R1', 'R2')
        holds = all(v['checks'][c]['verdict'] == 'VOID' and any('renderer' in r for r in v['checks'][c]['reasons']) for c in named)
        v['llvmpipeControl'] = {'holds': holds and any('llvmpipe' in g for g in renderers), 'renderers': sorted(renderers)}
        v['gate'] = 'PASS' if safe and v['llvmpipeControl']['holds'] else 'FAIL'
        return v
    v['ballast'] = ballast(out, build)
    seen = [c['verdict'] for c in v['checks'].values()]
    v['gate'] = 'FAIL' if not safe else ('VOID' if 'VOID' in seen else 'FAIL' if 'FAIL' in seen else 'PASS' if v['ballast']['holds']
                 else 'VOID' if v['ballast']['verdict'] == 'VOID' else 'FAIL')
    return v


def brief(v):
    keep = lambda c: {k: x for k, x in c.items() if k not in ('reasons',) or x}
    return dict(gate=v['gate'], realStateUnchanged=v['realStateUnchanged'], leakGrep=v['leakGrep'], checks={n: keep(c) for n, c in (v.get('checks') or {}).items()},
                ballast=v.get('ballast') and keep(v['ballast']), reference=v['reference'], controls=v.get('controls'), llvmpipeControl=v.get('llvmpipeControl'), errors=v['errors'])


if __name__ == '__main__':
    import sys
    if sys.argv[1] == '--preflight':
        found = preflight()
        print('\n'.join(['refusing: other GPU clients are running:'] + found) if found else 'preflight: no other weston, bramley or GPU client')
        sys.exit(2 if found else 0)
    result = verdict(sys.argv[1])
    save(sys.argv[1], 'verdict.json', result)
    print(json.dumps(brief(result), indent=1))
    sys.exit(0 if result['gate'] == 'PASS' else 1)
