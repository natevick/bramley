import argparse, glob, hashlib, importlib.util, json, math, os, statistics, subprocess, sys, time

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))
_spec = importlib.util.spec_from_file_location('covers_session', os.path.join(ROOT, 'scripts', 'app', 'covers-session.py'))
cs = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(cs)

REFERENCE = {'binary': os.path.join(ROOT, 'target', 'reference', 'bramley-main-4961e27'), 'sha256': '1a72eb4b2880ac5a', 'pss': 83.3}
R1_BAR, R2_BAR, DRIFT_BAR, BALLAST_OVER, LANDING_BAND = 200.0, 100.0, 5.0, 15.0, 5.0
COVERS, ADVANCE_S, PLAY_WARM_S, LINK_CONFIRM_S, MIN_WINDOW_POLLS, RISE_TOLERANCE = 50, 50.0, 15, 2, 30, 3.0
REFERENCE_RUNS = ('reference', 'reference-after')
R1_SAMPLES, R2_SAMPLES, R2_SETTLE_S, REF_SAMPLES, REF_SETTLE_S = 60, 20, 5, 20, 12


def sha(path):
    return hashlib.sha256(open(path, 'rb').read()).hexdigest()[:16]


def load(out, name):
    try:
        with open(os.path.join(out, name)) as f:
            return json.load(f)
    except (OSError, ValueError):
        return None


def save(out, name, data):
    with open(os.path.join(out, name), 'w') as f:
        json.dump(data, f, indent=1)


def now(binary, env):
    r = subprocess.run([binary, 'now'], env=env, capture_output=True, text=True, timeout=10)
    try:
        return json.loads(r.stdout) if r.returncode == 0 else None
    except ValueError:
        return None


def mem(pid):
    pids = [pid] + cs.children(pid)
    rolls = [cs.rollup(p) for p in pids]
    total = lambda *keys: round(sum(r.get(k, 0) for r in rolls for k in keys) / 1024, 2)
    return {'pss': total('Pss'), 'uss': total('Private_Clean', 'Private_Dirty'), 'rss': total('Rss'), 'procs': len(pids)}


SNAP_KEYS = ('pid', 'state', 'position', 'banner', 'engineStarted', 'texturesUploaded', 'coversFailed', 'framesPainted', 'glRenderer', 'applied', 'seq')
STANDING_GPU_CLIENTS = ('gnome-shell', 'Xvfb', 'grok-bot')
GPU_LIBS = ('libgallium', 'radeonsi', 'libLLVM')


def preflight():
    found = []
    for d in glob.glob('/proc/[0-9]*'):
        try:
            comm = open(f'{d}/comm').read().strip()
            libs = sorted({line.split()[-1].rsplit('/', 1)[-1] for line in open(f'{d}/maps') if any(lib in line for lib in GPU_LIBS)})
        except OSError:
            libs = []
        if int(d[6:]) != os.getpid() and (comm in ('weston', 'bramley') or (libs and comm not in STANDING_GPU_CLIENTS)):
            found.append(f'{d[6:]} {comm} {libs}')
    return found


def sample(pid, binary, env, n):
    rows = []
    for _ in range(n):
        t = time.time()
        s = now(binary, env) or {}
        rows.append(dict(mem(pid), t=round(t, 2), **{k: s.get(k) for k in SNAP_KEYS}))
        time.sleep(max(0.0, 1 - (time.time() - t)))
    return rows


def medians(rows):
    return {k: round(statistics.median(r[k] for r in rows), 1) for k in ('pss', 'uss', 'rss')} if rows else {}


def env_for(sock, sink, llvmpipe=False, **extra):
    env = dict(os.environ, WAYLAND_DISPLAY=sock, PULSE_SINK=sink, TMPDIR=os.path.join(ROOT, 'target', 'tmp'), **extra)
    env.pop('DISPLAY', None)
    if llvmpipe:
        env['LIBGL_ALWAYS_SOFTWARE'] = '1'
    return env


def launch(binary, env, log_path):
    cs.assert_isolated(env)
    rc = subprocess.run([binary, 'now'], env=env, capture_output=True).returncode
    if rc != 3:
        raise RuntimeError(f'slot held before launch (now exit {rc}); refusing')
    log = open(log_path, 'w')
    return subprocess.Popen([binary], env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True), log


def stop(p, log):
    if p.poll() is None:
        p.terminate()
        try:
            p.wait(15)
        except subprocess.TimeoutExpired:
            p.kill()
            p.wait()
    log.close()


def reference(out, sock, sink, name):
    with cs.isolated(f'{os.path.basename(out)}-{name}', True) as (iso, _):
        reference_in(out, sock, sink, name, env_for(sock, sink, **iso))


def reference_in(out, sock, sink, name, env):
    binary = REFERENCE['binary']
    ev = {'binary': os.path.basename(binary), 'sha256': sha(binary)}
    p, log = launch(binary, env, os.path.join(out, f'{name}.log'))
    ev['pid'] = p.pid
    try:
        cs.wait_for(lambda: (lambda s: s if s and s.get('framesPainted', 0) > 0 else None)(now(binary, env)), 30, 'reference first frame')
        time.sleep(REF_SETTLE_S)
        ev['samples'] = sample(p.pid, binary, env, REF_SAMPLES)
    finally:
        stop(p, log)
        save(out, f'{name}.json', ev)


def app_run(out, name, binary, sock, sink, llvmpipe, ballast_mib=None):
    with cs.isolated(f'{os.path.basename(out)}-{name}', True) as (iso, _):
        app_run_in(out, name, binary, sock, sink, llvmpipe, ballast_mib, iso)


def app_run_in(out, name, binary, sock, sink, llvmpipe, ballast_mib, iso):
    extra = {} if ballast_mib is None else {'BRAMLEY_GATE_BALLAST_MIB': str(ballast_mib)}
    env = env_for(sock, sink, llvmpipe, **extra, **iso)
    p, log = launch(binary, env, os.path.join(out, f'{name}.log'))
    guard = cs.Guard(p.pid, sink)
    guard.start()
    r1 = None
    try:
        cs.wait_for(lambda: (lambda s: s if s and s.get('framesPainted', 0) > 0 and (s.get('texturesUploaded', 0) >= COVERS or s.get('coversFailed')) else None)(now(binary, env)), 60, 'first frame and covers')
        time.sleep(R2_SETTLE_S)
        save(out, f'{name}.r2.json', {'binary': os.path.basename(binary), 'pid': p.pid, 'samples': sample(p.pid, binary, env, R2_SAMPLES)})
        r1 = {'binary': os.path.basename(binary), 'pid': p.pid, 'ballastMiB': ballast_mib, 'gpuBefore': cs.gpu(), 'samples': []}
        cs.wait_for(lambda: guard.polls > 0, 10, 'a clean guard poll')
        r1['prePlay'] = {'sinkExists': cs.sink_exists(sink), 'default': cs.default_sink(), 'guardPolls': guard.polls, 'guardKilled': bool(guard.killed)}
        if not r1['prePlay']['sinkExists'] or r1['prePlay']['default'] in (None, sink) or guard.killed:
            raise RuntimeError(f'audio not safe before play: {r1["prePlay"]}')
        played_at = time.time()
        ack = subprocess.run([binary, 'play', cs.SONG], env=env, capture_output=True, text=True, timeout=15)
        r1['playExit'] = ack.returncode
        try:
            cs.wait_for(lambda: (lambda s: s and s.get('state') == 'Playing' and (s.get('position') or 0) > 0)(now(binary, env)), 90, 'audio')
        except RuntimeError:
            r1['started'] = False
            return
        r1['playToAudioS'] = round(time.time() - played_at, 2)
        try:
            cs.wait_for(lambda: any(t >= played_at and sink in targets for t, targets in list(guard.log)), LINK_CONFIRM_S, 'a null-sink link')
        except RuntimeError:
            guard.kill(f'no null-sink link within {LINK_CONFIRM_S} s of audio')
            raise
        warm = lambda s: s and s.get('state') == 'Playing' and s.get('banner') is None and (s.get('position') or 0) > PLAY_WARM_S and s.get('applied', 0) >= s.get('seq', 1)
        try:
            cs.wait_for(lambda: warm(now(binary, env)), 90, f'playing past {PLAY_WARM_S} s')
            r1['started'] = True
        except RuntimeError:
            r1['started'] = False
        r1['samples'] = sample(p.pid, binary, env, R1_SAMPLES) if r1['started'] else []
        r1['gpuAfter'] = cs.gpu()
    finally:
        stop(p, log)
        guard.stop = True
        guard.join(2)
        save(out, f'{name}.guard.json', {'pid': p.pid, 'polls': guard.polls, 'killed': guard.killed and guard.killed['why'], 'log': guard.log})
        if r1 is not None:
            save(out, f'{name}.r1.json', r1)


def ballast_size(out):
    rows = (load(out, 'shipped.r1.json') or {}).get('samples')
    return max(0, math.ceil(R1_BAR + BALLAST_OVER - medians(rows)['uss'])) if rows else None


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
    missing, fails, voids, drifts = [], [], [], []
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
    return missing, fails, voids, drifts


def audio_voids(ev, guard, sink):
    rows = ev['samples']
    positions = [r.get('position') or 0 for r in rows]
    if not rows or positions[-1] - positions[0] < ADVANCE_S:
        return []
    window = [targets for t, targets in guard.get('log', []) if rows[0]['t'] <= t <= rows[-1]['t']]
    if len(window) < MIN_WINDOW_POLLS:
        return [f'guard observed {len(window)} polls in the R1 window < {MIN_WINDOW_POLLS}']
    off = sum(1 for targets in window if targets != [sink])
    return [f'{off} of {len(window)} polls in the R1 window not linked to the null sink only'] if off else []


def check(out, name, kind):
    missing, fails, voids, drifts = session_problems(out)
    ev, guard = load(out, f'{name}.{kind}.json'), load(out, f'{name}.guard.json')
    missing += [f'missing evidence: {f}' for f, d in ((f'{name}.{kind}.json', ev), (f'{name}.guard.json', guard)) if d is None]
    if missing:
        return {'verdict': 'FAIL', 'reasons': missing + fails + voids}
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
        voids += audio_voids(ev, guard, (load(out, 'session.verdict.json') or {}).get('sink'))
        positions = [r.get('position') or 0 for r in rows]
        advanced = positions[-1] - positions[0] if rows else 0
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
    zone = None
    if m:
        zone = 1 if m['uss'] > bar else 2 if m['rss'] <= bar else 3
        if zone == 1:
            fails.append(f'USS median {m["uss"]} > {bar}')
        if zone == 3 and m['pss'] > bar:
            fails.append(f'PSS median {m["pss"]} > {bar}')
    outcome = 'VOID' if voids else 'FAIL' if fails else 'PASS'
    if zone == 3:
        suspect = {'FAIL': [d for d in drifts if d > DRIFT_BAR], 'PASS': [d for d in drifts if d < -DRIFT_BAR]}.get(outcome, [])
        voids += [f'reference drifted {d:+.1f} MiB from {REFERENCE["pss"]} (bar {DRIFT_BAR}), which voids a {outcome}' for d in suspect]
    return dict(m, verdict='VOID' if voids else outcome, reasons=voids + fails, bar=bar, zone=zone, referenceDrift=max(drifts, key=abs))


def verdict(out):
    build = load(out, 'build.json') or {}
    refs = {n: medians((load(out, f'{n}.json') or {}).get('samples')) for n in REFERENCE_RUNS}
    v = {'commit': build.get('commit'), 'dirty': build.get('dirty'), 'llvmpipeForced': build.get('llvmpipe'), 'binaries': build.get('sha256'),
         'reference': dict(refs, recordedPss=REFERENCE['pss']), 'weston': weston_renderer(out),
         'R1': check(out, 'shipped', 'r1'), 'R2': check(out, 'shipped', 'r2'), 'errors': load(out, 'errors.json') or {},
         'screenshotExists': (load(out, 'screenshot.json') or {}).get('screenshotExists', False)}
    shipped, ballast = load(out, 'shipped.r1.json') or {}, load(out, 'ballast.r1.json') or {}
    gpu = lambda k: shipped.get(k, {}).get('mem_info_vram_used')
    if gpu('gpuBefore') is not None and gpu('gpuAfter') is not None:
        v['vramDeltaMiB'] = round((gpu('gpuAfter') - gpu('gpuBefore')) / 2**20, 1)
    b = check(out, 'ballast', 'r1')
    shipped_uss = medians(shipped.get('samples')).get('uss')
    rise = round(b['uss'] - shipped_uss, 1) if 'uss' in b and shipped_uss is not None else None
    threshold_only = b['verdict'] == 'FAIL' and len(b['reasons']) == 1 and b['reasons'][0].startswith('USS median')
    landed = 'uss' in b and abs(b['uss'] - (R1_BAR + BALLAST_OVER)) <= LANDING_BAND
    flagged = build.get('ballastFeaturesExit') == 1 and 'app/gate-sabotage' in (build.get('ballastFeaturesOutput') or '')
    rose = rise is not None and ballast.get('ballastMiB') is not None and abs(rise - ballast['ballastMiB']) <= RISE_TOLERANCE
    v['ballast'] = dict(b, ballastMiB=ballast.get('ballastMiB'), ussRise=rise, holds=threshold_only and landed and flagged and rose,
                        thresholdOnly=threshold_only, landedWithinBand=landed, failsShippedCheck=flagged, ussRoseByBallast=rose)
    if build.get('llvmpipe'):
        renderers = {str(r.get('glRenderer')) for r in (load(out, 'shipped.r2.json') or {}).get('samples', [])}
        v['llvmpipeControl'] = {'holds': all(v[c]['verdict'] == 'VOID' and any('renderer' in r for r in v[c]['reasons']) for c in ('R1', 'R2')) and any('llvmpipe' in g for g in renderers), 'renderers': sorted(renderers)}
    real = load(out, 'realstate.json') or {}
    v['realStateUnchanged'] = bool(real) and real.get('before', {}).get('keyringItems') is not None and real.get('before') == real.get('after')
    checks = (v['R1']['verdict'], v['R2']['verdict'])
    v['gate'] = 'FAIL' if not v['realStateUnchanged'] else ('VOID' if 'VOID' in checks else 'FAIL' if 'FAIL' in checks else 'PASS' if v['ballast']['holds']
                 else 'VOID' if v['ballast']['verdict'] == 'VOID' else 'FAIL')
    return v


def ballast_run(out, bins, sock, sink, llvmpipe):
    mib = ballast_size(out)
    if mib is None:
        raise RuntimeError('no shipped R1 median to size the ballast from')
    app_run(out, 'ballast', os.path.join(bins, 'bramley-ballast'), sock, sink, llvmpipe, mib)


def screenshot(out, bins, sock, sink):
    res = cs.run_one(f'acceptance-{os.path.basename(out)}', os.path.join(bins, 'bramley-dev'), 'shot', argparse.Namespace(out=out), sock, sink)
    save(out, 'screenshot.json', {k: res.get(k) for k in ('binary', 'sha256', 'screenshot', 'screenshotExists', 'error')})


def guard_fired(out):
    return [n for n in ('shipped', 'ballast') if (load(out, f'{n}.guard.json') or {}).get('killed')]


def real_state():
    token = os.path.join(cs.REAL_CONFIG, 'music-user-token')
    st = os.stat(token) if os.path.exists(token) else None
    cache = {f: [s.st_mtime_ns, s.st_size] for f in sorted(os.listdir(cs.REAL_CACHE)) for s in [os.stat(os.path.join(cs.REAL_CACHE, f))]} if os.path.isdir(cs.REAL_CACHE) else None
    return {'tokenMtimeNs': st and st.st_mtime_ns, 'tokenSize': st and st.st_size, 'keyringItems': cs.keyring_items('bramley'), 'cache': cache}


def run(out, llvmpipe):
    os.makedirs(os.path.join(ROOT, 'target', 'tmp'), exist_ok=True)
    before = real_state()
    bins = os.path.join(out, 'bin')
    build = load(out, 'build.json') or {}
    build['sha256'] = {b: sha(os.path.join(bins, b)) for b in sorted(os.listdir(bins))}
    save(out, 'build.json', build)
    errors = {}
    with cs.session(out, 'bramley-gate') as (sock, sink, _):
        for phase, plays, go in (('reference', False, lambda: reference(out, sock, sink, 'reference')),
                                 ('shipped', True, lambda: app_run(out, 'shipped', os.path.join(bins, 'bramley'), sock, sink, llvmpipe)),
                                 ('ballast', True, lambda: ballast_run(out, bins, sock, sink, llvmpipe)),
                                 ('reference-after', False, lambda: reference(out, sock, sink, 'reference-after')),
                                 ('screenshot', True, lambda: screenshot(out, bins, sock, sink))):
            try:
                if plays and guard_fired(out):
                    raise RuntimeError(f'skipped: the guard fired in {guard_fired(out)}')
                go()
            except Exception as e:
                errors[phase] = f'{type(e).__name__}: {e}'.replace(ROOT, '.').replace(os.path.expanduser('~'), '~')[:300]
            save(out, 'errors.json', errors)
    save(out, 'realstate.json', {'before': before, 'after': real_state()})


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--out')
    ap.add_argument('--llvmpipe', action='store_true')
    ap.add_argument('--verdict-only', action='store_true')
    ap.add_argument('--preflight', action='store_true')
    a = ap.parse_args()
    if a.preflight:
        found = preflight()
        print('\n'.join(['refusing: other GPU clients are running:'] + found) if found else 'preflight: no other weston, bramley or GPU client')
        sys.exit(2 if found else 0)
    if not a.verdict_only:
        run(a.out, a.llvmpipe)
    v = verdict(a.out)
    save(a.out, 'verdict.json', v)
    brief = lambda c: {k: c.get(k) for k in ('verdict', 'zone', 'pss', 'uss', 'rss', 'referenceDrift', 'reasons', 'ballastMiB', 'ussRise', 'holds') if k in c}
    print(json.dumps(dict(gate=v['gate'], realStateUnchanged=v['realStateUnchanged'], R1=brief(v['R1']), R2=brief(v['R2']), ballast=brief(v['ballast']), reference=v['reference'], llvmpipeControl=v.get('llvmpipeControl'), errors=v['errors']), indent=1))
    sys.exit(0 if v['gate'] == 'PASS' else 1)


if __name__ == '__main__':
    main()
