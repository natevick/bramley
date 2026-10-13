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
    ('F4 median', "elif median(reps) > F4_MS:", "elif min(reps) > F4_MS:"),
    ('no-stream VOID', "or [sink] not in window", ""),
    ('F5 every rep', "elif cmd in EVERY_REP and rounds and max(ms) > F5_MS:", "elif False:"),
    ('F6 count', "len(moves) < 3 or ", ""),
    ('F7 size', "< 100:", "< 1:"),
    ('F8 order', "ev.get('seen') != ev.get('expected')", "sorted(ev.get('seen') or []) != sorted(ev.get('expected') or [])"),
    ('F9 tools', "if ev.get('tools') != TOOLS:", "if not ev.get('tools'):"),
    ('F10 recovery', "net.get('coversRecoveredS') is None or ", ""),
    ('F10 offline view', "and net.get('downUi') == 'error' ", ""),
    ('F7 view total', "if ev.get('viewTotal') != ev.get('largestTracks'):", "if False:"),
    ('F10 fresh banner', " and bad.get('banner') != bad.get('bannerBefore')", ""),
    ('F10 probes', "!= [False, True, False, True]:", "!= [False, True, False, True] and False:"),
    ('F10 drop recovery', " or after.get('exit') != 0", ""),
    ('F10 alive at end', "if not net.get('aliveAtEnd'):", "if False:"),
    ('F10 gap VOID dropped', "if gap is None or gap >= DROP_GAP_S else []", "if False else []"),
    ('F10 gap bar exclusive', "gap >= DROP_GAP_S else", "gap > DROP_GAP_S else"),
    ('F10 hole bar', "net['holeRecoveredS'] > HOLE_BAR_S", "net['holeRecoveredS'] > HOLE_BAR_S * 2"),
    ('F10 hole unrecovered passes', "net.get('holeRecoveredS') is None or ", ""),
    ('F10 voids ignored', "    voids += extra.pop('voids', [])\n", "    extra.pop('voids', [])\n"),
    ('R3 USS zone', "if g['uss'] > R3_BAR:", "if False:"),
    ('R3 window', "growth(rows, 5, 30)", "growth(rows, 0, 30)"),
    ('R3 tracks', "if played < R3_TRACKS:", "if played < 1:"),
    ('R3 drift', "voids += drift_voids(extra.get('zone'), outcome, drifts)", "pass"),
    ('R4 state', "if ev.get('before') != state or ev.get('after') != state:", "if False:"),
    ('R5 bar', "(R5_BAR, 'Paused')", "(R4_BAR, 'Paused')"),
    ('ballast kept covers', "ev['ballastMiB'] + kept", "ev['ballastMiB']"),
    ('missing evidence beats VOID', "if missing:\n        return {'verdict': 'FAIL', 'reasons': missing + fails + voids}\n    ev = files", "if False:\n        pass\n    ev = files"),
    ('leak changes', "and (leak.get('changesAfter5') or 0) >= 4", ""),
    ('realstate', "real.get('before') == real.get('after')", "True"),
    ('co-tenant VOID', "    elif cotenants.get('seen'):", "    elif False:"),
    ('co-tenant watch polled', " or not cotenants.get('polls')", ""),
    ('leak grep hits', "leaks.get('hits') == {'repo': 0, 'run': 0} and ", ""),
    ('leak grep self-check', " and leaks.get('plantedCaught') is True", ""),
    ('no-stream VOID only once started', "if started and not missing and (", "if not missing and ("),
    ('controls ignore their stated reason', "return c['verdict'] == 'FAIL' and bool(reasons) and all(any(want in r for r in c['reasons']) for want in reasons)", "return c['verdict'] == 'FAIL'"),
    ('cut-tail skipped', "and m['skippedS'] > F6_SKIPPED_S and abs", "and abs"),
    ('F5 effect required', "or s.get('effectMs') is None for s in seen", "for s in seen"),
    ('F5 applied required', " or s.get('appliedMs') is None", ""),
    ('F5 advancing bar', "> F5_ADVANCING_MS:", "> F5_ADVANCING_MS * 2:"),
    ('F5 advancing required', "if None in heard:", "if False:"),
    ('F4 decrypted bar', "if delta(base, end, 'decryptedMs') < DECRYPTED_MS:", "if delta(base, end, 'decryptedMs') < DECRYPTED_MS - 1000:"),
    ('F4 decrypted as a total, not a delta', "if delta(base, end, 'decryptedMs') < DECRYPTED_MS:", "if (end.get('decryptedMs') or 0) < DECRYPTED_MS:"),
    ('F4 skipped bar', "if delta(heard, end, 'skippedMs') > SKIPPED_MS:", "if delta(heard, end, 'skippedMs') > SKIPPED_MS * 10:"),
    ('F4 skipped from the command, not from audible', "if delta(heard, end, 'skippedMs') > SKIPPED_MS:", "if delta(base, end, 'skippedMs') > SKIPPED_MS:"),
    ('F4 decrypt failures', "if delta(base, end, 'decryptFails') != 0:", "if False:"),
    ('F4 pace tolerance', "if abs(delta(heard, end, 'position') - delta(heard, end, 't')) > PACE_S:", "if abs(delta(heard, end, 'position') - delta(heard, end, 't')) > PACE_S * 3:"),
    ('F4 pace one-sided', "if abs(delta(heard, end, 'position') - delta(heard, end, 't')) > PACE_S:", "if delta(heard, end, 't') - delta(heard, end, 'position') > PACE_S:"),
    ('F4 banner at the end', " or j.get('banner') is not None", ""),
    ('F4 state at the end', " or j.get('state') != 'Playing'", ""),
    ('F4 judged plays counted', "if len(judged) < plays:", "if not judged:"),
    ('F4 per-rep exit', "if check == 'F4' and any(r.get('exit') != 0", "if check == 'F4' and all(r.get('exit') != 0"),
    ('F9 per-rep error', "any(r.get('error') is not False for r in", "all(r.get('error') is not False for r in"),
    ('F6 sent read at the flip', "'sentS': delta(base, flip, 'sentMs') / 1000", "'sentS': delta(base, end, 'sentMs') / 1000"),
    ('F6 sent slack', "- F6_SENT_SLACK_S and", "- F6_SENT_SLACK_S * 2 and"),
    ('F6 skipped bar', "m['skippedS'] <= F6_SKIPPED_S and", "m['skippedS'] <= F6_SKIPPED_S * 10 and"),
    ('F6 skipped read at the window end', "'skippedS': delta(base, end, 'skippedMs')", "'skippedS': delta(base, flip, 'skippedMs')"),
    ('F6 stall bar', "m['stallS'] <= F6_STALL_S", "m['stallS'] <= F6_STALL_S * 3"),
    ('F6 landing bar', "abs(m['landedOff']) <= F6_LAND_S and (m", "abs(m['landedOff']) <= F6_LAND_S * 2 and (m"),
    ('F6 drain timeouts', "'drained': delta(base, end, 'drainsAcked') >= 1 and delta(base, end, 'drainTimeouts') == 0", "'drained': delta(base, end, 'drainsAcked') >= 1"),
    ('F6 drain read at the flip', "'drained': delta(base, end, 'drainsAcked') >= 1", "'drained': delta(base, flip, 'drainsAcked') >= 1"),
    ('F6 window length', "if end['t'] < flip['t'] + F6_WINDOW_S:", "if False:"),
    ('R3 output underflows', "    elif underflows:\n", "    elif False:\n"),
    ('R3 output underflows unreadable', "    if underflows is None:\n", "    if False:\n"),
    ('R3 output underflows read from the last sample only', "if rows and rows[0].get(key) is not None and", "if rows and"),
    ('R3 pace tolerance', "if lag(a, b) > PACE_S]", "if lag(a, b) > PACE_S * 3]"),
    ('R3 seek exclusion window', "and not any(a['t'] - 1 <= t <= b['t'] + 1 for t in seeks)", ""),
    ('R3 banner', "if any(r.get('state') != 'Playing' or r.get('banner') is not None for r in rows):\n        fails.append('not Playing with no banner in every sample')\n    underflows", "if any(r.get('state') != 'Playing' for r in rows):\n        fails.append('not Playing with no banner in every sample')\n    underflows"),
    ('CPU bin size', "if ev.get('binS') != CPU_BIN_S or", "if"),
    ('CPU one process', " or any(p != 1 for p in ev.get('procs') or [None])", ""),
    ('F1 every launch', "elif max(s['ms'] for s in starts) > F1_MS:", "elif median(s['ms'] for s in starts) > F1_MS:"),
    ('stall control', "'holds': bool(caught) or (underflows or 0) >= 1}", "'holds': True}"),
    ('stall control ignores output underflows', "'holds': bool(caught) or (underflows or 0) >= 1}", "'holds': bool(caught)}"),
    ('stall control ignores pacing', "'holds': bool(caught) or (underflows or 0) >= 1}", "'holds': (underflows or 0) >= 1}"),
    ('bad token play expects sign-in-needed', "BAD_TOKEN_PLAY, NO_OUTPUT = 'developer token was rejected', 'developer token was rejected'", "BAD_TOKEN_PLAY, NO_OUTPUT = 'developer token was rejected', 'SignInNeeded'"),
    ('guard kill reason', "and 'stream linked to' in str(g.get('killed'))", ""),
    ('guard kill wrong sink present', "bool(g.get('wrongSinkPresent') and ", "bool(True and "),
    ('guard kill default unchanged', "and g.get('defaultBefore') == g.get('defaultAfter')", ""),
    ('no-stream reason', " and all(NO_OUTPUT in b for b in banners(out, 'nostream.f4.json'))", ""),
    ('bad token search reason', " and all(BAD_TOKEN_SEARCH in e for e in f3_errors)", ""),
    ('bad token play reason', " and all(BAD_TOKEN_PLAY in b for b in banners(out, 'badtoken.f4.json'))", ""),
    ('no-user banner', "\n                            and all('SignInNeeded' in b for b in banners(out, 'nouser.f4.json'))", ""),
    ('leak grep read files', " and (leaks.get('runFiles') or 0) > 0", ""),
    ('co-tenant standing by name, not pid', "allowed = pid in standing if standing is not None else comm in STANDING_GPU_CLIENTS", "allowed = comm in STANDING_GPU_CLIENTS"),
    ('co-tenant exclusion ignored', " and not exclude(pid) and ", " and "),
    ('missing counters pass', "    if gaps:\n        return [f'counters missing: {gaps}']\n", ""),
    ('F6 missing counters pass', "    if gaps:\n        return {'pass': False, 'why': f'counters missing: {gaps}'}\n", ""),
    ('leak grep had library names', " and (leaks.get('libraryNames') or 0) > 0", ""),
    ('phase errors', " and not v['phaseErrors']", ""),
    ('missing artifact reason', " and all(r.startswith('missing evidence') for r in v['reasons'][:1])", ""),
    ('missing artifact holds', "holds=all(v['verdict'] == 'FAIL' and v['reasons']", "holds=True or all(v['verdict'] == 'FAIL' and v['reasons']"),
    ('ballast landing', "landed = b.get('uss') is not None and abs(b['uss'] - (R1_BAR + BALLAST_OVER)) <= LANDING_BAND", "landed = True"),
    ('leak reason', "'holds': real_fail(leak, 'USS growth') and", "'holds': real_fail(leak) and"),
    ('guards of every session', "for session in ev.get('sessions') or [ev.get('session')]:", "for session in [ev.get('session') or (ev.get('sessions') or [None])[-1]]:"),
    ('cold F4 guard checked', "if 'session' in ev or 'sessions' in ev:", "if 'session' in ev:"),
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
