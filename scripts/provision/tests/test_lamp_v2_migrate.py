"""V1 -> V2 migration preparation against a synthetic /dev, /sys and /proc tree."""
from importlib.machinery import SourceFileLoader
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import unittest
import zipfile

SCRIPT = Path(__file__).resolve().parents[1] / 'lamp-v2-migrate'
LOADER = SourceFileLoader('lamp_v2_migrate', str(SCRIPT))
SPEC = importlib.util.spec_from_loader('lamp_v2_migrate', LOADER)
migrate = importlib.util.module_from_spec(SPEC)
LOADER.exec_module(migrate)

LAMP_ENV = ('# lamp-a63e, read at rest with torque off by lamp-body-measure.\n'
            'BODY_POSES="--home 1768,2060,2036 --folded-arm 2048,2046 --stand 1772,2060,2036,2657,2602 '
            '--pitch-window 1740,2030 --torque on"\n')


class LampTree(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        by_id = self.root / 'dev/serial/by-id'
        by_id.mkdir(parents=True)
        (by_id / 'usb-1a86_USB_Single_Serial_5CE4142000-if00').symlink_to('../../ttyACM0')
        usb = self.root / 'sys/devices/platform/usb3/3-1/3-1.3/3-1.3.4'
        (usb / '3-1.3.4:1.0').mkdir(parents=True)
        (usb / 'idVendor').write_text('01da\n')
        (usb / 'idProduct').write_text('5875\n')
        for n in (0, 1):
            video = self.root / f'sys/class/video4linux/video{n}'
            video.mkdir(parents=True)
            (video / 'index').write_text(f'{n}\n')
            (video / 'device').symlink_to(usb / '3-1.3.4:1.0')
        cards = self.root / 'proc/asound/cards'
        cards.parent.mkdir(parents=True)
        cards.write_text(' 0 [allwinnerhdmi  ]: allwinner-hdmi - allwinner-hdmi\n'
                         ' 4 [device_micro2  ]: USB-Audio - USB Composite Device\n')
        for n in range(8):
            freq = self.root / f'sys/devices/system/cpu/cpu{n}/cpufreq/cpuinfo_max_freq'
            freq.parent.mkdir(parents=True)
            freq.write_text('2002000\n' if n >= 6 else '1794000\n')
        for n, kind in enumerate(('cpul_thermal_zone', 'cpub_thermal_zone', 'gpu_thermal_zone', 'skin_zone')):
            zone = self.root / f'sys/class/thermal/thermal_zone{n}'
            zone.mkdir(parents=True)
            (zone / 'type').write_text(kind + '\n')

    def test_detects_this_lamps_hardware(self):
        self.assertEqual(migrate.servo_device(self.root), '/dev/ttyACM0')
        self.assertEqual(migrate.camera(self.root), ('/dev/video0', '01da:5875@3-1.3.4'))
        self.assertEqual(migrate.microphone(self.root), 'mic2')
        self.assertEqual(migrate.mixer(self.root, lambda card, control: True), ('allwinnerhdmi', 'Speaker'))
        self.assertEqual(migrate.fast_cpus(self.root), '6 7')
        self.assertEqual(migrate.judge_zones(self.root), '--judge-zone cpu --judge-zone skin')

    def test_refuses_what_it_cannot_tell_apart(self):
        (self.root / 'dev/serial/by-id/usb-1a86_second').symlink_to('../../ttyACM1')
        with self.assertRaisesRegex(migrate.Refused, 'one servo adapter'):
            migrate.servo_device(self.root)
        (self.root / 'proc/asound/cards').write_text(' 0 [allwinnerhdmi  ]: x\n')
        with self.assertRaisesRegex(migrate.Refused, 'microphone'):
            migrate.microphone(self.root)
        with self.assertRaisesRegex(migrate.Refused, 'Speaker'):
            migrate.mixer(self.root, lambda card, control: False)

    def test_profile_carries_measured_poses_and_model_gaze(self):
        poses = migrate.body_poses(LAMP_ENV)
        self.assertTrue(poses.startswith('--home 1768,2060,2036 '))
        self.assertTrue(poses.endswith(f' --gaze {migrate.MODEL_GAZE}'))
        self.assertEqual(migrate.body_poses(LAMP_ENV.replace(' --torque on', ' --torque on --gaze 700,500')).count('--gaze'), 1)
        hardware = {'servo': '/dev/ttyACM0', 'camera': ('/dev/video0', '01da:5875@3-1.3.4'), 'microphone': 'mic2',
                    'mixer': ('allwinnerhdmi', 'Speaker'), 'fast_cpus': '6 7', 'judge_zones': '--judge-zone cpu'}
        profile = migrate.profile('lamp-a63e', hardware, poses)
        subs = profile['substitutions']
        self.assertEqual((profile['unit_id'], subs['UNIT_ID']), ('lamp-a63e', 'lamp-a63e'))
        self.assertIn('--microphone mic2', subs['VOICE_OPTIONS'])
        self.assertEqual(subs['BODY_POSES'], poses)
        self.assertEqual(subs['HEALTH_EXPECT'].count('--expect'), len(migrate.PARTS))
        for value in subs.values():
            self.assertFalse(any(c in value for c in '%$\\"\'\n'), value)

    def test_poses_without_home_or_with_unit_syntax_are_refused(self):
        for text in ('', 'BODY_POSES="--stand 1,2,3,4,5"\n', 'BODY_POSES="--home 1,2,3 $(x)"\n'):
            with self.subTest(text=text), self.assertRaises(migrate.Refused):
                migrate.body_poses(text)

    def test_generic_controls_pins_drop_touch_only_lines(self):
        pins = json.loads(migrate.generic_pins('{"chip": "/dev/gpiochip0", "every_ms": 5, '
                                               '"reset": {"line": 100}, "legacy_head": true}'))
        self.assertEqual(pins, {'chip': '/dev/gpiochip0', 'every_ms': 5})

    def test_existing_different_setting_needs_force(self):
        path = self.root / 'provider.json'
        path.write_text('{"mine": 1}\n')
        written = []
        with self.assertRaisesRegex(migrate.Refused, '--force'):
            migrate.write(str(path), '{"new": 1}\n', 0o600, False, False, written)
        migrate.write(str(path), '{"mine": 1}\n', 0o600, False, False, written)
        migrate.write(str(path), '{"new": 1}\n', 0o600, True, False, written)
        self.assertEqual(path.read_text(), '{"new": 1}\n')
        self.assertEqual(os.stat(path).st_mode & 0o777, 0o600)
        migrate.write(str(self.root / 'dry.json'), '{}', 0o644, False, True, written)
        self.assertFalse((self.root / 'dry.json').exists())

    def test_package_without_migration_contents_is_refused(self):
        archive = self.root / 'old.zip'
        with zipfile.ZipFile(archive, 'w') as z:
            z.writestr('SERVICE_PREFLIGHT', '1\n')
        with self.assertRaisesRegex(migrate.Refused, 'cannot migrate a V1 lamp'):
            migrate.unpack(archive, self.root / 'out')
        with zipfile.ZipFile(archive, 'a') as z:
            z.writestr('tools/lamp-body-measure', b'\x7fELF')
            for name in ('voice-provider.json', 'controls.json', 'controls-pins.json'):
                z.writestr('provision/' + name, '{}')
        package = migrate.unpack(archive, self.root / 'out')
        self.assertEqual(os.stat(package / 'tools/lamp-body-measure').st_mode & 0o777, 0o755)


if __name__ == '__main__':
    unittest.main()
