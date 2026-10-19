"""Exercise publisher validation and metadata preservation without network access."""
import json
from pathlib import Path
import subprocess
import tempfile
import sys
import unittest
import zipfile

SCRIPT = Path(__file__).resolve().parents[1] / 'upload-service.sh'
VALIDATOR = SCRIPT.read_text().split("<<'PY'\n", 1)[1].split('\nPY\n', 1)[0]


class ServicePublisherTests(unittest.TestCase):
    def run_case(self, metadata=None, replacements='hal os-server', version='1.2.3',
                 package_version='1.2.3', executable=True, extra=None, component='lamp-os'):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            payload, archive = root / 'payload.json', root / 'service.zip'
            payload.write_text(json.dumps(metadata or {'hal': {'version': '1.0.0'}}))
            with zipfile.ZipFile(archive, 'w') as package:
                package.writestr('VERSION', package_version)
                info = zipfile.ZipInfo('bin/service')
                info.external_attr = (0o100755 if executable else 0o100644) << 16
                package.writestr(info, '#!/bin/sh\nexit 0\n')
                if extra:
                    package.writestr(*extra)
            result = subprocess.run([sys.executable, '-', str(payload), component, version,
                                     str(archive), 'https://example.test/package.zip', 'abc', replacements],
                                    input=VALIDATOR, text=True, capture_output=True)
            return result, json.loads(payload.read_text())

    def test_preserves_other_components_and_promotion_floors(self):
        before = {'hal': {'version': '9.0.0'}, 'lamp-os': {
            'service': 'lamp-os', 'replaces': ['os-server', 'hal'], 'version': '1.0.0',
            'min_version': '0.9.0', 'min_version_by_device': {'lamp': '1.0.0'},
            'activate_by_device': {'lamp': True},
            'activation_release_by_device': {'lamp': {'version': '1.0.0', 'sha256': 'old'}}}}
        result, after = self.run_case(before)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(after['hal'], before['hal'])
        self.assertEqual(after['lamp-os']['min_version'], '0.9.0')
        self.assertEqual(after['lamp-os']['activate_by_device'], {'lamp': False})
        self.assertNotIn('activation_release_by_device', after['lamp-os'])
        self.assertEqual(after['lamp-os']['min_version_by_device'], {'lamp': '1.0.0'})

    def test_rejects_older_release_without_changing_feed(self):
        before = {'lamp-os': {'service': 'lamp-os', 'replaces': ['hal', 'os-server'],
                              'version': '2.0.0', 'sha256': 'old'}}
        result, after = self.run_case(before)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('older than', result.stderr)
        self.assertEqual(after, before)

    def test_republishing_same_artifact_preserves_activation_binding(self):
        entry = {'service': 'lamp-os', 'replaces': ['hal', 'os-server'],
                 'version': '1.2.3', 'sha256': 'abc', 'activate_by_device': {'lamp': True},
                 'activation_release_by_device': {'lamp': {'version': '1.2.3', 'sha256': 'abc'}}}
        result, after = self.run_case({'lamp-os': entry})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(after['lamp-os']['activation_release_by_device'], entry['activation_release_by_device'])
        self.assertEqual(after['lamp-os']['activate_by_device'], {'lamp': True})

    def test_rejects_topology_mutation(self):
        result, _ = self.run_case({'lamp-os': {'service': 'lamp-os', 'replaces': ['hal']}})
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Topology is immutable', result.stderr)

    def test_rejects_version_reuse_with_different_content(self):
        result, _ = self.run_case({'lamp-os': {'service': 'lamp-os', 'replaces': ['hal', 'os-server'],
                                             'version': '1.2.3', 'sha256': 'different'}})
        self.assertIn('different content', result.stderr)
        self.assertNotEqual(result.returncode, 0)

    def test_rejects_unmanaged_or_protected_replacements(self):
        for replacements in ('ssh', 'unknown-service', 'hal hal', 'lamp-os', 'recover', 'rollback', 'rollback-to-legacy'):
            with self.subTest(replacements=replacements):
                result, _ = self.run_case(replacements=replacements)
                self.assertNotEqual(result.returncode, 0)

    def test_cli_verbs_cannot_be_service_components(self):
        for component in ('recover', 'rollback', 'rollback-to-legacy'):
            result, _ = self.run_case(component=component)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('Invalid service component', result.stderr)

    def test_accepts_registered_predecessor(self):
        result, after = self.run_case({'previous-os': {'service': 'previous-os'}}, replacements='previous-os')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(after['lamp-os']['replaces'], ['previous-os'])

    def test_rejects_invalid_packages(self):
        link = zipfile.ZipInfo('link')
        link.external_attr = 0o120777 << 16
        for kwargs in ({'package_version': '0.0.0'}, {'executable': False},
                       {'extra': ('../escape', 'bad')}, {'extra': (link, '/etc/passwd')}):
            with self.subTest(kwargs=kwargs):
                result, _ = self.run_case(**kwargs)
                self.assertNotEqual(result.returncode, 0)


if __name__ == '__main__':
    unittest.main()
