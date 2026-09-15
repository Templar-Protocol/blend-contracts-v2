import base64, hashlib, json, os, pathlib, re, subprocess, sys, time

root = pathlib.Path('/tmp/blend-kani-20260914-d_iw7rpr')
source = pathlib.Path('/tmp/blend-kani-20260914-d_iw7rpr/recovery-20260915T080802Z/astra-integrated-ready')
env = dict(os.environ)
env['PATH'] = '/tmp/adr8-local-20260913.RLSEWPiF/tools/bin:' + env['PATH']
records = []
env.pop('STELLAR_CONTRACT_ID', None)
env.pop('SOROBAN_CONTRACT_ID', None)
env['ADR8_DIFF_OUTPUT_DIR'] = str(root / 'astra-final-integration-differential-observations')

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def execute(name, argv, extra=None):
    log = root / ('astra-final-integration-' + name + '.log')
    start = time.monotonic()
    with log.open('w') as stream:
        result = subprocess.run(argv, cwd=source, env={**env, **(extra or {})}, stdout=stream, stderr=subprocess.STDOUT)
    text = log.read_text()
    record = {'stage': name, 'argv': argv, 'cwd': str(source), 'extra_env': extra or {}, 'exit': result.returncode, 'elapsed': time.monotonic() - start, 'log': log.name, 'log_sha256': digest(log), 'terminal': text[-4000:]}
    records.append(record)
    (root / 'astra-final-integration-validation-results.json').write_text(json.dumps(records, indent=2) + '\n')
    print(name, 'exit', result.returncode, 'seconds', round(record['elapsed'], 2), flush=True)
    print(text[-1200:], flush=True)
    if result.returncode:
        sys.exit(result.returncode)
    return text

inputs = {str(p.relative_to(source)): digest(p) for p in source.rglob('*') if p.is_file() and 'target' not in p.relative_to(source).parts and (p.suffix == '.rs' or p.name in ('Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', 'Makefile'))}
(root / 'astra-final-integration-validation-inputs.json').write_text(json.dumps(inputs, indent=2) + '\n')
versions = {binary: subprocess.check_output([binary, '--version'], cwd=source, env=env, text=True).strip() for binary in ('rustc', 'cargo', 'stellar')}
(root / 'astra-final-integration-validation-versions.json').write_text(json.dumps(versions, indent=2) + '\n')
assert versions['rustc'].startswith('rustc 1.81.'), versions
assert '22.6.0' in versions['stellar'], versions
execute('build', ['make', 'build'])
artifacts = {}
for name in ('pool', 'backstop', 'pool_factory'):
    old = root / 'source/target/wasm32-unknown-unknown/optimized' / (name + '.wasm')
    new = source / 'target/wasm32-unknown-unknown/optimized' / (name + '.wasm')
    specs = []
    for label, wasm in (('baseline', old), ('revised', new)):
        encoded = subprocess.check_output(['stellar', 'contract', 'info', 'interface', '--wasm', str(wasm), '--output', 'xdr-base64'], cwd=source, env=env)
        spec = base64.b64decode(encoded.strip(), validate=True)
        assert spec, 'Empty ABI is not evidence'
        (root / ('astra-final-integration-abi-' + name + '-' + label + '.xdr')).write_bytes(spec)
        specs.append(spec)
    artifacts[name] = {'baseline_sha256': digest(old), 'revised_sha256': digest(new), 'baseline_bytes': old.stat().st_size, 'revised_bytes': new.stat().st_size, 'wasm_equal': old.read_bytes() == new.read_bytes(), 'abi_equal': specs[0] == specs[1], 'abi_sha256': hashlib.sha256(specs[1]).hexdigest()}
(root / 'astra-final-integration-artifact-results.json').write_text(json.dumps(artifacts, indent=2) + '\n')
print(json.dumps(artifacts, indent=2), flush=True)
assert all(a['abi_equal'] for a in artifacts.values()), 'ABI mismatch blocks acceptance'
assert inputs == {name: digest(source / name) for name in inputs}, 'Source changed during build'
if '--build-only' in sys.argv:
    sys.exit(0)
text = execute('workspace', ['cargo', 'test', '--locked', '--all', '--tests', '--target-dir', str(root / 'astra-final-native-target')])
counts = [int(count) for count in re.findall(r'test result: ok\. (\d+) passed', text)]
assert counts and sum(counts) > 0, 'Zero native tests is not acceptance'
print('workspace passed tests', sum(counts), flush=True)
wasm = source / 'target/wasm32-unknown-unknown/optimized'
base = pathlib.Path('/tmp/adr8-local-20260913.RLSEWPiF/baseline')
text = execute('differential', ['cargo', 'test', '--locked', '-p', 'test-suites', '--test', 'adr8_differential', '--target-dir', str(root / 'astra-final-native-target'), '--', '--ignored', '--exact', 'adr8_base_fork_differential', '--nocapture'], {'ADR8_BASE_POOL_WASM': str(base / 'pool.wasm'), 'ADR8_BASE_BACKSTOP_WASM': str(base / 'backstop.wasm'), 'ADR8_FORK_POOL_WASM': str(wasm / 'pool.wasm'), 'ADR8_FORK_BACKSTOP_WASM': str(wasm / 'backstop.wasm')})
assert 'test result: ok. 1 passed; 0 failed;' in text, 'Missing exact differential verdict'
assert inputs == {name: digest(source / name) for name in inputs}, 'Source changed during validation'
print('Revised ABI, workspace and exact differential gates passed.', flush=True)
