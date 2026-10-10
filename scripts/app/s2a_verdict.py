import json, struct, sys, zlib
from pathlib import Path

def png_stats(path):
    data = Path(path).read_bytes()
    w, h, depth, ctype = struct.unpack('>IIBB', data[16:26])
    if (depth, ctype) != (8, 6):
        return w, h, None
    raw, pos = b'', 8
    while pos < len(data):
        n, kind = struct.unpack('>I4s', data[pos:pos + 8])
        if kind == b'IDAT':
            raw += data[pos + 8:pos + 8 + n]
        pos += 12 + n
    raw, stride, prev, colours = zlib.decompress(raw), w * 4, bytearray(w * 4), set()
    for y in range(h):
        f, line = raw[y * (stride + 1)], bytearray(raw[y * (stride + 1) + 1:(y + 1) * (stride + 1)])
        for i in range(stride):
            a, b, c = line[i - 4] if i >= 4 else 0, prev[i], prev[i - 4] if i >= 4 else 0
            p = a + b - c
            pred = [0, a, b, (a + b) // 2, a if abs(p - a) <= abs(p - b) and abs(p - a) <= abs(p - c) else b if abs(p - b) <= abs(p - c) else c][f]
            line[i] = (line[i] + pred) & 255
        colours.update(bytes(line[i:i + 4]) for i in range(0, stride, 4 * 7))
        prev = line
    return w, h, len(colours)

def main():
    name, mode = sys.argv[1], sys.argv[2]
    out = Path(sys.argv[3]) if len(sys.argv) > 3 else Path('target/app-runs')
    f = lambda ext: out / f'{name}.{ext}'
    rows = [json.loads(l) for l in f('status.jsonl').read_text().splitlines() if l.strip()] if f('status.jsonl').exists() else []
    final = json.loads(f('status.json').read_text()) if f('status.json').exists() else None
    void, fail, facts = [], [], {}
    renderer = (final or {}).get('glRenderer') or ''
    facts['glRenderer'] = renderer
    if final is None: void.append('no status file')
    if 'llvmpipe' in renderer.lower() or not any(k in renderer for k in ('AMD', 'radeonsi')): void.append(f'renderer {renderer!r}')
    if not final or final.get('framesPainted', 0) < 1: void.append('no frames painted')
    facts['maxPlayerThreads'] = max((r['playerThreads'] for r in rows), default=None)
    if f('pss.json').exists():
        pss = json.loads(f('pss.json').read_text())
        facts.update(pssMedMiB=pss['pssMedMiB'], pssMaxMiB=pss['pssMaxMiB'], samples=pss['samples'], procCountMin=pss['procCountMin'])
        if pss['samples'] < 5 or pss['exitedEarly'] is not None: void.append('measurement did not run to completion')
        if pss['procCountMin'] != 1: void.append(f"{pss['procCountMin']} processes")
    if f('png').exists():
        w, h, n = png_stats(f('png'))
        facts['png'] = f'{w}x{h}, {n} sampled colours'
        if (w, h) != (1100, 720) or not n or n < 2: fail.append(f"screenshot {facts['png']}")
    if mode == 'idle':
        if facts['maxPlayerThreads'] is None and 'pssMedMiB' in facts: void.append('thread watch never sampled')
        if facts['maxPlayerThreads']: fail.append('a player thread ran before any play')
        if final and final.get('engineStarted'): fail.append('engineStarted is true')
        if facts.get('pssMedMiB', 0) > 100: fail.append('PSS median over the 100 MiB R2 bar')
    else:
        sink = f('sink-name').read_text().strip() if f('sink-name').exists() else None
        routing = json.loads(f('routing.json').read_text()) if f('routing.json').exists() else {}
        facts['routing'] = routing.get('targets')
        if f('default-before').read_text() != f('default-after').read_text(): void.append('default sink changed')
        if f('sink-after').read_text().strip() != 'gone': void.append('null sink left behind')
        playing = [r['status'] for r in rows if r['status'].get('state') == 'Playing' and (r['status'].get('position') or 0) > 0]
        started = len(playing) >= 2 and playing[-1]['position'] > playing[0]['position']
        if started and (not sink or routing.get('targets') != [sink]): void.append(f'audio not routed only to the null sink: {routing.get("targets")}')
        if (final or {}).get('banner') is not None: fail.append(f"banner shown: {final['banner']}")
        facts.update(finalState=(final or {}).get('state'), finalPosition=(final or {}).get('position'), itemId=(final or {}).get('itemId'), banner=(final or {}).get('banner'))
        if not final or final.get('state') != 'Playing' or (final.get('position') or 0) <= 35 or final.get('itemId') != '6781024437':
            fail.append('not playing 6781024437 past 35 s')
        if final and final.get('applied', 0) < final.get('seq', 1): fail.append('final state is optimistic: applied < seq')
        if len(playing) < 2 or playing[-1]['position'] - playing[0]['position'] < 2: fail.append('position did not advance')
    verdict = 'VOID' if void else 'FAIL' if fail else 'PASS'
    print(json.dumps({'name': name, 'mode': mode, 'verdict': verdict, 'void': void, 'fail': fail, **facts}, indent=1))

main()
