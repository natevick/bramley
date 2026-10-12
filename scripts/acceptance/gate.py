import json, math, os, statistics

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))
REFERENCE = {'binary': os.path.join(ROOT, 'target', 'reference', 'bramley-main-4961e27'), 'sha256': '1a72eb4b2880ac5a', 'pss': 88.6}
R1_BAR, R2_BAR, R3_BAR, R4_BAR, R5_BAR, DRIFT_BAR, BALLAST_OVER, LANDING_BAND = 200.0, 100.0, 10.0, 5.0, 1.0, 5.0, 15.0, 5.0
COVERS, ADVANCE_S, MIN_WINDOW_POLLS, RISE_TOLERANCE, REPS = 50, 50.0, 30, 3.0, 5
F1_MS, F3_MS, F4_MS, F5_MS, F4_PLAYED_S, FULL_SONG_S, R3_TRACKS, R3_STALL_S = 500, 2000, 3000, 500, 35.0, 300.0, 8, 2.0
REFERENCE_RUNS = ('reference', 'reference-after')
R1_SAMPLES, R2_SAMPLES, REF_SAMPLES, CPU_SAMPLES = 60, 20, 20, 30
F5_COMMANDS, EVERY_REP = ('pause', 'seek', 'resume', 'next', 'prev'), ('pause', 'resume')
TOOLS = sorted('search play pause resume next previous seek queue_add now_playing library_playlists'.split())
CHECKS = ('F1', 'F2', 'F3', 'F4', 'F5', 'F6', 'F7', 'F8', 'F9', 'F10', 'R1', 'R2', 'R3', 'R4', 'R5', 'R6')
EVIDENCE = {'F1': ('f1',), 'F2': ('f2',), 'F3': ('f3',), 'F4': ('f4',), 'F5': ('f5',), 'F6': ('f6',), 'F7': ('f7',), 'F8': ('f8',), 'F9': ('f9',),
            'F10': ('f10-badid', 'f10-net'), 'R3': ('r3',), 'R4': ('r4',), 'R5': ('r5',)}


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
    guard = load(out, f'{ev.get("session")}.guard.json')
    if guard is None:
        return [], [f'missing evidence: {ev.get("session")}.guard.json']
    voids = [f'guard killed the app: {guard["killed"]}'] if guard.get('killed') else []
    window = [targets for t, targets in guard.get('log', []) if ev.get('t0', 0) <= t <= ev.get('t1', 0)]
    if started and (any(targets not in ([], [sink]) for targets in window) or [sink] not in window):
        voids.append('playback started but no stream was seen linked to the null sink only')
    return voids, []


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
    stalls = [round(b['t'] - a['t'] - (b['position'] - a['position']), 1) for a, b in zip(rows, rows[1:])
              if a.get('itemId') == b.get('itemId') and a.get('seeks') == b.get('seeks') and b['t'] - a['t'] - (b['position'] - a['position']) > R3_STALL_S]
    if stalls:
        fails.append(f'position stalled (underrun proxy): {stalls}')
    if g.get('uss') is not None:
        if g['uss'] > R3_BAR:
            fails.append(f'USS growth {g["uss"]} > {R3_BAR}')
        elif g['pss'] > R3_BAR:
            fails.append(f'PSS growth {g["pss"]} > {R3_BAR}')
    elif rows:
        fails.append('no samples at minute 5 or minute 30')
    return dict(g, zone=None if g.get('uss') is None else 1 if g['uss'] > R3_BAR else 3, tracks=played,
                changesAfter5=sum(1 for a, b in zip(rows, rows[1:]) if b['m'] >= 5 and a.get('itemId') != b.get('itemId')))


def audible(ev, fails):
    reps = [r.get('audioMs') for r in ev.get('reps') or []]
    final = ev.get('final') or {}
    if not ev.get('started'):
        fails.append('playback never started')
    if len(reps) < REPS or None in reps:
        fails.append(f'audio started in {sum(r is not None for r in reps)} of {REPS} plays')
    elif median(reps) > F4_MS:
        fails.append(f'audio median {median(reps)} ms > {F4_MS}')
    if final.get('itemId') != ev.get('song') or (final.get('position') or 0) <= F4_PLAYED_S or (final.get('sentMs') or 0) <= F4_PLAYED_S * 1000:
        fails.append(f'not past {F4_PLAYED_S} s of the asked song with output sent: {final}')
    if (final.get('duration') or 0) < FULL_SONG_S:
        fails.append(f'duration {final.get("duration")} < {FULL_SONG_S}: a preview, not the full song')
    return {'audioMedianMs': median(reps)}


def f_rules(check, ev, fails):
    if check == 'F1':
        starts = ev.get('starts') or []
        if len(starts) < REPS or any(s.get('ms') is None for s in starts):
            fails.append(f'{sum(s.get("ms") is not None for s in starts)} of {REPS} starts reached a first frame')
        elif median(s['ms'] for s in starts) > F1_MS:
            fails.append(f'first frame median {median(s["ms"] for s in starts)} ms > {F1_MS}')
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
        extra = audible(ev, fails)
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
            ms = [s.get('ms') for s in seen]
            if any(not s.get('matched') for s in seen):
                fails.append(f'{cmd}: a snapshot never matched (engine, seq) with the effect')
            elif cmd in EVERY_REP and rounds and max(ms) > F5_MS:
                fails.append(f'{cmd}: slowest {max(ms)} ms > {F5_MS}')
            elif rounds and median(ms) > F5_MS:
                fails.append(f'{cmd}: median {median(ms)} ms > {F5_MS}')
            out[cmd] = median(ms)
        return out
    if check == 'F6':
        moves = ev.get('transitions') or []
        if len(moves) < 3 or not all(t.get('pass') for t in moves):
            fails.append(f'{sum(bool(t.get("pass")) for t in moves)} of 3 transitions passed')
        return {}
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
        if ev.get('before') != state or ev.get('after') != state:
            fails.append(f'not {state} across the window: {ev.get("before")} -> {ev.get("after")}')
        if cpu and median(cpu) > bar:
            fails.append(f'CPU median {median(cpu)} % > {bar}')
        return {'cpuMedian': median(cpu), 'bar': bar}
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
        if 'session' in ev:
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


def real_fail(c):
    return c['verdict'] == 'FAIL' and not any(r.startswith('missing evidence') for r in c['reasons'])


def controls(out, build):
    sink, c, empty = (load(out, 'session.verdict.json') or {}).get('sink'), {}, os.path.join(out, 'empty')
    os.makedirs(empty, exist_ok=True)
    seen = lambda pre, names: {n: memory_check(out, f'{pre}shipped', 'r1', sink) if n == 'R1' else check(out, n, pre) for n in names}
    flagged = build.get('ballastFeaturesExit') == 1 and 'app/gate-sabotage' in (build.get('ballastFeaturesOutput') or '')
    nouser, badtoken, nostream = seen('nouser.', ('F2', 'F4', 'F5', 'F6', 'F9', 'R1')), seen('badtoken.', ('F3', 'F4')), seen('nostream.', ('F4',))
    leak = check(out, 'R3', 'leak.')
    c['noUserToken'] = dict({n: v['verdict'] for n, v in nouser.items()}, holds=nouser['F2']['verdict'] == 'PASS' and all(real_fail(v) for n, v in nouser.items() if n != 'F2'))
    c['badDeveloperToken'] = dict({n: v['verdict'] for n, v in badtoken.items()}, holds=all(real_fail(v) for v in badtoken.values()))
    c['noStream'] = {'F4': nostream['F4']['verdict'], 'holds': real_fail(nostream['F4']) and 'playback never started' in nostream['F4']['reasons']}
    c['leak'] = {'R3': leak['verdict'], 'zone': leak.get('zone'), 'uss': leak.get('uss'), 'changesAfter5': leak.get('changesAfter5'),
                 'holds': real_fail(leak) and leak.get('zone') == 1 and (leak.get('changesAfter5') or 0) >= 4 and flagged}
    c['missingArtifact'] = dict(found := {n: v['verdict'] for n, v in checks(empty).items()}, holds=all(v == 'FAIL' for v in found.values()))
    c['holds'] = all(x['holds'] for x in c.values())
    return c


def verdict(out):
    build = load(out, 'build.json') or {}
    v = {'commit': build.get('commit'), 'dirty': build.get('dirty'), 'mode': build.get('mode'), 'binaries': build.get('sha256'),
         'reference': dict({n: medians((load(out, f'{n}.json') or {}).get('samples')) for n in REFERENCE_RUNS}, recordedPss=REFERENCE['pss']),
         'weston': weston_renderer(out), 'errors': load(out, 'errors.json') or {}, 'screenshots': (load(out, 'screenshots.json') or {})}
    real = load(out, 'realstate.json') or {}
    v['realStateUnchanged'] = bool(real) and real.get('before', {}).get('keyringItems') is not None and real.get('before') == real.get('after')
    if build.get('mode') == 'controls':
        v['controls'] = controls(out, build)
        v['gate'] = 'PASS' if v['realStateUnchanged'] and v['controls']['holds'] else 'FAIL'
        return v
    v['checks'] = checks(out)
    if build.get('mode') == 'llvmpipe':
        renderers = {str(r.get('glRenderer')) for r in (load(out, 'shipped.r2.json') or {}).get('samples', [])}
        named = ('F1', 'F4', 'R1', 'R2')
        holds = all(v['checks'][c]['verdict'] == 'VOID' and any('renderer' in r for r in v['checks'][c]['reasons']) for c in named)
        v['llvmpipeControl'] = {'holds': holds and any('llvmpipe' in g for g in renderers), 'renderers': sorted(renderers)}
        v['gate'] = 'PASS' if v['realStateUnchanged'] and v['llvmpipeControl']['holds'] else 'FAIL'
        return v
    v['ballast'] = ballast(out, build)
    seen = [c['verdict'] for c in v['checks'].values()]
    v['gate'] = 'FAIL' if not v['realStateUnchanged'] else ('VOID' if 'VOID' in seen else 'FAIL' if 'FAIL' in seen else 'PASS' if v['ballast']['holds']
                 else 'VOID' if v['ballast']['verdict'] == 'VOID' else 'FAIL')
    return v


def brief(v):
    keep = lambda c: {k: x for k, x in c.items() if k not in ('reasons',) or x}
    return dict(gate=v['gate'], realStateUnchanged=v['realStateUnchanged'], checks={n: keep(c) for n, c in (v.get('checks') or {}).items()},
                ballast=v.get('ballast') and keep(v['ballast']), reference=v['reference'], controls=v.get('controls'), llvmpipeControl=v.get('llvmpipeControl'), errors=v['errors'])


if __name__ == '__main__':
    import sys
    result = verdict(sys.argv[1])
    save(sys.argv[1], 'verdict.json', result)
    print(json.dumps(brief(result), indent=1))
    sys.exit(0 if result['gate'] == 'PASS' else 1)
