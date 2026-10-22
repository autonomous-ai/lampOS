"""Offline cloud preparation using synthetic V1 and V2 credentials."""
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[1] / 'lamp_cloud_prepare.py'
SPEC = importlib.util.spec_from_file_location('lamp_cloud_prepare', SCRIPT)
migrate = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(migrate)


class CloudMigration(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.legacy = {'device_id': 'lamp-test', 'mqtt_endpoint': 'broker.test', 'mqtt_port': 1883,
                       'mqtt_username': 'lamp', 'mqtt_password': 'secret-broker',
                       'fa_channel': 'fa/lamp-test', 'fd_channel': 'fd/lamp-test',
                       'llm_base_url': 'https://voice.test/v1', 'llm_api_key': 'secret-voice',
                       'backend_base_url': 'https://backend.test/v1', 'backend_api_key': 'secret-backend'}
        self.save(migrate.V1_CONFIG, json.dumps(self.legacy))

    def save(self, name, text):
        target = self.root / name.lstrip('/')
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)
        return target

    def settings(self):
        self.save(migrate.V1_CONFIG, json.dumps(self.legacy))
        return dict(migrate.cloud_settings(self.root))

    def test_copies_device_broker_and_independent_backend_credentials(self):
        settings = self.settings()
        account = json.loads(settings[migrate.CLOUD_ACCOUNT])
        self.assertEqual(account['device_id'], 'lamp-test')
        self.assertEqual(account['mqtt_password'], 'secret-broker')
        self.assertEqual(account['mqtt_port'], 1883)
        self.assertEqual(account['llm_base_url'], 'https://backend.test/v1')
        self.assertEqual(settings[migrate.CLOUD_KEY], 'secret-backend\n')
        self.assertNotIn('api_key', settings[migrate.CLOUD_ACCOUNT])

    def test_legacy_backend_fallbacks_are_independent(self):
        self.legacy['backend_base_url'] = ' '
        settings = self.settings()
        self.assertEqual(json.loads(settings[migrate.CLOUD_ACCOUNT])['llm_base_url'], 'https://voice.test/v1')
        self.assertEqual(settings[migrate.CLOUD_KEY], 'secret-backend\n')
        self.legacy['backend_base_url'] = 'https://backend.test/v1'
        self.legacy['backend_api_key'] = ''
        settings = self.settings()
        self.assertEqual(settings[migrate.CLOUD_KEY], 'secret-voice\n')
        self.assertEqual(json.loads(settings[migrate.CLOUD_ACCOUNT])['llm_base_url'], 'https://backend.test/v1')

    def test_existing_valid_account_and_key_are_preserved_byte_for_byte(self):
        settings = self.settings()
        account = json.loads(settings[migrate.CLOUD_ACCOUNT])
        account['mqtt_password'] = 'rotated-secret'
        settings[migrate.CLOUD_ACCOUNT] = json.dumps(account)
        settings[migrate.CLOUD_KEY] = 'existing-backend-key'
        for name, text in settings.items():
            self.save(name, text)
        self.assertEqual(self.settings(), settings)

    def test_partial_pair_recovers_only_when_it_matches_v1(self):
        settings = self.settings()
        self.save(migrate.CLOUD_ACCOUNT, settings[migrate.CLOUD_ACCOUNT])
        self.assertEqual(self.settings(), settings)
        account = json.loads(settings[migrate.CLOUD_ACCOUNT])
        account['mqtt_password'] = 'other-key'
        self.save(migrate.CLOUD_ACCOUNT, json.dumps(account))
        with self.assertRaisesRegex(migrate.Refused, 'partial cloud account differs'):
            self.settings()
        self.save(migrate.CLOUD_ACCOUNT, settings[migrate.CLOUD_ACCOUNT])
        self.save(migrate.CLOUD_KEY, 'existing-key')
        self.legacy['device_id'] = 'another-device'
        with self.assertRaisesRegex(migrate.Refused, 'different V1 device_id'):
            self.settings()

    def test_invalid_credentials_refuse_without_exposing_secrets(self):
        for field, value in [('device_id', ''), ('mqtt_endpoint', ''), ('mqtt_username', ''),
                             ('fa_channel', ''), ('fd_channel', ''), ('mqtt_port', 0),
                             ('mqtt_port', 65536), ('mqtt_port', True), ('mqtt_port', 1883.5),
                             ('mqtt_password', {'secret': 'secret-broker'})]:
            with self.subTest(field=field, value=value):
                previous = self.legacy[field]
                self.legacy[field] = value
                with self.assertRaises(migrate.Refused) as error:
                    self.settings()
                self.assertNotIn('secret', str(error.exception))
                self.legacy[field] = previous
        self.legacy['backend_api_key'] = self.legacy['llm_api_key'] = ''
        with self.assertRaisesRegex(migrate.Refused, 'backend API key is missing'):
            self.settings()

    def test_credentials_are_private_and_idempotent(self):
        expected = self.settings()
        migrate.prepare(self.root)
        migrate.prepare(self.root)
        for name, text in expected.items():
            target = self.root / name.lstrip('/')
            self.assertEqual(target.stat().st_mode & 0o777, 0o600)
            self.assertEqual(target.stat().st_uid, os.geteuid())
            self.assertEqual(target.read_text(), text)

    def test_cloud_preparation_does_not_require_or_change_voice_credentials(self):
        self.legacy['llm_api_key'] = ''
        self.save(migrate.V1_CONFIG, json.dumps(self.legacy))
        voice = self.save('/etc/lampOS/voice/credential', 'provider-only-key')
        migrate.prepare(self.root)
        self.assertEqual(voice.read_text(), 'provider-only-key')

    def test_symlink_credential_is_refused_without_touching_its_target(self):
        victim = self.save('/other-key', 'leave-alone')
        target = self.root / migrate.CLOUD_KEY.lstrip('/')
        target.parent.mkdir(parents=True)
        target.symlink_to(victim)
        with self.assertRaisesRegex(migrate.Refused, 'symbolic links'):
            migrate.prepare(self.root)
        self.assertEqual(victim.read_text(), 'leave-alone')

    def test_prepared_v2_updates_without_legacy_configuration(self):
        migrate.prepare(self.root)
        (self.root / migrate.V1_CONFIG.lstrip('/')).unlink()
        migrate.prepare(self.root)

    def test_interrupted_pair_write_can_be_retried(self):
        original = migrate.write_private

        def interrupted(path, content):
            if path.name == 'backend-key':
                raise OSError('interrupted')
            original(path, content)

        with patch.object(migrate, 'write_private', side_effect=interrupted):
            with self.assertRaises(OSError):
                migrate.prepare(self.root)
        migrate.prepare(self.root)
        self.assertEqual((self.root / migrate.CLOUD_KEY.lstrip('/')).read_text(), 'secret-backend\n')

    def test_missing_auth_refuses_before_writes_and_keeper(self):
        with patch.object(migrate, 'cloud_settings', side_effect=migrate.Refused('secret')), \
                patch.object(migrate, 'write_private') as write, \
                patch.object(migrate.os, 'execv') as execute, \
                patch('sys.stderr') as error:
            self.assertEqual(migrate.main(['/package/bin/lamp-keeper', 'prepare']), 2)
            write.assert_not_called()
            execute.assert_not_called()
            self.assertNotIn('secret', str(error.write.call_args_list))

    def test_prepare_execs_real_keeper_with_all_arguments(self):
        with patch.object(migrate, 'prepare') as prepare, patch.object(migrate.os, 'execv') as execute:
            migrate.main(['/package/bin/lamp-keeper', 'prepare', '--example'])
            prepare.assert_called_once_with()
            execute.assert_called_once_with('/package/bin/lamp-keeper-real',
                                            ['/package/bin/lamp-keeper-real', 'prepare', '--example'])

    def test_serve_passes_through_without_credential_work(self):
        with patch.object(migrate, 'prepare') as prepare, patch.object(migrate.os, 'execv') as execute:
            migrate.main(['/package/bin/lamp-keeper', 'serve'])
            prepare.assert_not_called()
            execute.assert_called_once_with('/package/bin/lamp-keeper-real',
                                            ['/package/bin/lamp-keeper-real', 'serve'])

    def test_executable_wrapper_preserves_process_environment_and_arguments(self):
        wrapper = self.root / 'lamp-keeper'
        shutil.copyfile(SCRIPT, wrapper)
        real = self.root / 'lamp-keeper-real'
        real.write_text('#!' + sys.executable + '\nimport json, os, sys\n'
                        'print(json.dumps([os.getpid(), os.environ["KEEPER_TEST"], sys.argv[1:]]))\n')
        real.chmod(0o755)
        process = subprocess.Popen([sys.executable, str(wrapper), 'serve', '--example'],
                                   env={**os.environ, 'KEEPER_TEST': 'retained'}, stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE, text=True)
        stdout, stderr = process.communicate(timeout=10)
        self.assertEqual(process.returncode, 0, stderr)
        self.assertEqual(json.loads(stdout), [process.pid, 'retained', ['serve', '--example']])


if __name__ == '__main__':
    unittest.main()
