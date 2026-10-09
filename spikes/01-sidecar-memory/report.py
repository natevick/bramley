import json, sys
for path in sys.argv[1:]:
    d = json.load(open(path))
    s = d.get('summary', {})
    c = {k: v for k, v in d.get('checks', {}).items() if k != 'detail'}
    det = d.get('checks', {}).get('detail', {})
    from collections import Counter
    print(json.dumps({'name': d['name'], 'verdict': d.get('verdict'), 'firstAudioMs': d.get('firstAudioMs'), 'firstUsableMs': d.get('firstUsableMs'),
        'pssMed': s.get('pssMedMiB'), 'pssMax': s.get('pssMaxMiB'), 'rssMed': s.get('rssMedMiB'), 'ussMed': s.get('ussMedMiB'), 'procs': s.get('procCount'),
        'paused': s.get('paused'), 'checks': c, 'ct': [det.get('ctStart'), det.get('ctEnd')], 'keys': det.get('keyStatuses'), 'ks': det.get('playerKeySystem'),
        'errors': det.get('errors'), 'mk': det.get('musickitLoaded'), 'types': dict(Counter(s.get('types', [])))}))
