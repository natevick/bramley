import json, sys
m = json.load(open(sys.argv[1]))
st = json.load(open(sys.argv[2]))
try:
    rt = json.load(open(sys.argv[3]))
except Exception as e:
    rt = {'error': str(e), 'streams': [], 'targets': []}
before, after, sink = sys.argv[4], sys.argv[5], sys.argv[6]
app = st.get('app') or {}
sc = st.get('scenario') or {}
steps = {s['step']: s for s in sc.get('steps', [])}
events = app.get('mediaEvents') or {}
medias = st.get('medias') or {}
med = max(medias.values(), key=lambda x: x.get('decodedDecryptedSeconds', 0)) if medias else {}
sbs = st.get('sourceBuffers') or {}
buffered_end = max((r[-1][1] for r in (sb.get('buffered') for sb in sbs.values()) if r), default=None)
names = ['pause-at-15', 'after-20s-pause', 'seeked-to-40', '5s-after-resume', 'ended']
ct = lambda n: steps[n]['js']['currentTime']
out = lambda n: steps[n]['native']['outputPosition']
dec = lambda n: steps[n]['native']['decodedDecryptedSeconds'] + steps[n]['native']['decodedClearSeconds']
wall = lambda n: steps[n]['wallSec']
samples = [s['pssKiB'] / 1024 for s in m.get('samplesRaw', [])]
ran = {
    'allStepsRecorded': all(n in steps for n in names),
    'seekingSeekedEndedSeen': all(events.get(e, 0) >= 1 for e in ('seeking', 'seeked', 'ended')),
    'pipewireStream': len(rt.get('streams', [])) > 0 and sink in rt.get('targets', []),
    'defaultSinkUnchanged': before == after,
    'decryptedOver20s': med.get('decodedDecryptedSeconds', 0) > 20,
    'pssSampled': len(samples) > 0 and m.get('exitedEarly') is None,
}
measured, checks = {}, {}
if all(ran.values()):
    agree = {n: abs(ct(n) - out(n)) for n in ('pause-at-15', '5s-after-resume', 'ended')}
    measured = {
        'currentTimeVsOutput': agree,
        'pauseWallSec': wall('after-20s-pause') - wall('pause-at-15'),
        'pauseCurrentTimeDrift': abs(ct('after-20s-pause') - ct('pause-at-15')),
        'pauseOutputDrift': abs(out('after-20s-pause') - out('pause-at-15')),
        'seekLanding': ct('seeked-to-40'),
        'resumeAdvanceVsWall': abs((ct('5s-after-resume') - ct('seeked-to-40')) - (wall('5s-after-resume') - wall('seeked-to-40'))),
        'endCurrentTime': ct('ended'), 'bufferedEnd': buffered_end,
        'decodedVsAdvance': abs((dec('ended') - dec('seeked-to-40')) - (ct('ended') - ct('seeked-to-40'))),
        'pssFirstMiB': samples[0], 'pssMaxMiB': max(samples), 'pssLastMiB': samples[-1], 'pssGrowthMiB': max(samples) - samples[0],
    }
    checks = {
        '1_currentTimeMatchesOutput': all(v <= 0.5 for v in agree.values()),
        '2_pauseFroze': events.get('pause', 0) >= 1 and measured['pauseWallSec'] >= 20 and measured['pauseCurrentTimeDrift'] <= 0.1 and measured['pauseOutputDrift'] <= 0.25 and steps['after-20s-pause']['js']['paused'] is True,
        '3_seekLanded': abs(measured['seekLanding'] - 40) <= 0.5 and steps['seeked-to-40']['js']['seeking'] is False,
        '4_resumeTracksWall': measured['resumeAdvanceVsWall'] <= 0.5,
        '5_endedAtEnd': buffered_end is not None and abs(measured['endCurrentTime'] - buffered_end) <= 0.5 and measured['decodedVsAdvance'] <= 0.5,
        '6_pssFlat': measured['pssGrowthMiB'] <= 5,
    }
verdict = 'VOID' if not all(ran.values()) else ('PASS' if all(checks.values()) else 'FAIL')
print(json.dumps({'name': m['name'], 'verdict': verdict, 'didItRun': ran, 'checks': checks, 'measured': measured, 'pssMedMiB': m.get('pssMedMiB'),
                  'steps': sc.get('steps'), 'skipPause': sc.get('skipPause'), 'eventLog': app.get('eventLog'), 'appErrors': app.get('errors'), 'routing': rt}, indent=1))
