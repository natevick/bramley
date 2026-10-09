import argparse, json, os, signal, subprocess, sys, time, statistics

def kids_map():
    m = {}
    for d in os.listdir('/proc'):
        if not d.isdigit():
            continue
        try:
            with open(f'/proc/{d}/stat') as f:
                s = f.read()
            ppid = int(s[s.rindex(')') + 2:].split()[1])
            m.setdefault(ppid, []).append(int(d))
        except OSError:
            pass
    return m

def tree(root):
    m = kids_map()
    out, stack = [], [root]
    while stack:
        p = stack.pop()
        if not os.path.exists(f'/proc/{p}'):
            continue
        out.append(p)
        stack.extend(m.get(p, []))
    return out

def rollup(pid):
    r = {}
    try:
        with open(f'/proc/{pid}/smaps_rollup') as f:
            for line in f:
                parts = line.split()
                if len(parts) >= 2 and parts[0].endswith(':') and parts[1].isdigit():
                    r[parts[0][:-1]] = int(parts[1])
    except OSError:
        return None
    return r

def comm(pid):
    try:
        with open(f'/proc/{pid}/comm') as f:
            return f.read().strip()
    except OSError:
        return '?'

def sample(root):
    pss = rss = uss = 0
    per = []
    for p in tree(root):
        r = rollup(p)
        if r is None:
            continue
        u = r.get('Private_Clean', 0) + r.get('Private_Dirty', 0)
        pss += r.get('Pss', 0); rss += r.get('Rss', 0); uss += u
        per.append({'pid': p, 'comm': comm(p), 'pssKiB': r.get('Pss', 0), 'rssKiB': r.get('Rss', 0), 'ussKiB': u})
    return {'pssKiB': pss, 'rssKiB': rss, 'ussKiB': uss, 'count': len(per), 'per': per}

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--name', required=True)
    ap.add_argument('--warmup', type=float, default=5)
    ap.add_argument('--sample', type=float, default=60)
    ap.add_argument('--interval', type=float, default=2)
    ap.add_argument('--log', required=True)
    ap.add_argument('--out', required=True)
    ap.add_argument('cmd', nargs=argparse.REMAINDER)
    a = ap.parse_args()
    cmd = a.cmd[1:] if a.cmd and a.cmd[0] == '--' else a.cmd
    logf = open(a.log, 'w')
    t0 = time.time()
    proc = subprocess.Popen(cmd, stdout=logf, stderr=subprocess.STDOUT, start_new_session=True)
    samples = []
    try:
        time.sleep(a.warmup)
        end = time.time() + a.sample
        while time.time() < end:
            if proc.poll() is not None:
                break
            s = sample(proc.pid)
            s['t'] = round(time.time() - t0, 2)
            samples.append(s)
            time.sleep(a.interval)
    finally:
        exited = proc.poll()
        if exited is None:
            os.killpg(proc.pid, signal.SIGTERM)
            try:
                proc.wait(10)
            except subprocess.TimeoutExpired:
                os.killpg(proc.pid, signal.SIGKILL)
                proc.wait()
        logf.close()
    mib = lambda k: round(k / 1024, 1)
    med = lambda key: mib(statistics.median([s[key] for s in samples])) if samples else None
    mx = lambda key: mib(max(s[key] for s in samples)) if samples else None
    res = {
        'name': a.name, 'cmd': cmd, 'exitedEarly': exited, 'samples': len(samples),
        'pssMedMiB': med('pssKiB'), 'pssMaxMiB': mx('pssKiB'),
        'rssMedMiB': med('rssKiB'), 'rssMaxMiB': mx('rssKiB'),
        'ussMedMiB': med('ussKiB'), 'ussMaxMiB': mx('ussKiB'),
        'procCountMin': min((s['count'] for s in samples), default=0),
        'lastPer': samples[-1]['per'] if samples else [],
        'samplesRaw': [{k: v for k, v in s.items() if k != 'per'} for s in samples],
    }
    with open(a.out, 'w') as f:
        json.dump(res, f, indent=1)
    print(json.dumps({k: v for k, v in res.items() if k not in ('samplesRaw', 'lastPer')}))

if __name__ == '__main__':
    main()
