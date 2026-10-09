import json, sys
m = json.load(open(sys.argv[1]))
st = json.load(open(sys.argv[2]))
try:
    rt = json.load(open(sys.argv[3]))
except Exception as e:
    rt = {'error': str(e), 'streams': [], 'targets': []}
before, after, sink = sys.argv[4], sys.argv[5], sys.argv[6]
app = st.get('app') or {}
plat = st.get('platform') or {}
eme = plat.get('eme') or {}
medias = st.get('medias') or {}
med = max(medias.values(), key=lambda x: x.get('decodedDecryptedSeconds', 0)) if medias else {}
checks = {
    'c1_jsSawUsableKey': bool(eme.get('usableSeen')) and app.get('keyLoaded', 0) >= 1,
    'c2_jsCurrentTimeOver30s': app.get('maxCurrentTime', 0) > 30,
    'c2_nativeDecryptedOver30s': med.get('decodedDecryptedSeconds', 0) > 30 and med.get('decryptOk', 0) > 0,
    'c3_pipewireStream': len(rt.get('streams', [])) > 0 and sink in rt.get('targets', []),
    'c5_treeWalked': m.get('samples', 0) > 0 and m.get('procCountMin', 0) >= 1 and m.get('exitedEarly') is None,
    'defaultSinkUnchanged': before == after,
}
ok = all(checks.values())
pss = m.get('pssMedMiB')
verdict = 'VOID' if not ok else ('PASS' if pss <= 105 else 'MARGINAL' if pss <= 125 else 'FAIL')
print(json.dumps({
    'name': m['name'], 'verdict': verdict, 'pssMedMiB': pss, 'pssMaxMiB': m.get('pssMaxMiB'), 'rssMedMiB': m.get('rssMedMiB'), 'ussMedMiB': m.get('ussMedMiB'), 'procCountMin': m.get('procCountMin'),
    'checks': checks,
    'detail': {
        'appPhase': app.get('phase'), 'hlsVersion': app.get('hlsVersion'), 'appErrors': app.get('errors'), 'maxCurrentTime': app.get('maxCurrentTime'), 'keyLoaded': app.get('keyLoaded'),
        'firstAudioMs': app.get('firstAudioMs'), 'firstKeyLoadedMs': app.get('firstKeyLoadedMs'), 'mediaEvents': app.get('mediaEvents'),
        'eme': eme, 'uncaught': plat.get('uncaught'), 'xhr': plat.get('xhr'),
        'native': {k: med.get(k) for k in ('clearSamples', 'encryptedSamples', 'decryptOk', 'decryptFail', 'decodedClearSeconds', 'decodedDecryptedSeconds', 'decodeErr', 'keyWaits', 'underruns', 'seeks', 'loops', 'rate', 'channels')},
        'cdm': st.get('cdm'), 'emeGranted': [r.get('granted') for r in st.get('emeRequests', [])],
        'scripts': st.get('scripts'), 'opCounts': st.get('opCounts'), 'sourceBuffers': st.get('sourceBuffers'),
    },
    'routing': rt,
}, indent=1))
