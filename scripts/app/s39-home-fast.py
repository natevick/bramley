import argparse, hashlib, importlib.util, json, os, statistics, subprocess, time

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))
SPACING, BUDGET, CAP, SETTLE, STABLE, POLL, MEM_EVERY, TARGET = 20, 20, 60, 6, 4, 0.05, 0.2, 50
CACHE_KEYS = ('apiHits', 'apiMisses', 'apiRevalidations', 'artHits', 'artMisses', 'homeHits')
ORDER = 'ABC'
SPAWNED = []


def load(name, file):
    spec = importlib.util.spec_from_file_location(name, os.path.join(ROOT, 'scripts', 'app', file))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


cs = load('covers_session', 'covers-session.py')


def uss(pid):
    try:
        rolls = [cs.rollup(p) for p in [pid] + cs.children(pid)]
    except OSError:
        return 0.0
    return round(sum(r.get('Private_Clean', 0) + r.get('Private_Dirty', 0) for r in rolls) / 1024, 2)


def counts(status):
    cache = status.get('cache') or {}
    return {k: cache.get(k) for k in CACHE_KEYS}


def watch(p, t0, path):
    row, last_mem = {'peakUss': 0.0}, -MEM_EVERY
    while True:
        t, s = time.monotonic() - t0, cs.status(path) or {}
        n = s.get('texturesUploaded', 0)
        for key, hit in (('frame', s.get('framesPainted')), ('route', s.get('route') == 'Home'), ('first', n >= 1), ('all', n >= TARGET)):
            if hit:
                row.setdefault(key, round(t, 3))
        if 'all' in row and 'cacheAtAll' not in row:
            row['cacheAtAll'] = counts(s)
        if 'all' in row and 'requestsEarly' not in row and t - row['all'] >= STABLE:
            row['requestsEarly'] = s.get('coversRequested')
        if t - last_mem >= MEM_EVERY and ('all' not in row or t - row['all'] <= SETTLE):
            last_mem = t
            row['peakUss'] = max(row['peakUss'], uss(p.pid))
        if 'all' in row and t - row['all'] >= SETTLE:
            return {**row, 'cacheAtEnd': counts(s), 'requests': s.get('coversRequested'), 'textures': n}, None
        if t > CAP or p.poll() is not None:
            return row, 'exited' if p.poll() is not None else 'cap'


def pace():
    if SPAWNED:
        time.sleep(max(0, SPAWNED[-1] + SPACING - time.monotonic()))
    if len(SPAWNED) >= BUDGET:
        raise RuntimeError('refusing: launch budget spent')
    SPAWNED.append(time.monotonic())


def spawn(a, tag, binary, env):
    with cs.session(a.out, 's39') as (sock, sink, _):
        full = dict(os.environ, WAYLAND_DISPLAY=sock, PULSE_SINK=sink, TMPDIR=os.path.join(ROOT, 'target', 'tmp'), **env)
        full.pop('DISPLAY', None)
        cs.assert_isolated(full)
        path = os.path.join(a.out, tag + '.status.json')
        pace()
        with open(a.ledger, 'a') as f:
            f.write(json.dumps({'launch': tag, 'spawn': time.strftime('%Y-%m-%dT%H:%M:%S%z'), 'load1': os.getloadavg()[0]}) + '\n')
        t0 = time.monotonic()
        p = subprocess.Popen([os.path.abspath(binary), '--status', path, '--route', 'home', '--size', '1100x720'], env=full, stdout=open(os.path.join(a.out, tag + '.log'), 'w'), stderr=subprocess.STDOUT, start_new_session=True)
        guard = cs.Guard(p.pid, sink)
        guard.start()
        try:
            row, why = watch(p, t0, path)
        finally:
            guard.stop = True
            p.terminate()
            try:
                p.wait(15)
            except subprocess.TimeoutExpired:
                p.kill()
        if os.path.exists(path):
            os.remove(path)
    return {'launch': tag, 'why': why or (guard.killed and guard.killed['why']), 'load1': round(os.getloadavg()[0], 2), **row}


def launch(a, tag, binary, env):
    for _ in range(180):
        try:
            return spawn(a, tag, binary, env)
        except RuntimeError as e:
            if 'holds' not in str(e):
                raise
            time.sleep(20)
    raise RuntimeError('the GPU session lock stayed taken')


def valid(row, arm):
    at, end = row.get('cacheAtAll'), row.get('cacheAtEnd')
    if row['why'] or not all(k in row for k in ('frame', 'route', 'first', 'all')) or not at or not end:
        return False
    hits = (lambda c: c['artHits'] >= TARGET) if arm == 'B' else (lambda c: c['artHits'] == TARGET)
    home = (row['cacheAtEnd']['homeHits'] or 0) == (1 if arm == 'B' else 0)
    return all(hits(c) and c['artMisses'] == 0 for c in (at, end)) and home and row['requests'] == row.get('requestsEarly')


def sha(path):
    return hashlib.sha256(open(path, 'rb').read()).hexdigest()


def med(rows, key):
    return round(statistics.median([r[key] for r in rows]), 3) if rows else None


def spread(rows, key):
    return round(max(r[key] for r in rows) - min(r[key] for r in rows), 3) if rows else None


def judge(rows, n):
    arms = {arm: [r for r in rows if r['arm'] == arm and r['ok']] for arm in ORDER}
    summary = {arm: {'n': len(rs), **{f'{k}Median': med(rs, k) for k in ('first', 'all', 'peakUss', 'requests')}, **{f'{k}Spread': spread(rs, k) for k in ('first', 'all', 'peakUss')}} for arm, rs in arms.items()}
    if min(len(rs) for rs in arms.values()) < n:
        return {**summary, 'verdict': 'inconclusive'}
    a, b, c = (summary[arm] for arm in ORDER)
    bars = {
        'B all <= 1.5 s': b['allMedian'] <= 1.5,
        'C all <= 75% of A': c['allMedian'] <= 0.75 * a['allMedian'],
        'first <= 120% of A': max(b['firstMedian'], c['firstMedian']) <= 1.2 * a['firstMedian'],
        'requests <= A': max(b['requestsMedian'], c['requestsMedian']) <= a['requestsMedian'],
        'USS <= A + 10': max(b['peakUssMedian'], c['peakUssMedian']) <= a['peakUssMedian'] + 10,
    }
    return {**summary, 'ratioCAll': round(c['allMedian'] / a['allMedian'], 3), 'bars': bars, 'verdict': 'ship' if all(bars.values()) else 'do not ship'}


def grid_files(config):
    root = os.path.join(config, 'cache', 'bramley', 'api')
    return {name: open(os.path.join(root, name), 'rb').read() for name in os.listdir(root) if not name.endswith('.partial')}


def set_grid(config, arm, saved):
    root = os.path.join(config, 'cache', 'bramley', 'api')
    for name in saved:
        target = os.path.join(root, name)
        if arm == 'B':
            fd = os.open(target, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
            with os.fdopen(fd, 'wb') as f:
                f.write(saved[name])
        elif os.path.exists(target):
            os.remove(target)


def run(a):
    os.makedirs(a.out, exist_ok=True)
    binaries = {'A': a.a, 'B': a.b, 'C': a.b}
    with cs.isolated('s39', True) as (env, config):
        build = {arm: sha(path) for arm, path in binaries.items()}
        with open(a.ledger, 'a') as f:
            f.write(json.dumps({'build': build}) + '\n')
        fill = launch(a, 'F', a.b, env)
        print(json.dumps(fill), flush=True)
        saved = grid_files(config)
        if 'all' not in fill or not saved:
            raise RuntimeError('the fill launch did not reach 50 textures or wrote no grid; nothing measured')
        rows = []
        while len(SPAWNED) < BUDGET:
            ok = {arm: sum(r['ok'] for r in rows if r['arm'] == arm) for arm in ORDER}
            if min(ok.values()) >= a.n:
                break
            arm = min(ORDER, key=lambda x: (ok[x], ORDER.index(x)))
            set_grid(config, arm, saved)
            row = launch(a, f'{arm}{len(rows) + 1}', binaries[arm], env)
            row = {'arm': arm, **row, 'ok': valid(row, arm)}
            if arm == 'C':
                saved = grid_files(config) or saved
            rows.append(row)
            print(json.dumps(row), flush=True)
    summary = judge(rows, a.n)
    json.dump({'build': build, 'fill': fill, 'rows': rows, 'summary': summary}, open(os.path.join(a.out, 'rows.json'), 'w'), indent=1)
    print(json.dumps(summary), flush=True)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--a', required=True)
    ap.add_argument('--b', required=True)
    ap.add_argument('--out', required=True)
    ap.add_argument('--ledger', required=True)
    ap.add_argument('--n', type=int, default=5)
    run(ap.parse_args())


if __name__ == '__main__':
    main()
