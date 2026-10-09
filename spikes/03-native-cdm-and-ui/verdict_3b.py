import json, sys
m = json.load(open(sys.argv[1]))
st = json.load(open(sys.argv[2]))
try:
    rt = json.load(open(sys.argv[3]))
except Exception as e:
    rt = {'error': str(e), 'streams': [], 'targets': []}
before, after = sys.argv[4], sys.argv[5]
checks = {
    'c1_keyUsable': bool(st.get('usable')),
    'c2_decryptedOver30s': st.get('decodedDecryptedSeconds', 0) > 30 and st.get('decryptOk', 0) > 0,
    'c3_pipewireStream': len(rt.get('streams', [])) > 0 and 'spike3-null' in rt.get('targets', []),
    'c5_treeWalked': m.get('samples', 0) > 0 and m.get('procCountMin', 0) >= 1 and m.get('exitedEarly') is None,
    'defaultSinkUnchanged': before == after,
}
ok = all(checks[k] for k in ('c1_keyUsable', 'c2_decryptedOver30s', 'c3_pipewireStream', 'c5_treeWalked'))
pss = m.get('pssMedMiB')
verdict = 'VOID' if not ok else ('PASS' if pss <= 90 else 'FAIL')
print(json.dumps({
    'name': m['name'], 'verdict': verdict, 'pssMedMiB': pss, 'pssMaxMiB': m.get('pssMaxMiB'), 'rssMedMiB': m.get('rssMedMiB'), 'ussMedMiB': m.get('ussMedMiB'),
    'checks': checks,
    'detail': {k: st.get(k) for k in ('phase', 'keys', 'rejects', 'clearSamples', 'encryptedSamples', 'decryptOk', 'decryptFail', 'decodedDecryptedSeconds', 'decodeErrDecrypted', 'decodeErrClear', 'playedSeconds', 'segments', 'extra', 'shimLogs')},
    'routing': rt,
}, indent=1))
