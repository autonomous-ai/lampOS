"""Release-payload checks use synthetic ELF/model fixtures, never a device."""
import importlib.util
import json
from pathlib import Path
import struct
import tempfile
import unittest
from unittest.mock import patch
import zipfile

SPEC = importlib.util.spec_from_file_location('lamp_os_package', Path(__file__).parents[1] / 'lamp_os_package.py')
pkg = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(pkg)


class PackageTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.source, self.binaries, self.assets = (self.root / n for n in ('source', 'binaries', 'assets'))
        for directory in (self.source, self.binaries, self.assets):
            directory.mkdir()
        elf = bytearray(64)
        elf[:6] = b'\x7fELF\x02\x01'
        struct.pack_into('<HH', elf, 16, 3, 183)
        for name in pkg.CORE_BINARIES:
            (self.binaries / name).write_bytes(elf + name.encode())
        self.adapter = self.root / 'adapter'
        self.adapter.write_bytes(elf)
        self.libraries = self.root / 'lib'
        self.libraries.mkdir()
        (self.libraries / 'libasound.so.2').write_bytes(elf)
        for name in pkg.ASSETS:
            (self.assets / name).write_bytes(name.encode())
        self.hashes = {name: pkg.digest(name.encode()) for name in pkg.ASSETS}
        self.hash_patch = patch.object(pkg, 'ASSETS', self.hashes)
        self.hash_patch.start()
        self.addCleanup(self.hash_patch.stop)
        for name in ('models/name/name.onnx', 'models/stop/stop.onnx',
                     'models/policy/local-weights.json', 'deploy/character/lamp.txt'):
            self.write_source(name, b'fixture')
        self.profile = {'substitutions': {'UNIT_ID': 'lamp-test'}, 'required_paths': ['/etc/lampOS/voice/provider.json'],
                        'units': ['lampos.target', 'lampos-voice.slice', 'lampos-parts.slice', 'lamp-voice.service']}
        for unit in self.profile['units']:
            content = '[Unit]\nConditionHost=@UNIT_ID@\n'
            if unit == 'lamp-voice.service':
                content += '[Service]\nExecStart=/opt/lampOS/releases/@VOICE_SHA256@/lamp-voice serve\n'
            self.write_source('deploy/units/' + unit, content.encode())
        self.output = self.root / 'release.zip'

    def write_source(self, name, data):
        path = self.source / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)

    def build(self):
        return pkg.package(self.source, self.binaries, self.assets, self.profile, '1.2.3', self.output, self.adapter, self.libraries, 'a' * 40)

    def test_new_or_missing_runtime_cannot_be_silently_omitted(self):
        (self.binaries / 'lamp-new-runtime').write_bytes(self.adapter.read_bytes())
        with self.assertRaisesRegex(ValueError, 'unclassified binaries'):
            self.build()
        (self.binaries / 'lamp-new-runtime').unlink()
        self.write_source('crates/ears/Cargo.toml', b'[package]\nname="lamp-ears"\n')
        with self.assertRaisesRegex(ValueError, 'missing runtime binary'):
            self.build()

    def test_unit_credentials_are_preflight_prerequisites(self):
        unit = self.source / 'deploy/units/lamp-voice.service'
        unit.write_text(unit.read_text() + 'LoadCredential=provider:/etc/lampOS/test.key\n')
        self.build()
        with zipfile.ZipFile(self.output) as archive:
            manifest = json.loads(archive.read('package-manifest.json'))
        self.assertIn('/etc/lampOS/test.key', manifest['required_paths'])

    def test_new_upstream_runtime_and_development_binaries(self):
        runtime = ('lamp-selftest', 'lamp-settings', 'lamp-setup')
        development = ('lamp-memory-fixture', 'lamp-records-fixture', 'lamp-same-voice')
        for name in runtime + development:
            (self.binaries / name).write_bytes(self.adapter.read_bytes() + name.encode())
        manifest = self.build()
        for name in runtime:
            self.assertIn(name, manifest['binaries'])
        for name in development:
            self.assertNotIn(name, manifest['binaries'])
        with zipfile.ZipFile(self.output) as archive:
            names = archive.namelist()
            for name in runtime:
                self.assertTrue(any(path.endswith('/' + name) for path in names))
                self.assertNotIn('units/' + name + '.service', names)
            for name in development:
                self.assertFalse(any(path.endswith('/' + name) for path in names))

    def test_complete_deterministic_archive_and_modes(self):
        manifest = self.build()
        first = self.output.read_bytes()
        self.build()
        self.assertEqual(first, self.output.read_bytes())
        with zipfile.ZipFile(self.output) as archive:
            self.assertEqual(archive.read('VERSION'), b'1.2.3\n')
            self.assertEqual(archive.getinfo('bin/service').external_attr >> 16 & 0o777, 0o755)
            self.assertEqual(archive.getinfo('bin/lamp-keeper').external_attr >> 16 & 0o777, 0o755)
            self.assertIn(b'@RELEASE_ROOT@/runtime/', archive.read('units/lamp-voice.service'))
            info = json.loads(archive.read('package-manifest.json'))
            self.assertEqual(info['unit_id'], 'lamp-test')
            self.assertEqual(info['required_paths'], self.profile['required_paths'])
            sha = manifest['binaries']['lamp-voice']
            self.assertIn(f'runtime/releases/{sha}/lamp-turn-end', archive.namelist())
            self.assertIn(f'runtime/releases/{sha}/libonnxruntime.so.1.30.0', archive.namelist())
            self.assertEqual(archive.read('SERVICE_CLEANUP'), b'1\n')

    def test_generic_profile_defers_only_declared_device_values(self):
        for unit in pkg.DEFAULT_UNITS:
            content = '[Unit]\nConditionHost=@UNIT_ID@\n'
            if unit.endswith('.service'):
                content += '[Service]\n'
            if unit == 'lamp-controls.service':
                content += 'ExecStart=/opt/lampOS/controls/@CONTROLS_SHA256@/lamp-controls --config /opt/lampOS/controls/@CONTROLS_SHA256@/controls.json\n'
            self.write_source('deploy/units/' + unit, content.encode())
        pkg.package(self.source, self.binaries, self.assets, None, '1.2.3', self.output, self.adapter, self.libraries, 'a' * 40)
        with zipfile.ZipFile(self.output) as archive:
            manifest = json.loads(archive.read('package-manifest.json'))
            self.assertEqual(manifest['profile_path'], '/etc/lampOS/package-profile.json')
            self.assertNotIn('unit_id', manifest)
            self.assertIn('UNIT_ID', manifest['placeholders'])
            self.assertIn(b'/etc/lampOS/controls/controls.json', archive.read('units/lamp-controls.service'))

    def test_generic_rejects_unknown_new_placeholder(self):
        for unit in pkg.DEFAULT_UNITS:
            self.write_source('deploy/units/' + unit, b'[Service]\nConditionHost=@UNIT_ID@\nValue=@NEW_UNREVIEWED@\n')
        with self.assertRaisesRegex(ValueError, 'unresolved'):
            pkg.package(self.source, self.binaries, self.assets, None, '1.2.3', self.output, self.adapter, self.libraries, 'a' * 40)

    def test_missing_native_libraries(self):
        (self.libraries / 'libasound.so.2').unlink()
        with self.assertRaisesRegex(ValueError, 'libasound'):
            self.build()

    def test_reject_lfs_pointer(self):
        self.write_source('models/name/name.onnx', b'version https://git-lfs.github.com/spec/v1\noid sha256:fake')
        with self.assertRaisesRegex(ValueError, 'Git LFS'):
            self.build()

    def test_library_environment_and_source_revision(self):
        manifest = self.build()
        self.assertEqual(manifest['source_commit'], 'a' * 40)
        with zipfile.ZipFile(self.output) as archive:
            self.assertIn('lib/libasound.so.2', archive.namelist())
            self.assertIn(b'Environment=LD_LIBRARY_PATH=@RELEASE_ROOT@/lib', archive.read('units/lamp-voice.service'))

    def test_missing_core_binary(self):
        (self.binaries / 'lamp-body-worker').unlink()
        with self.assertRaisesRegex(ValueError, 'missing or unsafe'):
            self.build()
        self.assertFalse(self.output.exists())

    def test_wrong_architecture(self):
        data = bytearray((self.binaries / 'lamp-voice').read_bytes())
        struct.pack_into('<H', data, 18, 62)
        (self.binaries / 'lamp-voice').write_bytes(data)
        with self.assertRaisesRegex(ValueError, 'ARM64'):
            self.build()

    def test_unpinned_asset(self):
        (self.assets / 'melspectrogram.onnx').write_bytes(b'changed')
        with self.assertRaisesRegex(ValueError, 'checksum mismatch'):
            self.build()

    def test_unit_traversal(self):
        self.profile['units'].append('../evil.service')
        with self.assertRaisesRegex(ValueError, 'invalid unit'):
            self.build()

    def test_unresolved_template(self):
        self.write_source('deploy/units/lampos.target', b'Value=@UNSET@\n')
        with self.assertRaisesRegex(ValueError, 'unresolved'):
            self.build()

    def test_missing_packaged_reference(self):
        self.write_source('deploy/units/lampos.target', b'ExecStart=/opt/lampOS/missing\n')
        with self.assertRaisesRegex(ValueError, 'missing packaged path'):
            self.build()

    def test_secret_or_injected_substitution(self):
        for value in ('a\nExecStart=bad', '-----BEGIN RSA PRIVATE KEY-----'):
            self.profile['substitutions']['VOICE_OPTIONS'] = value
            with self.assertRaisesRegex(ValueError, 'sensitive substitution'):
                self.build()

    def test_no_independent_updater_activation(self):
        self.profile['units'].append('lamp-update.service')
        with self.assertRaisesRegex(ValueError, 'bootstrap owns'):
            self.build()

    def test_no_symlink_asset(self):
        path = self.assets / 'melspectrogram.onnx'
        path.unlink()
        path.symlink_to(self.assets / 'embedding_model.onnx')
        with self.assertRaisesRegex(ValueError, 'missing or unsafe'):
            self.build()

    def test_no_default_device_identity(self):
        self.profile['substitutions'].clear()
        with self.assertRaisesRegex(ValueError, 'UNIT_ID'):
            self.build()

    def test_bad_external_preflight_path(self):
        self.profile['required_paths'] = ['/etc/../root/secret']
        with self.assertRaisesRegex(ValueError, 'absolute external'):
            self.build()


if __name__ == '__main__':
    unittest.main()
