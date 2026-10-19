"""Service OTA transactions using isolated files, fake systemd and fault injection."""
import copy
import json
import os
from pathlib import Path
import stat
import subprocess
import tempfile
import types
import unittest
from unittest.mock import patch
import zipfile

SCRIPT = Path(__file__).resolve().parents[1] / 'software-update'
SOURCE = SCRIPT.read_text().split("<<'SERVICE_OTA_PY'\n", 1)[1].split('\nSERVICE_OTA_PY', 1)[0]


class ServiceOTATests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.m = types.ModuleType('service_ota_test')
        exec(compile(SOURCE.replace('/root/config/bootstrap.json', str(self.root / 'bootstrap.json')).replace('/opt/hal/.env', str(self.root / 'hal.env')).replace('/root/config/config.json', str(self.root / 'config.json')), str(SCRIPT), 'exec'), self.m.__dict__)
        self.m.STATE = self.root / 'state'
        self.m.ROOT = self.root / 'code'
        self.m.DATA = self.root / 'data'
        self.m.UNITS = self.root / 'units'
        self.m.UNITS.mkdir()
        self.m.REGISTRY = self.m.STATE / 'ownership.json'
        self.m.JOURNAL = self.m.STATE / 'transition.json'
        self.states = {'hal': 'active', 'os-server': 'active'}
        self.enabled = set()
        self.events = []
        self.fail_stop = None
        self.fail_health = set()
        self.m.systemctl = self.systemctl
        self.m.healthy = self.healthy
        self.m.legacy_healthy = self.legacy_healthy
        self.m.verify_group_empty = lambda key, state: None
        patcher = patch.object(self.m.os, 'sync', create=True, return_value=None)
        patcher.start()
        self.addCleanup(patcher.stop)
        (self.root / 'bootstrap.json').write_text('{}')

    def systemctl(self, *args):
        action = args[0]
        key = args[1].removesuffix('.service') if len(args) > 1 else None
        self.events.append(args)
        unit_exists = key in self.m.LEGACY or (self.m.UNITS / ((key or '') + '.service')).exists()
        if action == 'show':
            active = self.states.get(key, 'inactive')
            output = f'LoadState={"loaded" if unit_exists else "not-found"}\nActiveState={active}\nMainPID={100 if active == "active" else 0}\nControlPID=0\n'
            return subprocess.CompletedProcess(args, 0, output, '')
        if action == 'stop':
            if key == self.fail_stop:
                raise RuntimeError('injected stop failure')
            self.states[key] = 'inactive'
        if action == 'start':
            self.assertTrue(unit_exists, 'Starting missing service unit')
            self.assertFalse((self.m.STATE / 'blocked' / key).exists())
            registry = self.m.load()
            self.assertFalse(any(self.states.get(old) == 'active' for old in registry['blocked']), 'Two hardware owners')
            self.states[key] = 'active'
        if action == 'enable':
            self.enabled.add(key)
        if action == 'disable':
            self.enabled.discard(key)
        return subprocess.CompletedProcess(args, 0, '', '')

    def healthy(self, key, entry):
        if (key, entry['version']) in self.fail_health:
            raise RuntimeError('injected health failure')

    def legacy_healthy(self, key):
        if (key, 'legacy') in self.fail_health:
            raise RuntimeError('injected legacy health failure')

    def package(self, version='0.1.0', extra=()):
        path = self.root / (version + '.zip')
        with zipfile.ZipFile(path, 'w') as archive:
            item = zipfile.ZipInfo('bin/service')
            item.external_attr = (stat.S_IFREG | 0o755) << 16
            archive.writestr(item, '#!/bin/sh\nexit 0\n')
            archive.writestr('VERSION', version)
            if not any(name == 'SERVICE_PREFLIGHT' for name, _ in extra):
                archive.writestr('SERVICE_PREFLIGHT', '1')
            for name, value in extra:
                archive.writestr(name, value)
        return path

    def install(self, key='lamp-os', version='0.1.0', replaces=('hal', 'os-server'), activate=True):
        entry = dict(service=key, replaces=list(replaces), version=version)
        self.m.install(key, entry, str(self.package(version)), activate)

    def assert_legacy(self):
        self.assertEqual(self.m.load(), self.m.empty())
        self.assertFalse(self.m.JOURNAL.exists())
        self.assertEqual(self.states['hal'], 'active')
        self.assertEqual(self.states['os-server'], 'active')

    def test_architecture_checkpoint_survives_update_and_version_rollback(self):
        self.install(version='0.1.0')
        checkpoint = (self.m.STATE / 'activation/lamp-os.json').read_text()
        self.install(version='0.2.0')
        self.m.rollback('lamp-os')
        self.assertEqual(self.m.load()['components']['lamp-os']['version'], '0.1.0')
        self.assertEqual((self.m.STATE / 'activation/lamp-os.json').read_text(), checkpoint)
        self.m.rollback('lamp-os', to_legacy=True)
        self.assert_legacy()
        self.assertFalse((self.m.STATE / 'activation/lamp-os.json').exists())

    def test_interrupted_legacy_rollback_retains_original_checkpoint(self):
        self.install()
        self.install(version='0.2.0')
        real_commit, real_recover = self.m.commit, self.m.recover
        self.m.commit = lambda journal: (_ for _ in ()).throw(RuntimeError('power loss'))
        self.m.recover = lambda: None
        with self.assertRaisesRegex(RuntimeError, 'power loss'):
            self.m.rollback('lamp-os', to_legacy=True)
        self.assertTrue((self.m.STATE / 'activation/lamp-os.json').exists())
        self.m.commit, self.m.recover = real_commit, real_recover
        self.m.recover()
        self.assertEqual(self.states['lamp-os'], 'active')
        self.m.rollback('lamp-os', to_legacy=True)
        self.assert_legacy()

    def test_committed_legacy_rollback_finishes_cleanup_after_interruption(self):
        self.install()
        real_finalize, real_recover = self.m.finalize, self.m.recover
        self.m.finalize = lambda journal: (_ for _ in ()).throw(RuntimeError('disk error'))
        self.m.recover = lambda: None
        self.m.rollback('lamp-os', to_legacy=True)
        self.assertEqual(json.loads(self.m.JOURNAL.read_text())['phase'], 'committed')
        self.m.finalize, self.m.recover = real_finalize, real_recover
        self.m.recover()
        self.assert_legacy()
        self.assertFalse((self.m.STATE / 'activation/lamp-os.json').exists())

    def test_original_checkpoint_backfilled_before_first_update(self):
        self.install()
        (self.m.STATE / 'activation/lamp-os.json').unlink()
        self.install(version='0.2.0')
        self.m.rollback('lamp-os', to_legacy=True)
        self.assert_legacy()

    def test_missing_original_checkpoint_never_guesses_legacy_state(self):
        self.install()
        self.install(version='0.2.0')
        (self.m.STATE / 'activation/lamp-os.json').unlink()
        events = list(self.events)
        with self.assertRaisesRegex(RuntimeError, 'No saved activation checkpoint'):
            self.m.rollback('lamp-os', to_legacy=True)
        self.assertEqual(events, self.events)
        self.assertEqual(self.states['lamp-os'], 'active')

    def test_legacy_rollback_preserves_independent_service(self):
        self.install()
        self.install(key='other-worker', replaces=())
        self.install(version='0.2.0')
        self.m.rollback('lamp-os', to_legacy=True)
        self.assertEqual(self.states['other-worker'], 'active')
        self.assertIn('other-worker', self.m.load()['components'])
        self.assertEqual(self.states['hal'], 'active')

    def test_preflight_failure_never_stops_legacy(self):
        archive = self.package(extra=[('SERVICE_PREFLIGHT', '1')])
        entry = dict(service='lamp-os', replaces=['hal', 'os-server'], version='0.1.0')
        with patch.object(self.m, 'run', side_effect=RuntimeError('missing profile')):
            with self.assertRaisesRegex(RuntimeError, 'missing profile'):
                self.m.install('lamp-os', entry, str(archive), True)
        self.assert_legacy()
        self.assertEqual(self.events, [])
        self.assertEqual(list(self.m.UNITS.iterdir()), [])
        self.assertFalse((self.m.STATE / 'blocked').exists())
        self.assertFalse((self.m.STATE / 'enabled').exists())
        self.assertEqual(list((self.m.ROOT / 'lamp-os/releases').iterdir()), [])
        self.assertEqual(list((self.m.DATA / 'lamp-os/releases').iterdir()), [])

    def test_second_prepare_failure_restores_previous_runtime_and_data(self):
        self.install(version='0.1.0')
        old = copy.deepcopy(self.m.load())
        old_data = Path(old['components']['lamp-os']['data'])
        (old_data / 'conversation').write_text('before preflight')
        calls = []

        def prepare(*args, **kwargs):
            self.assertEqual(args[1], 'prepare')
            staged = Path(kwargs['env']['AUTONOMOUS_SERVICE_DATA'])
            self.assertNotEqual(staged, old_data)
            calls.append(staged)
            if len(calls) == 1:
                self.assertEqual(self.states['lamp-os'], 'active')
                self.assertEqual((staged / 'conversation').read_text(), 'before preflight')
                # Simulate a last legitimate runtime write before stopping it.
                (old_data / 'conversation').write_text('last runtime write')
                (staged / 'candidate-only').write_text('discard this provisional copy')
                return
            self.assertEqual(self.states['lamp-os'], 'inactive')
            self.assertEqual((staged / 'conversation').read_text(), 'last runtime write')
            self.assertFalse((staged / 'candidate-only').exists())
            raise RuntimeError('final staged configuration rejected')

        with patch.object(self.m, 'run', side_effect=prepare):
            with self.assertRaisesRegex(RuntimeError, 'final staged configuration rejected'):
                self.install(version='0.2.0', activate=False)
        self.assertEqual(len(calls), 2)
        self.assertEqual(self.m.load(), old)
        self.assertEqual(self.states['lamp-os'], 'active')
        self.assertEqual(self.states['hal'], 'inactive')
        self.assertEqual(self.states['os-server'], 'inactive')
        self.assertEqual((old_data / 'conversation').read_text(), 'last runtime write')
        self.assertFalse((old_data / 'candidate-only').exists())
        self.assertFalse(self.m.JOURNAL.exists())
        # Both previous-version rollback and architecture rollback remain intact.
        self.m.rollback('lamp-os', to_legacy=True)
        self.assert_legacy()

    def test_preflight_runs_before_stop_with_staged_private_data(self):
        archive = self.package(extra=[('SERVICE_PREFLIGHT', '1')])
        entry = dict(service='lamp-os', replaces=['hal', 'os-server'], version='0.1.0')
        def prepare(*args, **kwargs):
            self.assertEqual(args[1], 'prepare')
            self.assertEqual(self.states['hal'], 'active')
            self.assertFalse(self.m.JOURNAL.exists())
            data = Path(kwargs['env']['AUTONOMOUS_SERVICE_DATA'])
            self.assertTrue(data.is_dir())
            (data / 'prepared').write_text('ready')
        with patch.object(self.m, 'run', side_effect=prepare):
            self.m.install('lamp-os', entry, str(archive), True)
        self.assertEqual((Path(self.m.load()['components']['lamp-os']['data']) / 'prepared').read_text(), 'ready')

    def test_old_lamp_package_without_preflight_never_stops_legacy(self):
        archive = self.root / 'old.zip'
        with zipfile.ZipFile(archive, 'w') as z:
            item = zipfile.ZipInfo('bin/service')
            item.external_attr = (stat.S_IFREG | 0o755) << 16
            z.writestr(item, '#!/bin/sh\nexit 0\n')
            z.writestr('VERSION', '0.1.0')
        entry = dict(service='lamp-os', replaces=['hal', 'os-server'], version='0.1.0')
        with self.assertRaisesRegex(RuntimeError, 'requires SERVICE_PREFLIGHT'):
            self.m.install('lamp-os', entry, str(archive), True)
        self.assert_legacy()
        self.assertEqual(self.events, [])

    def test_unknown_preflight_contract_rejects_before_stop(self):
        archive = self.package(extra=[('SERVICE_PREFLIGHT', '2')])
        entry = dict(service='lamp-os', replaces=['hal', 'os-server'], version='0.1.0')
        with self.assertRaisesRegex(RuntimeError, 'preflight contract'):
            self.m.install('lamp-os', entry, str(archive), True)
        self.assert_legacy()
        self.assertEqual(self.events, [])

    def test_failed_cleanup_retains_checkpoint_for_safe_retry(self):
        self.install()
        checkpoint = self.m.STATE / 'activation/lamp-os.json'
        self.fail_stop = 'lamp-os'
        with self.assertRaisesRegex(RuntimeError, 'stop failure'):
            self.m.rollback('lamp-os', to_legacy=True)
        self.assertTrue(checkpoint.exists())
        self.assertNotEqual(self.states['hal'], 'active')
        self.fail_stop = None
        self.m.recover()
        self.m.rollback('lamp-os', to_legacy=True)
        self.assert_legacy()

    def test_cleanup_contract_and_legacy_unit_upgrade(self):
        self.install()
        unit = self.m.UNITS / 'lamp-os.service'
        unit.write_text(self.m.unit_text('lamp-os', cleanup=False))
        self.install(version='0.2.0', activate=False)
        self.assertIn('ExecStopPost=', unit.read_text())
        self.assertIn('/current/SERVICE_CLEANUP || exec ', unit.read_text())
        self.assertIn('/current/bin/service cleanup', unit.read_text())
        self.assertIn('TimeoutStopSec=60', unit.read_text())

    def test_failed_cleanup_is_not_treated_as_successful_stop(self):
        self.install()
        release = Path(self.m.load()['components']['lamp-os']['release'])
        (release / 'SERVICE_CLEANUP').write_text('1')
        with patch.object(self.m, 'run', side_effect=RuntimeError('cleanup failed')) as run:
            with self.assertRaisesRegex(RuntimeError, 'cleanup failed'):
                self.m.stop('lamp-os')
            self.assertEqual(run.call_args.args[1], 'cleanup')
            self.assertEqual(run.call_args.kwargs['env']['AUTONOMOUS_SERVICE_NAME'], 'lamp-os')
        self.assertNotEqual(self.states['hal'], 'active')

    def test_invalid_cleanup_contract_rejected_before_stopping_legacy(self):
        entry = dict(service='lamp-os', replaces=['hal', 'os-server'], version='0.1.0')
        archive = self.package(extra=[('SERVICE_CLEANUP', 'unsupported')])
        with self.assertRaisesRegex(RuntimeError, 'cleanup contract'):
            self.m.install('lamp-os', entry, str(archive), True)
        self.assert_legacy()
        self.assertEqual(self.events, [])

    def test_promoted_activation_requires_actual_device_and_floor(self):
        patcher = patch.dict(os.environ, {'DEVICE_TYPE': ' lamp '})
        patcher.start()
        self.addCleanup(patcher.stop)
        entry = dict(service='lamp-os', version='1.2.3', activate_by_device={'lamp': True},
                     min_version_by_device={'lamp': '1.0.0'}, sha256='a' * 64,
                     activation_release_by_device={'lamp': {'version': '1.2.3', 'sha256': 'a' * 64}})
        self.assertTrue(self.m.promoted_activation('lamp-os', entry))
        for change in ({'activate_by_device': {'lamp': False}}, {'activate_by_device': {'lamp': 'true'}},
                       {'activate_by_device': {'other': True}}, {'activate_by_device': []},
                       {'min_version_by_device': {}}, {'min_version_by_device': {'lamp': '0.0.0'}},
                       {'min_version_by_device': {'lamp': '2.0.0'}}, {'min_version_by_device': {'lamp': 1}},
                       {'service': 'hal'}, {'activation_release_by_device': {}},
                       {'version': '1.2.4'}, {'sha256': 'b' * 64}):
            with self.subTest(change=change), self.assertRaises(RuntimeError):
                self.m.promoted_activation('lamp-os', dict(entry, **change))
        self.assertEqual(self.events, [])

    def test_fresh_promoted_version_respects_rollback_guard(self):
        entry = dict(service='lamp-os', version='1.2.3', activate_by_device={'lamp': True},
                     min_version_by_device={'lamp': '1.0.0'}, sha256='a' * 64,
                     activation_release_by_device={'lamp': {'version': '1.2.3', 'sha256': 'a' * 64}})
        config = self.root / 'bootstrap.json'
        config.write_text(json.dumps({'rollback_versions': {'lamp-os': '1.2.3'}}))
        with patch.dict(os.environ, {'DEVICE_TYPE': 'lamp'}):
            with self.assertRaisesRegex(RuntimeError, 'blocked after rollback'):
                self.m.promoted_activation('lamp-os', entry)
            entry['version'] = '1.2.4'
            entry['activation_release_by_device']['lamp']['version'] = '1.2.4'
            self.assertTrue(self.m.promoted_activation('lamp-os', entry))
            config.write_text('{"rollback_versions": []}')
            with self.assertRaisesRegex(RuntimeError, 'Invalid rollback_versions'):
                self.m.promoted_activation('lamp-os', entry)
        self.assertEqual(self.events, [])

    def test_promoted_activation_device_config_fallback(self):
        patcher = patch.dict(os.environ, {'DEVICE_TYPE': ''})
        patcher.start()
        self.addCleanup(patcher.stop)
        (self.root / 'config.json').write_text('{"device_type":" lamp "}')
        entry = dict(service='lamp-os', version='1.2.3', activate_by_device={'lamp': True},
                     min_version_by_device={'lamp': '1.0.0'}, sha256='a' * 64,
                     activation_release_by_device={'lamp': {'version': '1.2.3', 'sha256': 'a' * 64}})
        self.assertTrue(self.m.promoted_activation('lamp-os', entry))
        os.environ['DEVICE_TYPE'] = 'other'
        with self.assertRaises(RuntimeError):
            self.m.promoted_activation('lamp-os', entry)

    def test_failed_promoted_activation_blocks_retry_and_restores_legacy(self):
        self.fail_health.add(('lamp-os', '0.1.0'))
        entry = dict(service='lamp-os', version='0.1.0', replaces=['hal', 'os-server'])
        with self.assertRaisesRegex(RuntimeError, 'health failure'):
            self.m.install('lamp-os', entry, str(self.package()), True, promoted=True)
        self.assert_legacy()
        self.assertEqual(json.loads((self.root / 'bootstrap.json').read_text())['rollback_versions'],
                         {'lamp-os': '0.1.0'})

    def test_interrupted_promoted_activation_blocks_retry_on_recovery(self):
        entry = dict(service='lamp-os', version='0.1.0', replaces=['hal', 'os-server'])
        original = self.m.recover
        self.m.recover = lambda: None
        self.fail_health.add(('lamp-os', '0.1.0'))
        with self.assertRaises(RuntimeError):
            self.m.install('lamp-os', entry, str(self.package()), True, promoted=True)
        self.m.recover = original
        original()
        self.assert_legacy()
        self.assertEqual(json.loads((self.root / 'bootstrap.json').read_text())['rollback_versions'],
                         {'lamp-os': '0.1.0'})

    def test_recovery_restores_legacy_even_when_rejection_marker_cannot_be_written(self):
        entry = dict(service='lamp-os', version='0.1.0', replaces=['hal', 'os-server'])
        self.fail_health.add(('lamp-os', '0.1.0'))
        (self.root / 'bootstrap.json').write_text('invalid-json')
        with self.assertRaises(ValueError):
            self.m.install('lamp-os', entry, str(self.package()), True, promoted=True)
        self.assertEqual(self.states['hal'], 'active')
        self.assertEqual(self.states['os-server'], 'active')
        self.assertTrue(self.m.JOURNAL.exists())
        (self.root / 'bootstrap.json').write_text('{}')
        self.m.recover()
        self.assert_legacy()
        self.assertEqual(json.loads((self.root / 'bootstrap.json').read_text())['rollback_versions'],
                         {'lamp-os': '0.1.0'})

    def test_metadata_does_not_activate_device(self):
        with self.assertRaisesRegex(RuntimeError, 'requires --activate'):
            self.install(activate=False)
        self.assertEqual(self.events, [])
        self.assert_legacy()

    def test_install_upgrade_and_private_data_rollback(self):
        self.install()
        original = self.m.load()['components']['lamp-os']
        (Path(original['data']) / 'memory').write_text('old')
        self.install(version='0.2.0', activate=False)
        updated = self.m.load()['components']['lamp-os']
        self.assertEqual((Path(updated['data']) / 'memory').read_text(), 'old')
        (Path(updated['data']) / 'memory').write_text('incompatible new schema')
        self.m.rollback('lamp-os')
        self.assertEqual(self.m.load()['components']['lamp-os'], original)
        self.assertEqual((Path(original['data']) / 'memory').read_text(), 'old')
        self.assertEqual(json.loads((self.root / 'bootstrap.json').read_text())['rollback_versions']['lamp-os'], '0.2.0')

    def test_independent_services_coexist_and_rollback_preserves_other_updates(self):
        self.install()
        self.install('vision-os', replaces=())
        self.install('vision-os', version='0.2.0', replaces=(), activate=False)
        other = self.m.load()['components']['vision-os']
        self.m.rollback('lamp-os')
        self.assertEqual(self.m.load()['components'], {'vision-os': other})
        self.assertEqual(self.states['vision-os'], 'active')
        self.assertEqual(self.states['hal'], 'active')
        self.assertEqual(self.m.status('hal'), 'legacy')

    def test_chained_replacements_and_reverse_rollback(self):
        self.install('lamp-a')
        self.install('lamp-b', replaces=('lamp-a',))
        self.install('lamp-c', replaces=('lamp-b',))
        for key in ('hal', 'os-server', 'lamp-a', 'lamp-b'):
            self.assertEqual(self.m.status(key), 'blocked')
        with self.assertRaisesRegex(RuntimeError, 'without a successor'):
            self.m.rollback('lamp-a')
        self.m.rollback('lamp-c')
        self.assertEqual(self.states['lamp-b'], 'active')
        self.assertEqual(self.m.status('lamp-a'), 'blocked')
        self.m.rollback('lamp-b')
        self.assertEqual(self.states['lamp-a'], 'active')
        self.m.rollback('lamp-a')
        self.assert_legacy()

    def test_topology_cannot_change_even_with_activate(self):
        self.install()
        before = copy.deepcopy(self.m.load())
        with self.assertRaisesRegex(RuntimeError, 'Topology change'):
            self.install(version='0.2.0', replaces=('hal',))
        self.assertEqual(before, self.m.load())

    def test_unknown_protected_or_conflicting_replacements_rejected(self):
        for key in ('bootstrap', 'ssh', '../hal', 'recover', 'rollback'):
            with self.subTest(key=key), self.assertRaises(RuntimeError):
                self.install(replaces=(key,))
        self.install()
        with self.assertRaisesRegex(RuntimeError, 'already claimed'):
            self.install('other-os')
        for key in ('hal', 'recover', 'rollback'):
            with self.assertRaisesRegex(RuntimeError, 'non-reserved'):
                self.install(key, replaces=())

    def test_existing_unmanaged_service_or_dropin_is_not_overwritten(self):
        unit = self.m.UNITS / 'lamp-os.service'
        unit.write_text('unmanaged')
        with self.assertRaisesRegex(RuntimeError, 'unmanaged'):
            self.install()
        self.assertEqual(unit.read_text(), 'unmanaged')
        unit.unlink()
        (self.m.UNITS / 'lamp-os.service.d').mkdir()
        with self.assertRaisesRegex(RuntimeError, 'unmanaged'):
            self.install()
        self.assertFalse(self.m.JOURNAL.exists())

    def test_stop_failure_never_starts_candidate(self):
        self.fail_stop = 'hal'
        with self.assertRaisesRegex(RuntimeError, 'stop failure'):
            self.install()
        self.assertNotIn(('start', 'lamp-os.service'), self.events)
        self.assertTrue(self.m.JOURNAL.exists())
        self.fail_stop = None
        self.m.recover()
        self.assert_legacy()

    def test_candidate_stop_failure_does_not_restart_previous_owner(self):
        def fail(key, entry):
            self.fail_stop = key
            raise RuntimeError('bad candidate')
        self.m.healthy = fail
        with self.assertRaisesRegex(RuntimeError, 'stop failure'):
            self.install()
        self.assertEqual(self.states['hal'], 'inactive')
        self.assertTrue(self.m.JOURNAL.exists())
        self.fail_stop = None
        self.m.recover()
        self.assert_legacy()

    def test_old_health_failure_retains_recovery_journal(self):
        self.fail_health = {('lamp-os', '0.1.0'), ('hal', 'legacy')}
        with self.assertRaisesRegex(RuntimeError, 'legacy health failure'):
            self.install()
        self.assertTrue(self.m.JOURNAL.exists())
        self.fail_health.clear()
        self.m.recover()
        self.assert_legacy()

    def test_crash_after_prepare_is_recoverable_and_retryable(self):
        real_prepare, real_recover = self.m.prepare_units, self.m.recover
        def crash(*args, **kwargs):
            real_prepare(*args, **kwargs)
            if kwargs.get('write'):
                raise RuntimeError('power loss')
        self.m.prepare_units, self.m.recover = crash, lambda: None
        with self.assertRaisesRegex(RuntimeError, 'power loss'):
            self.install()
        self.assertTrue(self.m.JOURNAL.exists())
        self.m.prepare_units, self.m.recover = real_prepare, real_recover
        self.m.recover()
        self.assert_legacy()
        self.install()

    def test_crash_before_commit_preserves_previous_history(self):
        self.install()
        previous = (self.m.STATE / 'history/lamp-os.json').read_text()
        real_recover = self.m.recover
        self.m.recover = lambda: None
        self.fail_health.add(('lamp-os', '0.2.0'))
        with self.assertRaisesRegex(RuntimeError, 'health failure'):
            self.install(version='0.2.0', activate=False)
        self.m.recover = real_recover
        self.m.recover()
        self.assertEqual((self.m.STATE / 'history/lamp-os.json').read_text(), previous)
        self.m.rollback('lamp-os')
        self.assert_legacy()

    def test_committed_install_finishes_history_after_interruption(self):
        real_finalize, real_recover = self.m.finalize, self.m.recover
        self.m.finalize = lambda journal: (_ for _ in ()).throw(RuntimeError('disk error'))
        self.m.recover = lambda: None
        # Simulate power loss, not an exception successfully recovered in-process.
        self.install()
        self.assertEqual(json.loads(self.m.JOURNAL.read_text())['phase'], 'committed')
        self.m.finalize, self.m.recover = real_finalize, real_recover
        self.m.recover()
        self.assertEqual(self.m.load()['components']['lamp-os']['version'], '0.1.0')
        self.assertTrue((self.m.STATE / 'history/lamp-os.json').exists())
        self.assertFalse(self.m.JOURNAL.exists())

    def test_rollback_crash_after_unit_removal_restores_pre_rollback_service(self):
        self.install()
        real_commit, real_recover = self.m.commit, self.m.recover
        self.m.commit = lambda journal: (_ for _ in ()).throw(RuntimeError('power loss'))
        self.m.recover = lambda: None
        with self.assertRaisesRegex(RuntimeError, 'power loss'):
            self.m.rollback('lamp-os')
        self.assertFalse((self.m.UNITS / 'lamp-os.service').exists())
        self.m.commit, self.m.recover = real_commit, real_recover
        self.m.recover()
        self.assertTrue((self.m.UNITS / 'lamp-os.service').exists())
        self.assertEqual(self.states['lamp-os'], 'active')
        self.assertEqual(self.states['hal'], 'inactive')

    def test_bad_archive_and_repeated_failures_do_not_accumulate_stages(self):
        entry = dict(service='lamp-os', version='0.1.0', replaces=['hal'])
        for filename in ('../escape', '/absolute', 'bin\\escape'):
            with self.assertRaisesRegex(RuntimeError, 'Unsafe'):
                self.m.install('lamp-os', entry, str(self.package(extra=[(filename, 'bad')])), True)
        self.assertEqual(list((self.m.ROOT / 'lamp-os/releases').iterdir()), [])
        info = zipfile.ZipInfo('link')
        info.external_attr = (stat.S_IFLNK | 0o777) << 16
        with self.assertRaisesRegex(RuntimeError, 'Unsafe'):
            self.m.install('lamp-os', entry, str(self.package(extra=[(info, '/etc/passwd')])), True)
        self.assertEqual(self.events, [])

    def test_pruning_preserves_registered_and_rollback_snapshots(self):
        self.install()
        self.install(version='0.2.0')
        self.install(version='0.3.0')
        versions = sorted((directory / 'VERSION').read_text() for directory in (self.m.ROOT / 'lamp-os/releases').iterdir())
        # The activation checkpoint is retained independently of version rollback.
        self.assertEqual(versions, ['0.1.0', '0.2.0', '0.3.0'])
        self.m.rollback('lamp-os')
        self.assertEqual(self.m.load()['components']['lamp-os']['version'], '0.2.0')

    def test_full_updater_blocks_legacy_update_and_rollback_before_network(self):
        self.install()
        source = SCRIPT.read_text()
        for old, new in (
            ('/root/bootstrap/services', self.m.STATE),
            ('/opt/autonomous-services', self.m.ROOT),
            ('/var/lib/autonomous-services', self.m.DATA),
            ('/root/bootstrap/rollback', self.root / 'legacy-rollback'),
            ('/root/config/bootstrap.json', self.root / 'bootstrap.json'),
            ('/opt/autonomous-services', self.m.ROOT),
            ('/var/lib/autonomous-services', self.m.DATA),
            ('/etc/systemd/system', self.m.UNITS),
            ('/var/lock/software-update', self.root / 'lock'),
        ):
            source = source.replace(old, str(new))
        updater = self.root / 'software-update'
        updater.write_text(source)
        shell_env = self.root / 'shell-env'
        shell_env.write_text('id() { echo 0; }; flock() { return 0; }; curl() { echo NETWORK_SHOULD_NOT_RUN >&2; return 88; }; export -f id flock curl\n')
        for args in (('hal',), ('os-server',), ('device',), ('rollback', 'hal'), ('rollback', 'os-server')):
            with self.subTest(args=args):
                result = subprocess.run(['/bin/bash', str(updater), *args],
                    env={**os.environ, 'PATH': '/usr/bin:/bin:/usr/sbin:/sbin',
                         'BASH_ENV': str(shell_env), 'SOFTWARE_UPDATE_DETACHED': '1'}, capture_output=True, text=True, timeout=15)
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertIn('has been replaced', result.stderr)
                self.assertNotIn('NETWORK_SHOULD_NOT_RUN', result.stderr)

    def test_full_updater_recovers_independent_components_after_migration(self):
        self._full_recovery_after_migration('bootstrap', succeeds=True)

    def test_full_updater_recovers_web_after_migration(self):
        self._full_recovery_after_migration('web', succeeds=True)

    def test_full_updater_rejects_recovery_of_replaced_owners(self):
        for component in ('hal', 'os-server', 'device'):
            with self.subTest(component=component):
                self._full_recovery_after_migration(component, succeeds=False)

    def test_full_updater_rejects_forged_independent_recovery_paths(self):
        self._full_recovery_after_migration('bootstrap', succeeds=False, forged=True)

    def _full_recovery_after_migration(self, component, succeeds, forged=False):
        if not self.m.REGISTRY.exists():
            self.install()
        rollback = self.root / 'legacy-rollback'
        rollback.mkdir(exist_ok=True)
        binaries = self.root / 'bin'
        binaries.mkdir(exist_ok=True)
        live = {'bootstrap': binaries / 'bootstrap-server', 'web': self.root / 'web',
                'hal': self.root / 'hal', 'os-server': binaries / 'os-server',
                'device': self.root / 'device'}[component]
        backup = rollback / (component + '.previous')
        if component in ('web', 'hal', 'device'):
            live.mkdir(exist_ok=True)
            backup.mkdir(exist_ok=True)
            (live / 'index.html').write_text('candidate')
            (backup / 'index.html').write_text('previous')
        else:
            live.write_text('candidate')
            backup.write_text('previous')
        actions = {'bootstrap': 'restore_bootstrap_service', 'web': 'restore_web_service_state',
                   'hal': 'restore_hal_service_state', 'os-server': 'restore_os_server_service',
                   'device': 'restore_device_service_state'}
        journal = rollback / 'pending-update.json'
        journal.write_text(json.dumps(dict(live=str(self.root / 'forged' if forged else live),
                                           backup=str(backup), restore=actions[component], had_live=True)))
        source = SCRIPT.read_text()
        for old, new in (
            ('/root/bootstrap/services', self.m.STATE),
            ('/opt/autonomous-services', self.m.ROOT),
            ('/var/lib/autonomous-services', self.m.DATA),
            ('/root/bootstrap/rollback', rollback),
            ('/root/bootstrap/progress', self.root / 'progress'),
            ('/root/config/bootstrap.json', self.root / 'bootstrap.json'),
            ('/etc/systemd/system', self.m.UNITS),
            ('/var/lock/software-update', self.root / 'lock'),
            ('/usr/local/bin', binaries),
            ('/usr/share/nginx/html/setup', self.root / 'web'),
            ('/opt/hal', self.root / 'hal'),
        ):
            source = source.replace(old, str(new))
        updater = self.root / 'software-update'
        updater.write_text(source)
        events = self.root / 'recovery-events'
        events.write_text('')
        shell_env = self.root / 'shell-env'
        shell_env.write_text("""id() { echo 0; }
flock() { return 0; }
systemctl() { echo "$*" >> "$RECOVERY_EVENTS"; return 0; }
nginx() { return 0; }
curl() { case "${!#}" in http://127.0.0.1:8080/health|http://127.0.0.1/) return 0;; *) echo UNEXPECTED_NETWORK >&2; return 88;; esac; }
export -f id flock systemctl nginx curl
""")
        result = subprocess.run(['/bin/bash', str(updater), 'recover'],
            env={**os.environ, 'BASH_ENV': str(shell_env), 'SOFTWARE_UPDATE_DETACHED': '1',
                 'RECOVERY_EVENTS': str(events)}, capture_output=True, text=True, timeout=20)
        self.assertEqual(result.returncode, 0 if succeeds else 1, result.stdout + result.stderr)
        self.assertNotIn('UNEXPECTED_NETWORK', result.stderr)
        output = events.read_text()
        self.assertNotIn('hal', output)
        self.assertNotIn('os-server', output)
        self.assertEqual(journal.exists(), not succeeds)
        if succeeds:
            self.assertIn('restart ' + ('nginx' if component == 'web' else 'bootstrap'), output)
            self.assertEqual((live / 'index.html').read_text() if component == 'web' else live.read_text(), 'previous')
        else:
            self.assertEqual(output, '')
            self.assertTrue(backup.exists())
            if not forged:
                self.assertIn('Recovery would restart replaced component', result.stderr)
        self.assertEqual(self.m.status('hal'), 'blocked')
        self.assertEqual(self.m.status('os-server'), 'blocked')

    def test_health_probe_uses_same_environment_cwd_and_running_pid(self):
        self.install()
        entry = self.m.load()['components']['lamp-os']
        observed = []
        def probe(*args, **kwargs):
            observed.append((args, kwargs))
            return subprocess.CompletedProcess(args, 0,
                json.dumps(dict(ready=True, version='0.1.0', pid=100)), '')
        self.m.run = probe
        # Use the original helper function, not the fake health checker.
        original = types.ModuleType('original_health')
        exec(compile(SOURCE, str(SCRIPT), 'exec'), original.__dict__)
        original.run, original.service_state, original.DATA = probe, self.m.service_state, self.m.DATA
        original.healthy('lamp-os', entry)
        args, kwargs = observed[0]
        self.assertEqual(args, (str(Path(entry['release']) / 'bin/service'), 'health'))
        self.assertEqual(kwargs['cwd'], str(self.m.DATA / 'lamp-os/current'))
        self.assertEqual(kwargs['env']['AUTONOMOUS_SERVICE_NAME'], 'lamp-os')
        self.assertEqual(kwargs['env']['AUTONOMOUS_SERVICE_DATA'], kwargs['cwd'])
        self.assertEqual(kwargs['timeout'], 2)

    def test_committed_rollback_finalizes_after_interruption(self):
        self.install()
        real_finalize, real_recover = self.m.finalize, self.m.recover
        self.m.finalize = lambda journal: (_ for _ in ()).throw(RuntimeError('disk error'))
        self.m.recover = lambda: None
        self.m.rollback('lamp-os')
        self.assertEqual(json.loads(self.m.JOURNAL.read_text())['phase'], 'committed')
        self.m.finalize, self.m.recover = real_finalize, real_recover
        self.m.recover()
        self.assert_legacy()
        self.assertFalse((self.m.STATE / 'history/lamp-os.json').exists())

    def test_signed_service_dispatch_checks_signature_digest_and_bootstrap(self):
        self._service_dispatch_checks()

    def test_unsigned_service_dispatch_checks_digest_and_bootstrap(self):
        self._service_dispatch_checks(unsigned=True)

    def test_unsigned_installed_service_can_update_without_a_signing_key(self):
        self._service_dispatch_checks(unsigned=True, managed=True)

    def _service_dispatch_checks(self, unsigned=False, managed=False):
        import base64
        import hashlib
        archive = self.package()
        component = dict(service='lamp-os', replaces=['hal', 'os-server'], version='0.1.0',
                         url='test://package', sha256=hashlib.sha256(archive.read_bytes()).hexdigest())
        payload = json.dumps({'lamp-os': component}).encode()
        metadata = dict(signed=dict(format='autonomous-ota/v1', payload=base64.b64encode(payload).decode(),
                        signature=dict(algorithm='ed25519', value=base64.b64encode(b'x' * 64).decode())))
        if unsigned:
            metadata = {'lamp-os': component}
        (self.root / 'metadata.json').write_text(json.dumps(metadata))
        config = dict(metadata_url='test://metadata')
        if not unsigned:
            config['signing_public_key'] = base64.b64encode(b'x' * 32).decode()
        (self.root / 'bootstrap.json').write_text(json.dumps(config))
        if managed:
            self.install()
            self.assertEqual(self.m.status('lamp-os'), 'managed')
        source = SCRIPT.read_text()
        for old, new in (
            ('/root/bootstrap/services', self.m.STATE),
            ('/opt/autonomous-services', self.m.ROOT),
            ('/var/lib/autonomous-services', self.m.DATA),
            ('/root/bootstrap/rollback', self.root / 'legacy-rollback'),
            ('/root/bootstrap/progress', self.root / 'progress'),
            ('/root/config/bootstrap.json', self.root / 'bootstrap.json'),
            ('/var/lock/software-update', self.root / 'lock'),
            ('/opt/hal', self.root / 'hal'),
        ):
            source = source.replace(old, str(new))
        # Mock only the transaction boundary: other tests exercise its real state
        # machine. This test drives the full shell trust/dispatch path on macOS.
        source = source.replace('service_transaction install "$APP" "$METADATA_PAYLOAD" "$ZIP_TMP" "${2:-}"',
                                'test_service_install "$APP" "$METADATA_PAYLOAD" "$ZIP_TMP" "${2:-}"')
        updater = self.root / 'software-update'
        updater.write_text(source)
        driver = self.root / 'tools.py'
        driver.write_text(r'''import hashlib,json,os,pathlib,shutil,sys
root=pathlib.Path(os.environ['SERVICE_TEST_ROOT'])
command,args=sys.argv[1],sys.argv[2:]
if command=='curl':
 path=root/('metadata.json' if args[-1]=='test://metadata' else '0.1.0.zip')
 shutil.copyfile(path,args[args.index('-o')+1])
 if '-D' in args: pathlib.Path(args[args.index('-D')+1]).write_text('HTTP/1.1 200 OK\r\nContent-Length: '+str(path.stat().st_size)+'\r\n\r\n')
elif command=='jq':
 if '-n' in args: print('{}'); sys.exit(0)
 value=json.loads(pathlib.Path(args[-1]).read_text())
 expression=args[-2]
 if '.format ==' in expression:
  signed=value.get('signed',value)
  sys.exit(0 if signed.get('format')=='autonomous-ota/v1' and signed.get('signature',{}).get('algorithm')=='ed25519' else 1)
 if expression.startswith('(.signed // .).'):
  for key in expression.split(').',1)[1].split('.'): value=value.get('signed',value).get(key,'')
 elif '.[$a].' in expression:
  key=args[args.index('--arg')+2]
  value=value.get(key,{}).get(expression.split('].',1)[1].split()[0],'')
 else: value=value.get(expression.lstrip('.').split()[0],'')
 print(value)
elif command=='sha256sum':
 digest,path=sys.stdin.read().strip().split(None,1)
 sys.exit(0 if os.environ.get('BAD_DIGEST')!='1' and hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest()==digest else 1)
elif command=='install':
 (root/'installed.json').write_text(json.dumps(args))
''')
        env_file = self.root / 'shell-env'
        env_file.write_text('''id() { echo 0; }
flock() { return 0; }
curl() { /usr/bin/python3 "$SERVICE_TEST_ROOT/tools.py" curl "$@"; }
jq() { /usr/bin/python3 "$SERVICE_TEST_ROOT/tools.py" jq "$@"; }
sha256sum() { /usr/bin/python3 "$SERVICE_TEST_ROOT/tools.py" sha256sum "$@"; }
openssl() { echo verified > "$SERVICE_TEST_ROOT/verified"; [ "${BAD_SIGNATURE:-0}" != 1 ]; }
timeout() { shift; if [[ "$1" == */bootstrap-server ]]; then echo "${BOOTSTRAP_PROTOCOL:-1}"; else "$@"; fi; }
test_service_install() { /usr/bin/python3 "$SERVICE_TEST_ROOT/tools.py" install "$@"; }
export -f id flock curl jq sha256sum openssl timeout test_service_install
''')
        scenarios = [('BOOTSTRAP_PROTOCOL', '0', 1), ('BAD_DIGEST', '1', 1),
                     ('BOOTSTRAP_PROTOCOL', '1', 0)]
        if unsigned:
            scenarios += [('MISSING_SHA', '1', 1), ('INVALID_SHA', '1', 1), ('MISSING_URL', '1', 1)]
        else:
            scenarios += [('BAD_SIGNATURE', '1', 1), ('UNSIGNED_FEED', '1', 1)]
        for flag, value, expected in scenarios:
            with self.subTest(flag=flag, value=value, unsigned=unsigned, managed=managed):
                supplied = copy.deepcopy(metadata)
                if flag == 'UNSIGNED_FEED':
                    supplied = {'lamp-os': component}
                elif flag == 'MISSING_SHA':
                    supplied['lamp-os'].pop('sha256')
                elif flag == 'INVALID_SHA':
                    supplied['lamp-os']['sha256'] = 'g' * 64
                elif flag == 'MISSING_URL':
                    supplied['lamp-os'].pop('url')
                (self.root / 'metadata.json').write_text(json.dumps(supplied))
                (self.root / 'installed.json').unlink(missing_ok=True)
                (self.root / 'verified').unlink(missing_ok=True)
                args = ['/bin/bash', str(updater), 'lamp-os'] + ([] if managed else ['--activate'])
                result = subprocess.run(args,
                    env={**os.environ, 'PATH':'/usr/bin:/bin:/usr/sbin:/sbin', 'BASH_ENV':str(env_file),
                         'SERVICE_TEST_ROOT':str(self.root), 'SOFTWARE_UPDATE_DETACHED':'1', flag:value},
                    capture_output=True, text=True, timeout=20)
                self.assertEqual(result.returncode, expected, result.stdout + result.stderr)
                self.assertEqual((self.root/'installed.json').exists(), expected==0)
                if expected==0:
                    self.assertEqual(json.loads((self.root/'installed.json').read_text())[::3], ['lamp-os', '' if managed else '--activate'])
                    self.assertEqual((self.root/'verified').exists(), not unsigned)

    def test_corrupt_registry_fails_closed(self):
        self.m.STATE.mkdir()
        self.m.REGISTRY.write_text('{broken')
        with self.assertRaises(ValueError):
            self.m.status('hal')
        self.assertEqual(self.events, [])


if __name__ == '__main__':
    unittest.main()
