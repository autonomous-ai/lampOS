"""Test the full-package release orchestration without compiling or publishing."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / 'upload-lamp-os.sh'
MOCK = r'''
import io, json, os, pathlib, sys, tarfile
name = pathlib.Path(sys.argv[0]).name
args = sys.argv[1:]
root = pathlib.Path(os.environ['MOCK_ROOT'])
with (root / 'calls').open('a') as f:
    f.write(json.dumps([name, *args]) + '\n')
if name == 'openssl':
    print('ED25519 Public-Key:')
elif name == 'git':
    if 'get-url' in args:
        print('https://example.invalid/repo.git')
    elif 'fetch' in args and os.environ.get('FETCH_FAIL'):
        sys.exit(1)
    elif 'rev-parse' in args:
        print('a' * 40)
    elif 'archive' in args:
        with tarfile.open(fileobj=sys.stdout.buffer, mode='w|') as tar:
            for path, data in [('lampOS/Cargo.lock', b'fixture\n'),
                               ('lampOS/rust-toolchain.toml', b'[toolchain]\nchannel = "1.96.0"\n')]:
                item = tarfile.TarInfo(path); item.size = len(data)
                tar.addfile(item, io.BytesIO(data))
elif name == 'docker' and args[0] == 'run' and os.environ.get('BUILD_FAIL'):
    sys.exit(1)
'''
PACKAGER = '''import json, os, pathlib, sys, zipfile
if os.environ.get('PACKAGE_FAIL'): sys.exit(1)
args = dict(zip(sys.argv[1::2], sys.argv[2::2]))
assert args['--source-commit'] == 'a' * 40
assert '--libraries' in args and '--adapter' in args
with zipfile.ZipFile(args['--output'], 'w') as z:
    z.writestr('VERSION', args['--version'])
pathlib.Path(os.environ['MOCK_ROOT'], 'packaged.json').write_text(json.dumps(args))
'''
PUBLISHER = '''#!/usr/bin/env bash
set -eu
[ "$COMPONENT" = lamp-os ]
[ "$REPLACES" = 'hal os-server' ]
[ -f "$PACKAGE" ]
printf '%s' "$V" > "$MOCK_ROOT/published"
'''


class UploadLampOSTests(unittest.TestCase):
    def run_case(self, build_only=False, signing=True, initial_version='0.0.9', **flags):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            checkout = root / 'checkout'
            scripts = checkout / 'scripts/release'; scripts.mkdir(parents=True)
            for name in (SCRIPT.name, 'ota-config.sh', 'ota-metadata.sh'):
                shutil.copy2(SCRIPT.parent / name, scripts / name)
            (scripts / 'lamp_os_package.py').write_text(PACKAGER)
            (scripts / 'fetch_lamp_os_assets.py').write_text('')
            (scripts / 'upload-service.sh').write_text(PUBLISHER)
            version_file = checkout / 'system/VERSION_LAMP_OS'
            version_file.parent.mkdir(); version_file.write_text(initial_version + '\n')
            bindir = root / 'bin'; bindir.mkdir()
            for name in ('git', 'docker', 'gsutil', 'openssl'):
                tool = bindir / name
                tool.write_text(f'#!{sys.executable}\n' + MOCK); tool.chmod(0o755)
            key = root / 'test-key'
            key.write_text('fixture; openssl is mocked')
            env = dict(os.environ, MOCK_ROOT=str(root), PATH=f'{bindir}:{os.environ["PATH"]}',
                       LAMP_OS_BUILD_ONLY='1' if build_only else '0', **flags)
            for name in ('OTA_SIGNING_PRIVATE_KEY', 'OTA_SIGNING_KEY_ID', 'LAMP_OS_OUTPUT', 'VERSION_FILE'):
                env.pop(name, None)
            if signing:
                env.update(OTA_SIGNING_PRIVATE_KEY=str(key), OTA_SIGNING_KEY_ID='fixture')
                if signing == 'partial':
                    env.pop('OTA_SIGNING_PRIVATE_KEY')
            result = subprocess.run(['bash', str(scripts / SCRIPT.name)], env=env, text=True, capture_output=True)
            calls = [json.loads(line) for line in (root / 'calls').read_text().splitlines()] if (root / 'calls').exists() else []
            published = (root / 'published').read_text() if (root / 'published').exists() else None
            return result, calls, published, version_file.read_text().strip()

    def test_publishes_complete_package_through_existing_signed_publisher(self):
        result, calls, published, version = self.run_case()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((published, version), ('0.0.10', '0.0.10'))
        self.assertTrue(any('refs/heads/lamp-voice' in call for call in calls))
        self.assertTrue(any(call[:2] == ['docker', 'run'] and 'linux/arm64' in call for call in calls))
        self.assertFalse(any('promote' in arg for call in calls for arg in call))

    def test_build_only_does_not_publish_or_bump_version(self):
        result, _, published, version = self.run_case(build_only=True, signing=False)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIsNone(published)
        self.assertEqual(version, '0.0.9')

    def test_unsigned_publish_without_key(self):
        result, calls, published, version = self.run_case(signing=False)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((published, version), ('0.0.10', '0.0.10'))

    def test_partial_signing_fails_before_expensive_build(self):
        result, calls, published, version = self.run_case(signing='partial')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('configure both', result.stderr)
        self.assertEqual(calls, [])
        self.assertIsNone(published)
        self.assertEqual(version, '0.0.9')

    def test_fetch_build_package_and_version_errors_never_publish(self):
        for flags in ({'FETCH_FAIL': '1'}, {'BUILD_FAIL': '1'}, {'PACKAGE_FAIL': '1'}, {'initial_version': 'bad'}):
            with self.subTest(flags=flags):
                result, _, published, version = self.run_case(**flags)
                self.assertNotEqual(result.returncode, 0)
                self.assertIsNone(published)
                self.assertEqual(version, flags.get('initial_version', '0.0.9'))


if __name__ == '__main__':
    unittest.main()
