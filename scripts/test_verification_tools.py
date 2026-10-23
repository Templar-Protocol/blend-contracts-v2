"""Fail-closed regressions for local tool integrity and extraction provenance."""

import hashlib
import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parent.parent
original_path = sys.path[:]
sys.path.insert(0, str(ROOT / 'scripts'))
try:
    import verify_verus
    spec = importlib.util.spec_from_file_location('local_extraction', ROOT / 'verification/extraction/run.py')
    extraction = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(extraction)
finally:
    sys.path[:] = original_path


def sha(data):
    return hashlib.sha256(data).hexdigest()


class ToolBoundaryTest(unittest.TestCase):
    def test_every_loaded_verus_component_is_checked_before_execution(self):
        pin = json.loads((ROOT / 'verification/verus/toolchain.json').read_text())
        components = {
            'verus': ('verus', 'executable_sha256'),
            'rust_verify': ('verus', 'rust_verify_sha256'),
            'z3': ('solver', 'sha256'),
            'vstd.vir': ('libraries', 'bundled_vstd_vir_sha256'),
            'libverus_builtin_macros.so': ('libraries', 'bundled_builtin_macros_sha256'),
            'libverus_state_machines_macros.so': ('libraries', 'bundled_state_machines_macros_sha256'),
            'libverus_builtin.rlib': ('libraries', 'bundled_builtin_rlib_sha256'),
            'libvstd.rlib': ('libraries', 'bundled_libvstd_rlib_sha256'),
        }
        with tempfile.TemporaryDirectory(prefix='verus-pin-check-') as temporary:
            root = Path(temporary)
            home = root / 'tools'
            bundle = home / 'verus-x86-linux'
            bundle.mkdir(parents=True)
            files = []
            for name, (section, key) in components.items():
                path = bundle / name
                data = f'fixture component {name}'.encode()
                path.write_bytes(data)
                pin[section][key] = sha(data)
                files.append((path, data))
            compiler = home / 'rustup/toolchains' / pin['rust']['toolchain'] / 'bin/rustc'
            compiler.parent.mkdir(parents=True)
            compiler.write_bytes(b'compiler fixture')
            pin['rust']['rustc_sha256'] = sha(b'compiler fixture')
            files.append((compiler, b'compiler fixture'))
            metadata = root / 'verification/verus/toolchain.json'
            metadata.parent.mkdir(parents=True)
            metadata.write_text(json.dumps(pin))
            with (mock.patch.object(verify_verus, 'ROOT', root),
                  mock.patch.dict(os.environ, {'VERUS_HOME': str(home)}),
                  mock.patch.object(verify_verus.subprocess, 'run', side_effect=AssertionError('program launched'))):
                for path, data in files:
                    with self.subTest(component=path.name):
                        path.write_bytes(data + b' altered')
                        with self.assertRaises(ValueError):
                            verify_verus.verify(lambda *_: self.fail('prover launched'), root)
                        path.write_bytes(data)

    def test_changed_extraction_compiler_cannot_reach_charon(self):
        pin = json.loads((ROOT / 'verification/extraction/evaluation.json').read_text())['toolchain']['rust']
        with tempfile.TemporaryDirectory(prefix='charon-pin-check-') as temporary:
            sandbox = Path(temporary) / 'extraction'
            tools = sandbox / 'tools'
            tools.mkdir(parents=True)
            expected = {}
            for name in extraction.EXPECTED:
                data = f'tool fixture {name}'.encode()
                (tools / name).write_bytes(data)
                expected[name] = sha(data)
            compiler = sandbox / pin['toolchain_directory'] / 'bin/rustc'
            compiler.parent.mkdir(parents=True)
            compiler.write_bytes(b'existing but wrong compiler')
            (sandbox / 'cargo-home').mkdir()
            with (mock.patch.object(extraction, 'SANDBOX', sandbox),
                  mock.patch.object(extraction, 'TOOLS', tools),
                  mock.patch.object(extraction, 'EXPECTED', expected),
                  mock.patch.object(extraction.subprocess, 'run', side_effect=AssertionError('program launched'))):
                with self.assertRaises(ValueError):
                    extraction.checked_toolchain()

    def test_untrusted_tool_parent_or_home_blocks_before_hashing(self):
        for backend in ('verus', 'extraction'):
            for component in ('parent', 'home'):
                with self.subTest(backend=backend, component=component):
                    with tempfile.TemporaryDirectory(prefix='tool-custody-check-') as temporary:
                        parent = Path(temporary) / 'tool-parent'
                        home = parent / backend
                        tools = home / 'tools'
                        tools.mkdir(parents=True)
                        (home / 'rustup').mkdir()
                        (home / 'cargo-home').mkdir()
                        for name in extraction.EXPECTED:
                            (tools / name).write_bytes(b'tool fixture')
                        (parent if component == 'parent' else home).chmod(0o777)
                        if backend == 'verus':
                            with (mock.patch.dict(os.environ, {'VERUS_HOME': str(home)}),
                                  mock.patch.object(verify_verus, 'hash_file', side_effect=AssertionError('hashing started'))):
                                with self.assertRaises(ValueError):
                                    verify_verus.verify(lambda *_: self.fail('prover launched'), Path(temporary))
                        else:
                            with (mock.patch.object(extraction, 'SANDBOX', home),
                                  mock.patch.object(extraction, 'TOOLS', tools),
                                  mock.patch.object(extraction, 'digest', side_effect=AssertionError('hashing started'))):
                                with self.assertRaises(ValueError):
                                    extraction.checked_toolchain()

    def test_writable_extraction_binary_blocks_before_hashing(self):
        with tempfile.TemporaryDirectory(prefix='tool-binary-custody-') as temporary:
            sandbox = Path(temporary) / 'extraction'
            tools = sandbox / 'tools'
            tools.mkdir(parents=True)
            (sandbox / 'rustup').mkdir()
            (sandbox / 'cargo-home').mkdir()
            for name in extraction.EXPECTED:
                (tools / name).write_bytes(b'tool fixture')
            (tools / 'charon-driver').chmod(0o666)
            with (mock.patch.object(extraction, 'SANDBOX', sandbox),
                  mock.patch.object(extraction, 'TOOLS', tools),
                  mock.patch.object(extraction, 'digest', side_effect=AssertionError('hashing started'))):
                with self.assertRaises(ValueError):
                    extraction.checked_toolchain()

    def test_llbc_requires_matching_origin_target_and_sources(self):
        source = {'pool/src/math.rs': 'current-source-digest'}
        target = 'x86_64-unknown-linux-gnu'
        value = {'has_errors': False, 'translated': {'target_information': [{'key': target}]}}
        payload = json.dumps(value).encode()
        failed_payload = json.dumps({**value, 'has_errors': True}).encode()
        base = {
            'stage': 'charon', 'exit_code': 0, 'target': target,
            'tool_sha256': extraction.EXPECTED,
            'generated_sha256': {'pool.llbc': sha(payload)},
            'source_sha256_before': source, 'source_sha256_after': source,
        }
        with tempfile.TemporaryDirectory(prefix='llbc-custody-check-') as temporary:
            path = Path(temporary) / 'pool.llbc'
            receipt = path.with_name('invocation.json')
            cases = [
                ({**base, 'exit_code': 1}, payload, None, source),
                ({**base, 'stage': 'aeneas'}, payload, None, source),
                ({**base, 'generated_sha256': {'pool.llbc': sha(failed_payload)}},
                 failed_payload, None, source),
                ({**base, 'target': 'wasm32v1-none'}, payload, None, source),
                (base, payload, 'wasm32v1-none', source),
                (base, payload + b' ', None, source),
                (base, payload, None, {'pool/src/math.rs': 'changed-source'}),
                ({**base, 'source_sha256_before': {}}, payload, None, source),
                ({**base, 'tool_sha256': {}}, payload, None, source),
            ]
            for index, (origin, data, requested, current) in enumerate(cases):
                with self.subTest(case=index):
                    path.write_bytes(data)
                    receipt.write_text(json.dumps(origin))
                    with self.assertRaises(ValueError):
                        extraction.admit_llbc(path, requested, current)


if __name__ == '__main__':
    unittest.main()
