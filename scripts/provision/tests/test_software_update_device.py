"""Run the real device-profile updater against an isolated HAL and profile tree."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
import zipfile

SCRIPT = Path(__file__).resolve().parents[1] / "software-update"

DRIVER = '''#!/usr/bin/env python3
import os, pathlib, shutil, sys
name = pathlib.Path(sys.argv[0]).name
root = pathlib.Path(os.environ["TEST_ROOT"])
a = sys.argv[1:]
if name == "id": print("0")
elif name == "systemctl":
    with (root / "events").open("a") as f: f.write(" ".join(a) + "\\n")
elif name == "curl" and "-o" in a:
    shutil.copyfile(root / a[-1].split("/")[-1], a[a.index("-o") + 1])
'''


class DeviceProfileHALGateTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.hal = self.root / "hal"
        self.profile = self.root / "devices" / "intern-v2"
        self.rollback = self.root / "rollback"
        for path in (self.hal, self.profile, self.rollback, self.root / "bin"):
            path.mkdir(parents=True)
        (self.hal / ".env").write_text(f"DEVICE_TYPE=intern-v2\nDEVICES_DIR={self.root / 'devices'}\n")
        (self.profile / "ROBOT.md").write_text("installed profile")
        (self.root / "bootstrap.json").write_text("{}")
        # The Harness nginx route is already present, so that pre-step changes nothing.
        (self.root / "nginx" / "conf.d").mkdir(parents=True)
        (self.root / "nginx" / "conf.d" / "default.conf").write_text("location = /api/harness/ws {}\n")
        (self.root / "metadata.json").write_text(json.dumps(
            {"devices": {"intern-v2": {"version": "0.0.13", "url": "test://device.zip"}}}))
        source = SCRIPT.read_text()
        for original, replacement in (
            ("/root/config/bootstrap.json", self.root / "bootstrap.json"),
            ("/root/config/config.json", self.root / "config.json"),
            ("/root/bootstrap/rollback", self.rollback),
            ("/root/bootstrap/progress", self.root / "progress"),
            ("/root/bootstrap/services", self.root / "services"),
            ("/var/lock/software-update", self.root / "software-update"),
            ("/usr/local/bin", self.root / "bin"),
            ("/opt/hal", self.hal),
            ("/etc/nginx", self.root / "nginx"),
        ):
            source = source.replace(original, str(replacement))
        self.updater = self.root / "software-update"
        self.updater.write_text(source)
        self.fake_bin = self.root / "fake-bin"
        self.fake_bin.mkdir()
        driver = self.fake_bin / "driver"
        driver.write_text(DRIVER)
        driver.chmod(0o755)
        for name in ("id", "systemctl", "curl", "sync", "sleep", "flock", "nginx"):
            (self.fake_bin / name).symlink_to(driver)

    def run_update(self, hal_version, legacy_profile=False):
        if hal_version is None:
            (self.hal / "VERSION_HAL").unlink(missing_ok=True)
        else:
            (self.hal / "VERSION_HAL").write_text(hal_version)
        with zipfile.ZipFile(self.root / "device.zip", "w") as archive:
            archive.writestr("ROBOT.md", "new profile")
            if legacy_profile:
                archive.writestr("DEVICE.md", "new profile")
        return subprocess.run(
            ["/bin/bash", str(self.updater), "device"],
            env={**os.environ, "PATH": str(self.fake_bin) + os.pathsep + os.environ["PATH"],
                 "TEST_ROOT": str(self.root), "SOFTWARE_UPDATE_DETACHED": "1",
                 "OTA_METADATA_URL": "test://metadata.json"},
            capture_output=True, text=True, timeout=60,
        )

    def events(self):
        path = self.root / "events"
        return path.read_text().splitlines() if path.exists() else []

    def assert_refused_untouched(self, result, reason):
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn(reason, result.stderr)
        self.assertIn("profile unchanged", result.stderr)
        # Refused before save/stop: no service was touched and nothing awaits recovery.
        self.assertEqual([e for e in self.events() if e.split()[0] in ("stop", "restart")], [])
        self.assertEqual((self.profile / "ROBOT.md").read_text(), "installed profile")
        self.assertFalse((self.rollback / "pending-update.json").exists())
        self.assertFalse((self.rollback / "device.previous").exists())
        self.assertEqual(list((self.root / "devices").glob(".intern-v2.new.*")), [])

    def test_robot_md_profile_is_refused_on_hal_that_reads_only_device_md(self):
        result = self.run_update("0.0.867")
        self.assert_refused_untouched(result, "needs HAL >= 0.1.2 (ROBOT.md); installed HAL 0.0.867")

    def test_unreadable_hal_version_is_refused(self):
        result = self.run_update(None)
        self.assert_refused_untouched(result, "installed HAL version is unreadable")

    def test_current_hal_installs_the_profile(self):
        for version in ("0.1.2", "0.1.302"):
            with self.subTest(version=version):
                (self.profile / "ROBOT.md").write_text("installed profile")
                (self.root / "events").unlink(missing_ok=True)
                result = self.run_update(version)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertIn("device profile (intern-v2) updated to 0.0.13", result.stdout)
                self.assertEqual((self.profile / "ROBOT.md").read_text(), "new profile")
                self.assertIn("stop hal", self.events())

    def test_profile_that_still_ships_device_md_installs_on_old_hal(self):
        result = self.run_update("0.0.867", legacy_profile=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual((self.profile / "ROBOT.md").read_text(), "new profile")


if __name__ == "__main__":
    unittest.main()
