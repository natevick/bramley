import json, subprocess, sys
status = json.load(open(sys.argv[1]))
pid = status.get('pid')
dump = json.loads(subprocess.run(['pw-dump'], capture_output=True, text=True, check=True).stdout)
nodes = {o['id']: (o.get('info') or {}).get('props') or {} for o in dump if o.get('type') == 'PipeWire:Interface:Node'}
ours = [i for i, p in nodes.items() if str(p.get('application.process.id')) == str(pid) and p.get('media.class', '').startswith('Stream/Output')]
targets = []
for o in dump:
    if o.get('type') != 'PipeWire:Interface:Link':
        continue
    info = o.get('info') or {}
    if info.get('output-node-id') in ours:
        targets.append(nodes.get(info.get('input-node-id'), {}).get('node.name'))
print(json.dumps({'pid': pid, 'streams': [{'id': i, 'name': nodes[i].get('node.name'), 'app': nodes[i].get('application.name')} for i in ours], 'targets': sorted(set(t for t in targets if t))}))
