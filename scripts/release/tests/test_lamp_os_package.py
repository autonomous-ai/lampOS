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
        (self.binaries / 'lamp-keeper').write_bytes(elf + b'usage\nlamp-keeper prepare\nlamp-keeper status\n')
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

    POLICY_TEMPLATE = ('[Unit]\nConditionHost=@UNIT_ID@\n[Service]\nDynamicUser=yes\nLoadCredential=jev.key:/etc/lampOS/policy/jev.key\n'
                       'StateDirectory=lampos-policy\nExecStart=/opt/lampOS/policy/@POLICY_SHA256@/lamp-policy '
                       '--key-file ${CREDENTIALS_DIRECTORY}/jev.key --log-dir /var/lib/lampos-policy --bus-root /run/lampos-bus\n')
    PRODUCT_POLICY = ('[Service]\nUser=lampos\nExecStart=/opt/lampOS/policy/current/lamp-policy --key-file %d/jev.key '
                      '--endpoint https://relay.example/jev/decisions --log-dir /var/lib/lampos-policy\n')

    PRODUCT_UNITS = {
        'lamp-voice.service': '[Service]\nEnvironment=LAMP_BUS=on\nEnvironment=LAMP_BUS_ROOT=/run/lampos-bus\nEnvironment=LAMP_BUS_AUDIO=on\n',
        'lamp-body.service': '[Service]\nEnvironment=LAMP_BUS=on\n',
        'lamp-eyes.service': '[Service]\nEnvironment=LAMP_BUS=on\n',
        'lamp-memory.service': '[Service]\nExecStart=/opt/lampOS/memory/current/lamp-memory --dir /m --bus-root /run/lampos-bus\n',
    }

    def write_generic_units(self, extra=''):
        for unit in pkg.DEFAULT_UNITS:
            content = '[Unit]\nConditionHost=@UNIT_ID@\n'
            if unit.endswith('.service'):
                content += '[Service]\n' + extra
            if unit == 'lamp-controls.service':
                content += 'ExecStart=/opt/lampOS/controls/@CONTROLS_SHA256@/lamp-controls --config /opt/lampOS/controls/@CONTROLS_SHA256@/controls.json\n'
            if unit == 'lamp-policy.service':
                content = self.POLICY_TEMPLATE
            if unit == 'lamp-memory.service':
                content += 'ExecStart=/opt/lampOS/memory/@MEMORY_SHA256@/lamp-memory --dir /m\n'
            self.write_source('deploy/units/' + unit, content.encode())
            if unit.endswith('.service'):
                self.write_source('deploy/product/' + unit, self.PRODUCT_UNITS.get(unit, '[Service]\n').encode())
        self.write_source('deploy/product/lamp-policy.service', self.PRODUCT_POLICY.encode())
        for name in pkg.PROVISION_SETTINGS:
            self.write_source('deploy/product/settings/' + name, b'{"fixture": true}\n')
        for name in pkg.PROVISIONING_BINARIES:
            (self.binaries / name).write_bytes(self.adapter.read_bytes() + name.encode())

    def test_generic_carries_migration_tool_and_setting_templates(self):
        self.write_generic_units()
        manifest = self.build_generic()
        with zipfile.ZipFile(self.output) as archive:
            self.assertEqual(archive.getinfo('tools/lamp-body-measure').external_attr >> 16 & 0o777, 0o755)
            for name in pkg.PROVISION_SETTINGS:
                self.assertEqual(archive.read('provision/' + name), b'{"fixture": true}\n')
        self.assertNotIn('lamp-body-measure', manifest['binaries'])
        (self.binaries / 'lamp-body-measure').unlink()
        with self.assertRaisesRegex(ValueError, 'missing or unsafe file: lamp-body-measure'):
            self.build_generic()
        self.write_generic_units()
        self.write_source('deploy/product/settings/voice-provider.json', b'{"key": "AIza' + b'x' * 35 + b'"}')
        with self.assertRaisesRegex(ValueError, 'nonsecret setting template'):
            self.build_generic()

    def test_generic_runs_the_seated_measure_when_the_source_has_it(self):
        # An older source has no seated measure: the package is as before.
        self.write_generic_units()
        manifest = self.build_generic()
        self.assertNotIn(pkg.SEATED_MEASURE_UNIT, manifest['units'])
        # lampOS #2163: the oneshot reads a lamp without poses, seated, with
        # the tool beside lamp-body, and joins no bus.
        self.write_source('deploy/units/' + pkg.SEATED_MEASURE_UNIT, (
            '[Unit]\nConditionHost=@UNIT_ID@\n[Service]\nType=oneshot\nUser=@LAMP_USER@\n'
            "ExecStart=/bin/sh -c 'exec /opt/lampOS/body/@BODY_SHA256@/lamp-body-measure "
            "--device @SERVO_DEVICE@ --unit-id @UNIT_ID@ --lock-directory @MOTOR_LOCK_DIR@ --seated "
            "--write /var/lib/lampos-body/measured.env'\n").encode())
        manifest = self.build_generic()
        self.assertIn(pkg.SEATED_MEASURE_UNIT, manifest['units'])
        body = manifest['binaries']['lamp-body']
        with zipfile.ZipFile(self.output) as archive:
            names = archive.namelist()
            unit = archive.read('units/' + pkg.SEATED_MEASURE_UNIT).decode()
            placeholders = json.loads(archive.read('package-manifest.json'))['placeholders']
            mode = archive.getinfo(f'runtime/body/{body}/lamp-body-measure').external_attr >> 16 & 0o777
        self.assertIn('tools/lamp-body-measure', names)
        self.assertEqual(mode, 0o755)
        self.assertIn(f'@RELEASE_ROOT@/runtime/body/{body}/lamp-body-measure --device @SERVO_DEVICE@', unit)
        self.assertNotIn('LAMP_BUS', unit)
        self.assertNotIn('lamp-body-measure', manifest['binaries'])
        self.assertTrue({'SERVO_DEVICE', 'MOTOR_LOCK_DIR', 'LAMP_USER'} <= set(placeholders))

    def build_generic(self):
        return pkg.package(self.source, self.binaries, self.assets, None, '1.2.3', self.output, self.adapter, self.libraries, 'a' * 40)

    def test_generic_policy_matches_product_identity_state_and_relay(self):
        self.write_generic_units()
        self.build_generic()
        with zipfile.ZipFile(self.output) as archive:
            policy = archive.read('units/lamp-policy.service').decode()
            manifest = json.loads(archive.read('package-manifest.json'))
        self.assertNotIn('DynamicUser', policy)
        self.assertIn('User=@LAMP_USER@\nGroup=@LAMP_USER@\n', policy)
        self.assertIn('StateDirectory=lampos-policy\nStateDirectoryMode=0700\n', policy)
        self.assertIn('/lamp-policy --endpoint https://relay.example/jev/decisions --key-file', policy)
        self.assertIn('LAMP_USER', manifest['placeholders'])
        self.assertIn('/etc/lampOS/policy/jev.key', manifest['required_paths'])

    def test_generic_parts_join_the_bus_as_their_product_units_do(self):
        self.write_generic_units()
        self.build_generic()
        with zipfile.ZipFile(self.output) as archive:
            units = {u: archive.read('units/' + u).decode() for u in pkg.DEFAULT_UNITS if u.endswith('.service')}
        self.assertIn('Environment=LAMP_BUS=on\nEnvironment=LAMP_BUS_AUDIO=on\n', units['lamp-voice.service'])
        for unit in ('lamp-body.service', 'lamp-eyes.service'):
            self.assertIn('Environment=LAMP_BUS=on\n', units[unit])
            self.assertNotIn('LAMP_BUS_AUDIO', units[unit])
        self.assertIn('/lamp-memory --dir /m --bus-root /run/lampos-bus\n', units['lamp-memory.service'])
        self.assertNotIn('LAMP_BUS=', units['lamp-name.service'])
        self.assertEqual(units['lamp-policy.service'].count('--bus-root'), 1)

    def test_generic_needs_each_parts_product_unit(self):
        self.write_generic_units()
        (self.source / 'deploy/product/lamp-eyes.service').unlink()
        with self.assertRaisesRegex(ValueError, 'deploy/product/lamp-eyes.service'):
            self.build_generic()

    def test_generic_policy_rewrite_fails_closed(self):
        self.write_generic_units()
        self.write_source('deploy/product/lamp-policy.service', b'[Service]\nExecStart=/x/lamp-policy --log-dir /y\n')
        with self.assertRaisesRegex(ValueError, 'names no --endpoint'):
            self.build_generic()
        self.write_source('deploy/product/lamp-policy.service', self.PRODUCT_POLICY.encode())
        self.write_source('deploy/units/lamp-policy.service',
                          self.POLICY_TEMPLATE.replace('DynamicUser=yes\n', 'User=someone\n').encode())
        with self.assertRaisesRegex(ValueError, 'template changed'):
            self.build_generic()

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

    def test_cloud_skills_carried_without_units_and_tools_excluded(self):
        runtime = ('lamp-cloud', 'lamp-skills')
        excluded = ('lamp-body-measure', 'lamp-policy-replay')
        for name in runtime + excluded:
            (self.binaries / name).write_bytes(self.adapter.read_bytes() + name.encode())
        manifest = self.build()
        with zipfile.ZipFile(self.output) as archive:
            names = archive.namelist()
        for name in runtime:
            part = name.removeprefix('lamp-')
            self.assertIn(f'runtime/{part}/{manifest["binaries"][name]}/{name}', names)
            self.assertNotIn(f'units/{name}.service', names)
        for name in excluded:
            self.assertNotIn(name, manifest['binaries'])
            self.assertFalse(any(path.endswith('/' + name) for path in names))

    def test_cloud_and_skills_declared_in_source_must_be_built(self):
        for part in ('cloud', 'skills'):
            with self.subTest(part=part):
                self.write_source(f'crates/{part}/Cargo.toml', f'[package]\nname="lamp-{part}"\n'.encode())
                with self.assertRaisesRegex(ValueError, f'missing runtime binary declared in source: lamp-{part}'):
                    self.build()
                (self.source / f'crates/{part}/Cargo.toml').unlink()

    def test_preflight_marker_requires_keeper_prepare(self):
        self.build()
        with zipfile.ZipFile(self.output) as archive:
            self.assertEqual(archive.read('SERVICE_PREFLIGHT'), b'1\n')
            self.assertIn(b'\nlamp-keeper prepare\n', archive.read('bin/lamp-keeper'))
        (self.binaries / 'lamp-keeper').write_bytes(self.adapter.read_bytes() + b'usage\nlamp-keeper status\n')
        self.output.unlink()
        with self.assertRaisesRegex(ValueError, 'lacks the prepare command'):
            self.build()
        self.assertFalse(self.output.exists())

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
        self.write_generic_units()
        pkg.package(self.source, self.binaries, self.assets, None, '1.2.3', self.output, self.adapter, self.libraries, 'a' * 40)
        with zipfile.ZipFile(self.output) as archive:
            manifest = json.loads(archive.read('package-manifest.json'))
            self.assertEqual(manifest['profile_path'], '/etc/lampOS/package-profile.json')
            self.assertNotIn('unit_id', manifest)
            self.assertIn('UNIT_ID', manifest['placeholders'])
            self.assertIn(b'/etc/lampOS/controls/controls.json', archive.read('units/lamp-controls.service'))

    def test_generic_rejects_unknown_new_placeholder(self):
        self.write_generic_units(extra='Value=@NEW_UNREVIEWED@\n')
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
