import contextlib, fcntl, glob, json, os, re, secrets, shutil, signal, subprocess, threading, time

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))
SONG = '6781024437'
REAL_CONFIG = os.path.expanduser('~/.config/bramley')
REAL_CACHE = os.path.expanduser('~/.cache/bramley')
PW_STALL_S = 5
SECRETS = ['gdbus', 'call', '--session', '--dest', 'org.freedesktop.secrets']


def keyring_items(service):
    r = subprocess.run(SECRETS + ['--object-path', '/org/freedesktop/secrets', '--method', 'org.freedesktop.Secret.Service.SearchItems', "{'service': '%s'}" % service], capture_output=True, text=True, timeout=10)
    return sorted(re.findall(r"objectpath '([^']+)'", r.stdout)) if r.returncode == 0 else None


def assert_isolated(env):
    config, service = env.get('BRAMLEY_CONFIG_DIR'), env.get('BRAMLEY_KEYRING_SERVICE', 'bramley')
    cache = env.get('XDG_CACHE_HOME')
    real_cache = not cache or os.path.realpath(os.path.join(cache, 'bramley')) == os.path.realpath(REAL_CACHE)
    if not config or os.path.realpath(config) == os.path.realpath(REAL_CONFIG) or service in ('', 'bramley') or real_cache:
        raise RuntimeError(f'refusing to launch against the real config, cache or keyring: {config!r}, {cache!r}, {service!r}')
    state = env.get('XDG_STATE_HOME')
    if not state or not os.path.isabs(state) or os.path.commonpath([os.path.realpath(state), os.path.realpath(config)]) != os.path.realpath(config):
        raise RuntimeError(f'refusing to launch where the host Omarchy theme could leak in: {state!r}')


@contextlib.contextmanager
def isolated(tag, signed_in):
    config = os.path.join(ROOT, 'target', 'tmp', f'config-{tag}-{os.getpid()}')
    env = {'BRAMLEY_CONFIG_DIR': config, 'BRAMLEY_KEYRING_SERVICE': f'bramley-gate-{tag}', 'XDG_CACHE_HOME': os.path.join(config, 'cache'), 'XDG_STATE_HOME': os.path.join(config, 'state')}
    try:
        os.makedirs(config, mode=0o700)
        for f in ('developer-token', 'music-user-token') if signed_in else ('developer-token',):
            shutil.copy(os.path.join(REAL_CONFIG, f), config)
        assert_isolated(env)
        yield env, config
    finally:
        for item in keyring_items(env['BRAMLEY_KEYRING_SERVICE']) or []:
            subprocess.run(SECRETS + ['--object-path', item, '--method', 'org.freedesktop.Secret.Item.Delete'], capture_output=True, timeout=10)
        shutil.rmtree(config, ignore_errors=True)


def pw_dump():
    return json.loads(subprocess.run(['pw-dump'], capture_output=True, text=True, check=True, timeout=1).stdout)


def props(dump):
    return {o['id']: (o.get('info') or {}).get('props') or {} for o in dump if o.get('type') == 'PipeWire:Interface:Node'}


def sink_exists(name):
    return any(p.get('node.name') == name and p.get('media.class') == 'Audio/Sink' for p in props(pw_dump()).values())


def routing(pid, dump):
    nodes = props(dump)
    ours = [i for i, p in nodes.items() if str(p.get('application.process.id')) == str(pid) and p.get('media.class', '').startswith('Stream/Output')]
    targets = set()
    for o in dump:
        info = o.get('info') or {}
        if o.get('type') == 'PipeWire:Interface:Link' and info.get('output-node-id') in ours:
            targets.add(nodes.get(info.get('input-node-id'), {}).get('node.name'))
    return ours, targets


def default_sink():
    out = subprocess.run(['wpctl', 'inspect', '@DEFAULT_AUDIO_SINK@'], capture_output=True, text=True).stdout
    return next((l.split('=', 1)[1].strip().strip('"') for l in out.splitlines() if 'node.name' in l), None)


def restore_default(name):
    ids = [i for i, p in props(pw_dump()).items() if p.get('node.name') == name]
    return bool(ids) and subprocess.run(['wpctl', 'set-default', str(ids[0])]).returncode == 0 and default_sink() == name


class Guard(threading.Thread):
    def __init__(self, pid, allowed):
        super().__init__(daemon=True)
        self.pid, self.allowed = pid, allowed
        self.stop, self.killed, self.last_clean, self.seen, self.polls, self.log = False, None, None, set(), 0, []
        self.stalls = []

    def run(self):
        answered = time.monotonic()
        while not self.stop and os.path.exists(f'/proc/{self.pid}'):
            try:
                ours, targets = routing(self.pid, pw_dump())
            except Exception as e:
                self.stalls.append([round(time.time(), 2), str(e)])
                if time.monotonic() - answered >= PW_STALL_S:
                    self.kill(f'pw-dump failed for {PW_STALL_S} s: {e}')
                    return
                time.sleep(0.25)
                continue
            answered = time.monotonic()
            self.polls += 1
            self.seen |= targets
            self.log.append([round(time.time(), 2), sorted(str(t) for t in targets)])
            bad = sorted(str(t) for t in targets if t != self.allowed)
            if bad:
                self.kill(f'stream linked to {bad}')
                return
            self.last_clean = time.time()
            time.sleep(0.25)

    def kill(self, why):
        try:
            os.kill(self.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        self.killed = {'why': why, 't': time.time()}


def rollup(pid):
    r = {}
    with open(f'/proc/{pid}/smaps_rollup') as f:
        for line in f:
            parts = line.split()
            if len(parts) >= 2 and parts[0].endswith(':') and parts[1].isdigit():
                r[parts[0][:-1]] = int(parts[1])
    return r


def children(pid):
    found = []
    for d in glob.glob('/proc/[0-9]*/stat'):
        try:
            s = open(d).read()
            if int(s[s.rindex(')') + 2:].split()[1]) == pid:
                found.append(int(d.split('/')[2]))
        except (OSError, ValueError):
            pass
    return found


def gpu():
    out = {}
    for key in ('mem_info_vram_used', 'mem_info_gtt_used'):
        vals = [int(open(p).read()) for p in glob.glob(f'/sys/class/drm/card*/device/{key}')]
        out[key] = sum(vals) if vals else None
    return out


def status(path):
    try:
        return json.load(open(path))
    except (OSError, ValueError):
        return None


def wait_for(pred, timeout, what):
    end = time.time() + timeout
    while time.time() < end:
        v = pred()
        if v:
            return v
        time.sleep(0.2)
    raise RuntimeError(f'timed out waiting for {what}')


GPU_LOCK, LOCK_HELD = f'/run/user/{os.getuid()}/bramley-gpu-session.lock', 'BRAMLEY_GPU_LOCK_HELD'


@contextlib.contextmanager
def gpu_lock():
    if os.environ.get(LOCK_HELD) == '1':
        yield
        return
    fd = os.open(GPU_LOCK, os.O_CREAT | os.O_RDWR, 0o600)
    try:
        fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        os.close(fd)
        raise RuntimeError(f'another weston session holds {GPU_LOCK}; refusing') from None
    try:
        yield
    finally:
        os.close(fd)


def unique_sink(prefix, out):
    run = re.sub(r'[^A-Za-z0-9_-]', '-', os.path.basename(os.path.abspath(out)))
    return f'{prefix}-null-{run}-{os.getpid()}-{secrets.token_hex(3)}'


def null_sink(name):
    node = f'create-node adapter {{ factory.name=support.null-audio-sink node.name={name} node.description={name} media.class=Audio/Sink audio.position=[FL FR] priority.session=0 priority.driver=0 object.linger=false }}'
    pw = subprocess.Popen(['bash', '-c', f"{{ echo '{node}'; exec sleep 100000; }} | pw-cli >/dev/null 2>&1"], start_new_session=True)
    try:
        wait_for(lambda: sink_exists(name), 10, f'null sink {name}')
    except RuntimeError:
        os.killpg(pw.pid, signal.SIGTERM)
        raise
    return pw


@contextlib.contextmanager
def session(out, prefix):
    with gpu_lock():
        with _session(out, prefix) as found:
            yield found


@contextlib.contextmanager
def _session(out, prefix):
    os.environ['XDG_RUNTIME_DIR'] = f'/run/user/{os.getuid()}'
    sock, sink = f'{prefix}-{os.getpid()}', unique_sink(prefix, out)
    weston = subprocess.Popen(['weston', '--backend=headless', '--renderer=gl', f'--socket={sock}', '--width=1280', '--height=800', '--idle-time=0', f'--log={out}/weston.log'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
    pw = None
    summary = {'sink': sink}
    try:
        pw = null_sink(sink)
        wait_for(lambda: os.path.exists(f"{os.environ['XDG_RUNTIME_DIR']}/{sock}"), 10, 'weston')
        summary['defaultAtStart'] = default_sink()
        if summary['defaultAtStart'] in (None, sink):
            raise RuntimeError(f"default sink is {summary['defaultAtStart']}; refusing")
        yield sock, sink, summary
    finally:
        if pw:
            os.killpg(pw.pid, signal.SIGTERM)
        os.killpg(weston.pid, signal.SIGTERM)
        time.sleep(1.5)
        summary['sinkGone'] = not sink_exists(sink)
        summary['defaultAtEnd'] = default_sink()
        if summary.get('defaultAtStart') and summary['defaultAtEnd'] != summary['defaultAtStart']:
            summary['defaultRestored'] = restore_default(summary['defaultAtStart'])
        print(json.dumps(summary), flush=True)
        json.dump(summary, open(os.path.join(out, 'session.verdict.json'), 'w'))

