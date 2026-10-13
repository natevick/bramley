import hashlib, json, os, struct, subprocess, sys, tomllib

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
UNSUPPORTED_DEP_KEYS = ('path', 'artifact')


def fail(msg):
    sys.exit(f'crate-publish-body: {msg}')


def package(name):
    out = subprocess.run(['cargo', 'metadata', '--format-version', '1', '--no-deps'],
                         cwd=ROOT, check=True, capture_output=True).stdout
    found = [p for p in json.loads(out)['packages'] if p['name'] == name]
    if len(found) != 1:
        fail(f'expected one package named {name}, found {len(found)}')
    return found[0]


def dependency(d):
    for key in UNSUPPORTED_DEP_KEYS:
        if d.get(key):
            fail(f'dependency {d["name"]} has {key}; crates.io cannot take it')
    if d.get('registry') is not None:
        fail(f'dependency {d["name"]} comes from registry {d["registry"]}, not crates.io')
    dep = {
        'optional': d['optional'],
        'default_features': d['uses_default_features'],
        'name': d['name'],
        'features': d['features'],
        'version_req': d['req'],
        'target': d['target'],
        'kind': d['kind'] or 'normal',
    }
    if d.get('rename') is not None:
        dep['explicit_name_in_toml'] = d['rename']
    return dep


def new_crate(p):
    manifest_dir = os.path.dirname(p['manifest_path'])
    with open(p['manifest_path'], 'rb') as f:
        if 'badges' in tomllib.load(f):
            fail('the manifest has [badges]; cargo metadata does not expose them, so this script cannot carry them')
    readme = None
    if p['readme']:
        with open(os.path.join(manifest_dir, p['readme']), encoding='utf-8') as f:
            readme = f.read()
    if p['license_file'] and not os.path.exists(os.path.join(manifest_dir, p['license_file'])):
        fail(f'the license file {p["license_file"]} does not exist')
    return {
        'name': p['name'],
        'vers': p['version'],
        'deps': [dependency(d) for d in p['dependencies']],
        'features': dict(sorted(p['features'].items())),
        'authors': p['authors'],
        'description': p['description'],
        'documentation': p['documentation'],
        'homepage': p['homepage'],
        'readme': readme,
        'readme_file': p['readme'],
        'keywords': p['keywords'],
        'categories': p['categories'],
        'license': p['license'],
        'license_file': p['license_file'],
        'repository': p['repository'],
        'badges': {},
        'links': p['links'],
        'rust_version': p['rust_version'],
    }


def write_private(path, data, gid):
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o640)
    with os.fdopen(fd, 'wb') as f:
        f.write(data)
    os.chown(path, -1, gid)
    os.chmod(path, 0o640)


def main():
    if len(sys.argv) != 4:
        sys.exit('usage: crate-publish-body.py <package> <path to .crate> <output dir>')
    name, crate_path, out_dir = sys.argv[1:]
    meta = new_crate(package(name))
    expected = f'{meta["name"]}-{meta["vers"]}.crate'
    if os.path.basename(crate_path) != expected:
        fail(f'{crate_path} is not {expected}')
    with open(crate_path, 'rb') as f:
        crate = f.read()
    meta_bytes = json.dumps(meta, separators=(',', ':'), ensure_ascii=False).encode('utf-8')
    body = struct.pack('<I', len(meta_bytes)) + meta_bytes + struct.pack('<I', len(crate)) + crate

    json_len = struct.unpack_from('<I', body, 0)[0]
    crate_len = struct.unpack_from('<I', body, 4 + json_len)[0]
    embedded = body[8 + json_len:]
    if 8 + json_len + crate_len != len(body) or embedded != crate or json.loads(body[4:4 + json_len]) != meta:
        fail('the body does not parse back to the metadata and crate it was built from')

    gid = os.stat(out_dir).st_gid
    write_private(os.path.join(out_dir, 'body.bin'), body, gid)
    write_private(os.path.join(out_dir, 'metadata.json'), meta_bytes, gid)
    print(f'crate     {len(crate)} bytes sha256 {hashlib.sha256(crate).hexdigest()}')
    print(f'embedded  {crate_len} bytes sha256 {hashlib.sha256(embedded).hexdigest()}')
    print(f'metadata  {json_len} bytes')
    print(f'body.bin  {len(body)} bytes sha256 {hashlib.sha256(body).hexdigest()}')


if __name__ == '__main__':
    main()
