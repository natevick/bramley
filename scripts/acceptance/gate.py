import argparse, hashlib, importlib.util, json, math, os, statistics, subprocess, sys, time

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))
_spec = importlib.util.spec_from_file_location('covers_session', os.path.join(ROOT, 'scripts', 'app', 'covers-session.py'))
cs = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(cs)

REFERENCE = {'binary': os.path.join(ROOT, 'target', 'reference', 'bramley-main-4961e27'), 'sha256': '1a72eb4b2880ac5a', 'pss': 83.3}
R1_BAR, R2_BAR, DRIFT_BAR, BALLAST_OVER, LANDING_BAND = 200.0, 100.0, 5.0, 15.0, 5.0
COVERS, ADVANCE_S, PLAY_WARM_S = 50, 50.0, 15
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


SNAP_KEYS = ('state', 'position', 'banner', 'engineStarted', 'texturesUploaded', 'coversFailed', 'framesPainted', 'glRenderer', 'applied', 'seq')


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


def reference(out, sock, sink):
    env = env_for(sock, sink)
    binary = REFERENCE['binary']
    ev = {'binary': os.path.basename(binary), 'sha256': sha(binary), 'recordedSha256': REFERENCE['sha256'], 'recordedPss': REFERENCE['pss']}
    p, log = launch(binary, env, os.path.join(out, 'reference.log'))
    try:
        ready = cs.wait_for(lambda: (lambda s: s if s and s.get('framesPainted', 0) > 0 else None)(now(binary, env)), 30, 'reference first frame')
        time.sleep(REF_SETTLE_S)
        ev['samples'] = sample(p.pid, binary, env, REF_SAMPLES)
        ev.update(medians(ev['samples']), glRenderer=ready.get('glRenderer'))
    finally:
        stop(p, log)
        save(out, 'reference.json', ev)


def app_run(out, name, binary, sock, sink, llvmpipe, ballast_mib=None):
    extra = {} if ballast_mib is None else {'BRAMLEY_GATE_BALLAST_MIB': str(ballast_mib)}
    env = env_for(sock, sink, llvmpipe, **extra)
    p, log = launch(binary, env, os.path.join(out, f'{name}.log'))
    guard = cs.Guard(p.pid, sink)
    guard.start()
    r1 = None
    try:
        cs.wait_for(lambda: (lambda s: s if s and s.get('framesPainted', 0) > 0 and (s.get('texturesUploaded', 0) >= COVERS or s.get('coversFailed')) else None)(now(binary, env)), 60, 'first frame and covers')
        time.sleep(R2_SETTLE_S)
        rows = sample(p.pid, binary, env, R2_SAMPLES)
        save(out, f'{name}.r2.json', dict(medians(rows), binary=os.path.basename(binary), samples=rows))
        r1 = {'binary': os.path.basename(binary), 'ballastMiB': ballast_mib, 'gpuBefore': cs.gpu()}
        cs.wait_for(lambda: guard.polls > 0, 10, 'a clean guard poll')
        r1['prePlay'] = {'sinkExists': cs.sink_exists(sink), 'default': cs.default_sink(), 'guardPolls': guard.polls, 'guardKilled': bool(guard.killed)}
        if not r1['prePlay']['sinkExists'] or r1['prePlay']['default'] == sink or guard.killed:
            raise RuntimeError(f'audio not safe before play: {r1["prePlay"]}')
        ack = subprocess.run([binary, 'play', cs.SONG], env=env, capture_output=True, text=True, timeout=15)
        r1['playExit'] = ack.returncode
        warm = lambda s: s and s.get('state') == 'Playing' and s.get('banner') is None and (s.get('position') or 0) > PLAY_WARM_S and s.get('applied', 0) >= s.get('seq', 1)
        try:
            cs.wait_for(lambda: warm(now(binary, env)), 90, f'playing past {PLAY_WARM_S} s')
            r1['started'] = True
        except RuntimeError:
            r1['started'] = False
        r1['samples'] = sample(p.pid, binary, env, R1_SAMPLES) if r1['started'] else []
        r1.update(medians(r1['samples']), gpuAfter=cs.gpu())
    finally:
        stop(p, log)
        guard.stop = True
        guard.join(2)
        if r1 is not None:
            r1.update(guardSeen=sorted(str(t) for t in guard.seen), guardKilled=guard.killed and guard.killed['why'], defaultAfter=cs.default_sink())
            save(out, f'{name}.r1.json', r1)


def ballast_size(out):
    r1 = load(out, 'shipped.r1.json')
    if not r1 or not r1.get('samples'):
        return None
    return max(0, math.ceil(R1_BAR + BALLAST_OVER - r1['pss']))


def renderer_voids(rows, what):
    seen = sorted({str(r.get('glRenderer')) for r in rows})
    bad = [g for g in seen if 'radeonsi' not in g or 'llvmpipe' in g.lower()]
    return [f'{what} renderer is not radeonsi: {bad}'] if bad or not seen else []


def session_problems(out):
    fails, voids = [], []
    build, sess, ref = load(out, 'build.json'), load(out, 'session.verdict.json'), load(out, 'reference.json')
    if not build:
        fails.append('missing evidence: build.json')
    elif build.get('shippedFeaturesExit') != 0:
        fails.append(f'shipped binary failed check-shipped-features (exit {build.get("shippedFeaturesExit")})')
    if not sess:
        fails.append('missing evidence: session.verdict.json')
    else:
        if sess.get('defaultAtStart') != sess.get('defaultAtEnd'):
            voids.append(f'default sink changed: {sess.get("defaultAtStart")} -> {sess.get("defaultAtEnd")}')
        if not sess.get('sinkGone'):
            voids.append('null sink left behind')
    if not ref or not ref.get('samples'):
        fails.append('missing evidence: reference.json')
    else:
        if ref.get('sha256') != REFERENCE['sha256']:
            voids.append(f'reference sha {ref.get("sha256")} is not the recorded {REFERENCE["sha256"]}')
        if abs(ref['pss'] - REFERENCE['pss']) > DRIFT_BAR:
            voids.append(f'reference drifted {ref["pss"] - REFERENCE["pss"]:+.1f} MiB from {REFERENCE["pss"]} (bar {DRIFT_BAR})')
        voids += renderer_voids(ref['samples'], 'reference')
    return fails, voids


def check(out, name, kind):
    ev = load(out, f'{name}.{kind}.json')
    fails, voids = session_problems(out)
    if ev is None:
        return {'verdict': 'FAIL', 'reasons': [f'missing evidence: {name}.{kind}.json'] + fails + voids}
    rows = ev.get('samples') or []
    voids += renderer_voids(rows, name) if rows else []
    if kind == 'r2':
        bar, want = R2_BAR, R2_SAMPLES
        if any(r.get('engineStarted') for r in rows):
            fails.append('engine started before the first play')
    else:
        bar, want = R1_BAR, R1_SAMPLES
        positions = [r.get('position') or 0 for r in rows]
        advanced = positions[-1] - positions[0] if rows else 0
        if ev.get('guardKilled'):
            voids.append(f'guard killed the app: {ev["guardKilled"]}')
        if advanced >= ADVANCE_S and not ev.get('guardSeen'):
            voids.append('playback advanced but no stream linked to the null sink')
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
    if rows and ev['pss'] > bar:
        fails.append(f'PSS median {ev["pss"]} > {bar}')
    res = {'verdict': 'VOID' if voids else 'FAIL' if fails else 'PASS', 'reasons': voids + fails, 'bar': bar}
    res.update({k: ev.get(k) for k in ('pss', 'uss', 'rss') if k in ev})
    return res


def verdict(out):
    build = load(out, 'build.json') or {}
    ref = load(out, 'reference.json') or {}
    v = {'commit': build.get('commit'), 'dirty': build.get('dirty'), 'llvmpipeForced': build.get('llvmpipe'), 'binaries': build.get('sha256'),
         'reference': {k: ref.get(k) for k in ('binary', 'sha256', 'recordedPss', 'pss', 'uss', 'rss')},
         'R1': check(out, 'shipped', 'r1'), 'R2': check(out, 'shipped', 'r2'), 'errors': load(out, 'errors.json') or {},
         'screenshotExists': (load(out, 'screenshot.json') or {}).get('screenshotExists', False)}
    shipped, ballast = load(out, 'shipped.r1.json') or {}, load(out, 'ballast.r1.json') or {}
    gpu = lambda k: shipped.get(k, {}).get('mem_info_vram_used')
    if gpu('gpuBefore') is not None and gpu('gpuAfter') is not None:
        v['vramDeltaMiB'] = round((gpu('gpuAfter') - gpu('gpuBefore')) / 2**20, 1)
    b = check(out, 'ballast', 'r1')
    expected = ballast_size(out)
    threshold_only = b['verdict'] == 'FAIL' and len(b['reasons']) == 1 and b['reasons'][0].startswith('PSS median')
    landed = b.get('pss') is not None and abs(b['pss'] - (R1_BAR + BALLAST_OVER)) <= LANDING_BAND
    flagged = build.get('ballastFeaturesExit') == 1 and 'app/gate-sabotage' in (build.get('ballastFeaturesOutput') or '')
    sized = expected is not None and ballast.get('ballastMiB') == expected
    v['ballast'] = dict(b, ballastMiB=ballast.get('ballastMiB'), expectedMiB=expected, holds=threshold_only and landed and flagged and sized,
                        thresholdOnly=threshold_only, landedWithinBand=landed, failsShippedCheck=flagged, sizedFromThisRun=sized)
    if build.get('llvmpipe'):
        renderers = {str(r.get('glRenderer')) for r in (load(out, 'shipped.r2.json') or {}).get('samples', [])}
        v['llvmpipeControl'] = {'holds': all(v[c]['verdict'] == 'VOID' and any('renderer' in r for r in v[c]['reasons']) for c in ('R1', 'R2')) and any('llvmpipe' in g for g in renderers), 'renderers': sorted(renderers)}
    v['gate'] = 'PASS' if v['R1']['verdict'] == v['R2']['verdict'] == 'PASS' and v['ballast']['holds'] else 'VOID' if 'VOID' in (v['R1']['verdict'], v['R2']['verdict']) else 'FAIL'
    return v


def ballast_run(out, bins, sock, sink, llvmpipe):
    mib = ballast_size(out)
    if mib is None:
        raise RuntimeError('no shipped R1 median to size the ballast from')
    app_run(out, 'ballast', os.path.join(bins, 'bramley-ballast'), sock, sink, llvmpipe, mib)


def screenshot(out, bins, sock, sink):
    res = cs.run_one(f'acceptance-{os.path.basename(out)}', os.path.join(bins, 'bramley-dev'), 'shot', argparse.Namespace(out=out), sock, sink)
    save(out, 'screenshot.json', {k: res.get(k) for k in ('binary', 'sha256', 'screenshot', 'screenshotExists', 'error')})


def run(out, llvmpipe):
    os.makedirs(os.path.join(ROOT, 'target', 'tmp'), exist_ok=True)
    bins = os.path.join(out, 'bin')
    build = load(out, 'build.json') or {}
    build['sha256'] = {b: sha(os.path.join(bins, b)) for b in sorted(os.listdir(bins))}
    save(out, 'build.json', build)
    errors = {}
    with cs.session(out, 'bramley-gate') as (sock, sink, _):
        for phase, go in (('reference', lambda: reference(out, sock, sink)),
                          ('shipped', lambda: app_run(out, 'shipped', os.path.join(bins, 'bramley'), sock, sink, llvmpipe)),
                          ('ballast', lambda: ballast_run(out, bins, sock, sink, llvmpipe)),
                          ('screenshot', lambda: screenshot(out, bins, sock, sink))):
            try:
                go()
            except Exception as e:
                errors[phase] = f'{type(e).__name__}: {e}'
            save(out, 'errors.json', errors)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--out', required=True)
    ap.add_argument('--llvmpipe', action='store_true')
    ap.add_argument('--verdict-only', action='store_true')
    a = ap.parse_args()
    if not a.verdict_only:
        run(a.out, a.llvmpipe)
    v = verdict(a.out)
    save(a.out, 'verdict.json', v)
    brief = lambda c: {k: c.get(k) for k in ('verdict', 'pss', 'uss', 'reasons', 'holds') if k in c}
    print(json.dumps(dict(gate=v['gate'], R1=brief(v['R1']), R2=brief(v['R2']), ballast=brief(v['ballast']), reference=v['reference'], llvmpipeControl=v.get('llvmpipeControl'), errors=v['errors']), indent=1))
    sys.exit(0 if v['gate'] == 'PASS' else 1)


if __name__ == '__main__':
    main()
