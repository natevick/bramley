import os, re, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
SCRATCH = os.path.join(HERE, '..', '..', 'target', 'tmp', 'mutants')
NOOP = 'no-op control'
MUTANTS = [
    (NOOP, "R1_BAR, R2_BAR, R3_BAR", "R1_BAR,  R2_BAR, R3_BAR"),
    ('llvmpipe alongside radeonsi', "'radeonsi' not in str(g) or 'llvmpipe' in str(g).lower()", "'radeonsi' not in str(g)"),
    ('missing weston renderer line', "if bad or not renderers else []", "if bad else []"),
    ('a stream with an extra target', "off = sum(1 for targets in window if targets != [sink])", "off = sum(1 for targets in window if sink not in targets)"),
    ('unbounded guard window', "if rows[0]['t'] <= t <= rows[-1]['t']]", "]"),
    ('ballast reason order', "[r[:10] for r in b['reasons']] == ['USS median']", "[r[:10] for r in b['reasons']][-1:] == ['USS median']"),
    ('R1 VOID with R2 FAIL', "('VOID' if 'VOID' in seen else 'FAIL' if 'FAIL' in seen", "('FAIL' if 'FAIL' in seen else 'VOID' if 'VOID' in seen"),
    ('drift bar inclusive', "[d for d in drifts if d > DRIFT_BAR]", "[d for d in drifts if d >= DRIFT_BAR]"),
    ('drift only upward', "'PASS': [d for d in drifts if d < -DRIFT_BAR]", "'PASS': []"),
    ('zone 2 dropped', "2 if m['rss'] <= bar else 3", "3"),
    ('R2 engine rule dropped', "if any(r.get('engineStarted') for r in rows):", "if False:"),
    ('F1 bar', "> F1_MS:", "> F1_MS * 2:"),
    ('F1 sidebar', "not s.get('playlists') or ", ""),
    ('F2 refusal exit', "ev.get('playExit') != 5 or ", ""),
    ('F2 streams', " or ev.get('streams')", ""),
    ('F3 view', "any(r.get('uiMs') is None for r in reps)", "False"),
    ('F3 median key', "for key in ('wallMs', 'uiMs'):", "for key in ('wallMs',):"),
    ('F4 preview', "< FULL_SONG_S:", "< 0:"),
    ('F4 sent', " or (final.get('sentMs') or 0) <= F4_PLAYED_S * 1000", ""),
    ('F4 median', "elif median(reps) > F4_MS:", "elif min(reps) > F4_MS:"),
    ('no-stream VOID', "or [sink] not in window", ""),
    ('no-stream VOID only once started', "if started and (", "if ("),
    ('F5 every rep', "elif cmd in EVERY_REP and rounds and max(ms) > F5_MS:", "elif False:"),
    ('F5 matched', "if any(not s.get('matched') for s in seen):", "if False:"),
    ('F6 count', "len(moves) < 3 or ", ""),
    ('F7 size', "< 100:", "< 1:"),
    ('F8 order', "ev.get('seen') != ev.get('expected')", "sorted(ev.get('seen') or []) != sorted(ev.get('expected') or [])"),
    ('F9 tools', "if ev.get('tools') != TOOLS:", "if not ev.get('tools'):"),
    ('F10 recovery', "net.get('coversRecoveredS') is None or ", ""),
    ('F10 offline view', "and net.get('downUi') == 'error' ", ""),
    ('R3 USS zone', "if g['uss'] > R3_BAR:", "if False:"),
    ('R3 window', "growth(rows, 5, 30)", "growth(rows, 0, 30)"),
    ('R3 tracks', "if played < R3_TRACKS:", "if played < 1:"),
    ('R3 stall seeks', "and a.get('seeks') == b.get('seeks') ", ""),
    ('R3 drift', "voids += drift_voids(extra.get('zone'), outcome, drifts)", "pass"),
    ('R4 state', "if ev.get('before') != state or ev.get('after') != state:", "if False:"),
    ('R5 bar', "(R5_BAR, 'Paused')", "(R4_BAR, 'Paused')"),
    ('ballast kept covers', "ev['ballastMiB'] + kept", "ev['ballastMiB']"),
    ('missing evidence beats VOID', "if missing:\n        return {'verdict': 'FAIL', 'reasons': missing + fails + voids}\n    ev = files", "if False:\n        pass\n    ev = files"),
    ('controls accept vacuous FAILs', "return c['verdict'] == 'FAIL' and not any(r.startswith('missing evidence') for r in c['reasons'])", "return c['verdict'] == 'FAIL'"),
    ('leak changes', "and (leak.get('changesAfter5') or 0) >= 4", ""),
    ('realstate', "real.get('before') == real.get('after')", "True"),
    ('leak grep hits', "leaks.get('hits') == {'repo': 0, 'run': 0} and ", ""),
    ('leak grep self-check', " and leaks.get('plantedCaught') is True", ""),
    ('cut-tail skipped', "and t.get('skippedS', 0) > 0.1 ", ""),
    ('cut-tail drained', "and not t.get('drained') ", ""),
]


def main():
    source = open(os.path.join(HERE, 'gate.py')).read()
    os.makedirs(SCRATCH, exist_ok=True)
    killed, survived, rows = 0, [], []
    for i, (name, old, new) in enumerate(MUTANTS):
        if source.count(old) != 1:
            sys.exit(f'mutant {name!r} does not apply exactly once')
        path = os.path.join(SCRATCH, f'gate_{i}.py')
        open(path, 'w').write(source.replace(old, new))
        r = subprocess.run([sys.executable, '-I', os.path.join(HERE, 'test_gate.py')], env=dict(os.environ, GATE_UNDER_TEST=path), capture_output=True, text=True)
        ran = re.search(r'Ran (\d+) tests', r.stderr)
        if not ran or int(ran[1]) == 0:
            sys.exit(f'mutant {name!r}: the tests did not run\n{r.stderr[-800:]}')
        dead = r.returncode != 0
        rows.append(f'{"KILLED  " if dead else "SURVIVED"} {name} ({ran[1]} tests)')
        if name != NOOP:
            killed += dead
            survived += [] if dead else [name]
        elif dead:
            sys.exit('the no-op mutant was killed: the suite fails without a real change')
        os.remove(path)
    print('\n'.join(rows))
    print(f'{killed} of {len(MUTANTS) - 1} killed; no-op survived')
    sys.exit(1 if survived else 0)


if __name__ == '__main__':
    main()
