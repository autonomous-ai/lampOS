"""Fault injection for the real updater helpers, isolated from host services/files."""
import os
import json
import zipfile
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "software-update"


class RecoveryTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.hal = self.root / "hal"
        self.web = self.root / "web"
        self.backup = self.root / "rollback"
        for path in (self.hal, self.web, self.backup, self.root / "bin"):
            path.mkdir()
        (self.hal / "VERSION_HAL").write_text("old")
        (self.hal / ".env").write_text("keep-me")
        (self.web / "index.html").write_text("old")
        (self.hal / "ROBOT.md").write_text("old profile")
        (self.backup / "hal.service-state").write_text("active")
        (self.backup / "web.nginx-state").write_text("active")
        (self.backup / "device.services").write_text("hal=active\nos-server=active\n")
        (self.root / "bootstrap.json").write_text('{"httpPort":8080}')
        self.full_source = SCRIPT.read_text()
        for original, replacement in (
            ("/root/config/bootstrap.json", self.root / "bootstrap.json"),
            ("/root/bootstrap/rollback", self.backup),
            ("/root/bootstrap/progress", self.root / "progress"),
            ("/usr/share/nginx/html/setup", self.web),
            ("/usr/local/bin", self.root / "bin"),
            ("/opt/hal", self.hal),
        ):
            self.full_source = self.full_source.replace(original, str(replacement))
        self.source = self.full_source.split('[ "$(id -u)"', 1)[0]
        self.mocks = r'''
sync() { :; }
nginx() { return 0; }
systemctl() {
  printf '%s\n' "$*" >> "$TEST_ROOT/events"
  case "$1" in
    is-enabled) return 0 ;;
    is-active) [ "$3" != "${DEAD_UNIT:-}" ]; return $? ;;
  esac
  return 0
}
wait_for_web() { [ "${BAD_HEALTH:-0}" = 0 ]; }
wait_for_url() {
  [ "${BAD_HEALTH:-0}" = 0 ] || return 1
  if [ -f "$HAL_DIR/VERSION_HAL" ] && [ "$(cat "$HAL_DIR/VERSION_HAL")" = broken ]; then return 1; fi
  if [ -n "${CORE_BINARY:-}" ] && grep -q BROKEN "$CORE_BINARY"; then return 1; fi
  return 0
}
# GNU install -D/timeout are unavailable on macOS; preserve their semantics here.
install() {
  [ "$1" != -D ] || shift
  local mode="$2" source="$3" dest="$4"
  mkdir -p "$(dirname "$dest")" && cp "$source" "$dest" && chmod "$mode" "$dest"
}
timeout() { shift; "$@"; }
'''

    def run_shell(self, body, **env):
        return subprocess.run(
            ["/bin/bash", "-c", self.source + self.mocks + "\n" + body],
            env={**os.environ, "TEST_ROOT": str(self.root), **env},
            capture_output=True, text=True, timeout=15,
        )

    def test_full_hal_updater_rejects_bad_candidate_and_preserves_old_env(self):
        for failure in ("startup", "sync", "none"):
            with self.subTest(failure=failure):
                (self.hal / "VERSION_HAL").write_text("old")
                package = self.root / "hal.zip"
                with zipfile.ZipFile(package, "w") as archive:
                    archive.writestr("VERSION_HAL", "broken" if failure == "startup" else "new")
                    archive.writestr("server.py", "# candidate")
                (self.root / "metadata.json").write_text(json.dumps({"hal": {"version": "new", "url": "test://hal.zip"}}))
                fake_bin = self.root / "fake-bin"
                fake_bin.mkdir(exist_ok=True)
                driver = fake_bin / "driver"
                driver.write_text('''#!/usr/bin/env python3
import os, pathlib, shutil, sys
name=pathlib.Path(sys.argv[0]).name
root=pathlib.Path(os.environ["TEST_ROOT"])
a=sys.argv[1:]
if name == "id": print("0")
elif name == "uv" and a == ["--version"]: print("uv 0.12.23")
elif name == "uv": sys.exit(1 if os.environ["FAILURE"] == "sync" else 0)
elif name == "systemctl":
 with (root/"full-events").open("a") as f: f.write(" ".join(a)+"\\n")
elif name == "curl":
 if "-o" in a: shutil.copyfile(root/a[-1].split("/")[-1], a[a.index("-o")+1])
 else:
  v=root/"hal/VERSION_HAL"
  sys.exit(0 if v.exists() and v.read_text() != "broken" else 22)
''')
                driver.chmod(0o755)
                for name in ("id", "uv", "systemctl", "curl", "sync", "sleep", "flock"):
                    link = fake_bin / name
                    if not link.exists(): link.symlink_to(driver)
                updater = self.root / "software-update"
                updater.write_text(self.full_source.replace("/var/lock/software-update", str(self.root / "software-update")))
                result = subprocess.run(
                    ["/bin/bash", str(updater), "hal"],
                    env={**os.environ, "PATH": str(fake_bin)+os.pathsep+os.environ["PATH"],
                         "TEST_ROOT": str(self.root), "SOFTWARE_UPDATE_DETACHED": "1",
                         "OTA_METADATA_URL": "test://metadata.json", "FAILURE": failure},
                    capture_output=True, text=True, timeout=30,
                )
                self.assertEqual(result.returncode, 0 if failure == "none" else 1, result.stdout+result.stderr)
                progress = json.loads((self.root / "progress/hal.json").read_text())
                self.assertEqual(progress["phase"], "completed" if failure == "none" else "failed")
                if failure == "startup":
                    self.assertIn("HAL health check failed", result.stderr)
                elif failure == "sync":
                    self.assertIn("HAL staging failed", result.stderr)
                else:
                    self.assertIn("hal updated to new", result.stdout)
                self.assertEqual((self.hal / "VERSION_HAL").read_text(), "new" if failure == "none" else "old")
                self.assertEqual((self.hal / ".env").read_text(), "keep-me")
                self.assertFalse((self.backup / "pending-update.json").exists())
                self.assertEqual(list(self.root.glob(".hal.new.*")), [])

    HAL_PROBES = r'''
free() { printf '%s\n' "total used free shared buff/cache available" "Mem: 3900 3700 50 10 150 120" "Swap: 1000 900 100"; }
df() { printf '%s\n' "Filesystem Size Used Avail Use% Mounted" "/dev/root 29G 28G 200M 99% /"; }
systemctl() {
  if [ "$1" = show ]; then
    printf '%s\n' LoadState=loaded UnitFileState=enabled ActiveState=inactive SubState=dead Result=success \
      ExecMainStatus=0 NRestarts=0 ConditionResult=no "ActiveEnterTimestamp=Fri 2026-10-30 10:00:00 UTC" \
      "InactiveEnterTimestamp=Sat 2026-10-31 00:06:00 UTC"
    return 0
  fi
  printf '%s\n' "$*" >> "$TEST_ROOT/events"
  case "$1" in
    is-enabled) return 0 ;;
    is-active) [ "$3" != "${DEAD_UNIT:-}" ]; return $? ;;
  esac
  return 0
}
journalctl() {
  if [ "$1" = -k ]; then echo "Out of memory: Killed process 42 (python)"; return 0; fi
  printf '%s\n' "user said: I failed my exam" "ERROR user asked about the error in my code" \
    "WARNING heard: please kill the fatal exception" "ValueError: bad value from user text" \
    "INFO hal started" "Stopped HAL Hardware Runtime. and the user said hello" \
    "hal.service: I heard call mum and then something private" \
    "ImportError: cannot import name my_secret from private_thing" \
    "ModuleNotFoundError: No module named 'x'; the user said hello" \
    "Traceback (most recent call last):" \
    '  File "/opt/hal/server.py", line 10, in main' "    secret = user_text" \
    "ModuleNotFoundError: No module named 'torch'" \
    "hal.service: Main process exited, code=killed, status=9/KILL" \
    "Stopped HAL Hardware Runtime." "hal.service: Deactivated successfully." \
    "Condition check resulted in HAL Hardware Runtime being skipped."
}
'''

    def test_failed_hal_health_check_reports_why(self):
        result = self.run_shell(self.HAL_PROBES + 'if check_hal; then exit 0; else exit 1; fi', BAD_HEALTH="1")
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        for expected in ("HAL diagnostics", "total=3900M avail=120M swap_used=900M", "use=99%",
                         'File "/opt/hal/server.py", line 10, in main',
                         "ModuleNotFoundError: No module named 'torch'",
                         "hal.service: Main process exited", "Out of memory",
                         "UnitFileState=enabled", "ConditionResult=no", "InactiveEnterTimestamp=Sat 2026-10-31",
                         "Stopped HAL Hardware Runtime.", "hal.service: Deactivated successfully.",
                         "Condition check resulted in HAL Hardware Runtime being skipped."):
            self.assertIn(expected, result.stderr)
        # A line that only imitates a known prefix is dropped or cut to its type name.
        # HAL logs what users say; no free-text line may reach the report, even one
        # containing an error-looking word, and exception messages are dropped.
        for leaked in ("failed my exam", "asked about", "kill the fatal", "bad value from user text",
                       "user_text", "INFO hal started", "call mum", "my_secret",
                       "the user said hello", "and the user"):
            self.assertNotIn(leaked, result.stderr)

    def test_healthy_hal_prints_no_diagnostics(self):
        result = self.run_shell(self.HAL_PROBES + 'if check_hal; then exit 0; else exit 1; fi')
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertNotIn("HAL diagnostics", result.stderr)

    def test_hal_diagnostics_stay_short_for_noisy_journals(self):
        noisy = self.HAL_PROBES + r'''
journalctl() { for i in $(seq 1 400); do printf '  File "/%0500d", line 1, in f\n' "$i"; printf "hal.service: Failed with result 'exit-code'.\n"; done; }
if check_hal; then exit 0; else exit 1; fi
'''
        result = self.run_shell(noisy, BAD_HEALTH="1")
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertLess(len(result.stderr.encode()), 1500, result.stderr)

    def test_device_rejects_failed_hal_even_when_os_server_is_healthy(self):
        result = self.run_shell('DEVICE_DEST="$HAL_DIR"\nif check_device_profile; then exit 0; else exit 1; fi', DEAD_UNIT="hal")
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)

    def test_web_rejects_dead_nginx(self):
        result = self.run_shell('if check_web; then exit 0; else exit 1; fi', DEAD_UNIT="nginx")
        self.assertEqual(result.returncode, 1)

    def test_intentionally_disabled_web_can_skip_probe(self):
        result = self.run_shell('web_was_active() { return 1; }\nif check_web; then exit 0; else exit 1; fi', DEAD_UNIT="nginx", BAD_HEALTH="1")
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_crash_at_each_publication_boundary_restores_original(self):
        for phase in ("prepared", "moved", "published"):
            with self.subTest(phase=phase):
                result = self.run_shell(r'''
mark_publish_pending "$HAL_DIR" "$ROLLBACK_DIR/hal.previous" restore_hal_service_state
if [ "$PHASE" != prepared ]; then mv "$HAL_DIR" "$ROLLBACK_DIR/hal.previous"; fi
if [ "$PHASE" = published ]; then mkdir "$HAL_DIR"; echo broken > "$HAL_DIR/VERSION_HAL"; fi
kill -KILL $$
''', PHASE=phase)
                self.assertEqual(result.returncode, -9)
                self.assertTrue((self.backup / "pending-update.json").is_file())
                restored = self.run_shell('recover_pending_update')
                self.assertEqual(restored.returncode, 0, restored.stdout + restored.stderr)
                self.assertEqual((self.hal / "VERSION_HAL").read_text(), "old")
                self.assertEqual((self.hal / ".env").read_text(), "keep-me")
                self.assertFalse((self.backup / "pending-update.json").exists())

    HAL_BLOCK = r"""
HAL_BLOCK_SERVICES="$TEST_ROOT/services"
HAL_BLOCK_MARKER="$HAL_BLOCK_SERVICES/blocked/hal"
HAL_BLOCK_DROPIN="$TEST_ROOT/units/hal.service.d/99-autonomous-ownership.conf"
mkdir -p "${HAL_BLOCK_MARKER%/*}" "${HAL_BLOCK_DROPIN%/*}"
echo 1 > "$HAL_BLOCK_MARKER"
printf '[Unit]\nConditionPathExists=!%s\n[Service]\nKillMode=control-group\nTimeoutStopSec=20\n' "$HAL_BLOCK_MARKER" > "$HAL_BLOCK_DROPIN"
"""

    def block_left(self):
        return ((self.root / "services/blocked/hal").exists(),
                (self.root / "units/hal.service.d/99-autonomous-ownership.conf").exists())

    def test_stale_hal_block_without_owner_is_cleared(self):
        result = self.run_shell(self.HAL_BLOCK + 'clear_stale_hal_block')
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(self.block_left(), (False, False))
        self.assertFalse((self.root / "units/hal.service.d").exists())
        self.assertIn("daemon-reload", (self.root / "events").read_text())

    def test_stale_hal_block_cleared_when_registry_has_no_owner(self):
        result = self.run_shell(self.HAL_BLOCK + 'echo "{}" > "$HAL_BLOCK_SERVICES/ownership.json"\n'
                                'service_transaction() { [ "$1 $2" = "status hal" ] && echo legacy; }\nclear_stale_hal_block')
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(self.block_left(), (False, False))

    def test_owned_or_changing_hal_block_is_kept(self):
        for case, setup in (
            ("owned", 'echo "{}" > "$HAL_BLOCK_SERVICES/ownership.json"\nservice_transaction() { echo blocked; }'),
            ("registry unreadable", 'echo "{}" > "$HAL_BLOCK_SERVICES/ownership.json"\nservice_transaction() { return 1; }'),
            ("transition", 'echo "{}" > "$HAL_BLOCK_SERVICES/transition.json"'),
            ("foreign dropin", 'echo "[Unit]" > "$HAL_BLOCK_DROPIN"'),
        ):
            with self.subTest(case=case):
                result = self.run_shell(self.HAL_BLOCK + setup + '\nclear_stale_hal_block')
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertEqual(self.block_left(), (True, True))
                for name in ("ownership.json", "transition.json"):
                    (self.root / "services" / name).unlink(missing_ok=True)

    def test_recovery_clears_stale_hal_block_and_finishes(self):
        result = self.run_shell(self.HAL_BLOCK + """
wait_for_url() { [ ! -e "$HAL_BLOCK_MARKER" ]; }
mark_publish_pending "$HAL_DIR" "$ROLLBACK_DIR/hal.previous" restore_hal_service_state
mv "$HAL_DIR" "$ROLLBACK_DIR/hal.previous"
mkdir "$HAL_DIR"; echo broken > "$HAL_DIR/VERSION_HAL"
recover_pending_update
""")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("clearing a HAL ownership block", result.stdout)
        self.assertEqual(self.block_left(), (False, False))
        self.assertEqual((self.hal / "VERSION_HAL").read_text(), "old")
        self.assertFalse((self.backup / "pending-update.json").exists())

    def test_hal_skip_message_names_the_condition(self):
        result = self.run_shell(self.HAL_PROBES + """
journalctl() {
  [ "$1" = -k ] && return 0
  printf '%s\\n' "hal.service - HAL Hardware Runtime was skipped because of an unmet condition check (ConditionPathExists=!/root/bootstrap/services/blocked/hal)." \\
    "hal.service - HAL Hardware Runtime was skipped because of an unmet condition check (ConditionPathExists=!/x) and the user said hi."
}
if check_hal; then exit 0; else exit 1; fi""", BAD_HEALTH="1")
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("(ConditionPathExists=!/root/bootstrap/services/blocked/hal).", result.stderr)
        self.assertNotIn("user said", result.stderr)

    def test_postpublish_error_runs_rollback_in_exit_trap(self):
        result = self.run_shell(r'''
trap on_exit EXIT
mark_publish_pending "$HAL_DIR" "$ROLLBACK_DIR/hal.previous" restore_hal_service_state
mv "$HAL_DIR" "$ROLLBACK_DIR/hal.previous"
mkdir "$HAL_DIR"; echo broken > "$HAL_DIR/VERSION_HAL"
false
''')
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertEqual((self.hal / "VERSION_HAL").read_text(), "old")
        self.assertFalse((self.backup / "pending-update.json").exists())
        self.assertEqual((self.root / "events").read_text().splitlines().count("restart hal"), 1)

    def test_committed_health_does_not_restart_again_on_exit(self):
        result = self.run_shell(r'''
trap on_exit EXIT
mark_publish_pending "$HAL_DIR" "$ROLLBACK_DIR/hal.previous" restore_hal_service_state
mv "$HAL_DIR" "$ROLLBACK_DIR/hal.previous"
mkdir "$HAL_DIR"; echo healthy > "$HAL_DIR/VERSION_HAL"
restore_hal_service_state
check_hal
clear_publish_pending
disarm_service_restore
''')
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertFalse((self.backup / "pending-update.json").exists())
        self.assertEqual((self.root / "events").read_text().splitlines().count("restart hal"), 1)

    def test_recovery_retry_does_not_discard_restored_runtime(self):
        result = self.run_shell(r'''
mark_publish_pending "$HAL_DIR" "$ROLLBACK_DIR/hal.previous" restore_hal_service_state
mv "$HAL_DIR" "$ROLLBACK_DIR/hal.previous"
mkdir "$HAL_DIR"; echo broken > "$HAL_DIR/VERSION_HAL"
recover_pending_update
''', BAD_HEALTH="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual((self.hal / "VERSION_HAL").read_text(), "old")
        self.assertTrue((self.backup / "pending-update.json").exists())
        retried = self.run_shell('recover_pending_update')
        self.assertEqual(retried.returncode, 0, retried.stderr)
        self.assertFalse((self.backup / "pending-update.json").exists())

    def test_legacy_missing_live_tree_preserves_sole_backup(self):
        self.hal.rename(self.backup / "hal.previous")
        result = self.run_shell('recover_pending_update')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.hal / "VERSION_HAL").read_text(), "old")

    def test_failed_first_install_does_not_leave_installed_version(self):
        result = self.run_shell(r'''
rm -rf "$HAL_DIR"
trap on_exit EXIT
mark_publish_pending "$HAL_DIR" "$ROLLBACK_DIR/hal.previous" restore_hal_service_state
mkdir "$HAL_DIR"; echo broken > "$HAL_DIR/VERSION_HAL"
exit 1
''')
        self.assertEqual(result.returncode, 1)
        self.assertFalse(self.hal.exists())
        self.assertTrue((self.backup / "hal.failed" / "VERSION_HAL").exists())
        self.assertFalse((self.backup / "pending-update.json").exists())

    def test_device_restart_failure_is_not_masked_by_hal_restart(self):
        result = self.run_shell(r'''
systemctl() { [ "$1 $2" != "restart os-server" ]; }
if restore_device_service_state; then exit 0; else exit 1; fi
''')
        self.assertEqual(result.returncode, 1)

    def test_core_binary_failed_start_rolls_back_and_success_commits(self):
        for unit, restore, check in (("os-server", "restore_os_server_service", "check_os_server"),
                                     ("bootstrap", "restore_bootstrap_service", "check_bootstrap")):
            for broken in (True, False):
                with self.subTest(unit=unit, broken=broken):
                    dest = self.root / "bin" / ("bootstrap-server" if unit == "bootstrap" else unit)
                    dest.write_text("#!/bin/sh\necho old\n")
                    dest.chmod(0o755)
                    candidate = self.root / "candidate"
                    candidate.write_text("#!/bin/sh\n# " + ("BROKEN" if broken else "HEALTHY") + "\necho new\n")
                    result = self.run_shell(f'trap on_exit EXIT\nupdate_core_binary {unit} "{dest}" "{candidate}" {restore} {check}', CORE_BINARY=str(dest))
                    self.assertEqual(result.returncode, 1 if broken else 0, result.stdout + result.stderr)
                    self.assertIn("echo old" if broken else "HEALTHY", dest.read_text())
                    self.assertFalse((self.backup / "pending-update.json").exists())

    def test_device_rootfs_copy_failure_retains_recovery_journal(self):
        result = self.run_shell(r'''
DEVICE_DEST="$HAL_DIR"
mkdir -p "$ROLLBACK_DIR/device.previous.rootfs/files"
printf 'tmp/first\tpresent\ntmp/second\tpresent\n' > "$ROLLBACK_DIR/device.previous.rootfs/manifest"
mark_publish_pending "$DEVICE_DEST" "$ROLLBACK_DIR/device.previous" restore_device_service_state
mv "$DEVICE_DEST" "$ROLLBACK_DIR/device.previous"
mkdir "$DEVICE_DEST"; echo broken > "$DEVICE_DEST/ROBOT.md"
# Do not touch host /tmp: fail the first restore copy, permit any later call.
mkdir() { return 0; }
cp() { [ "$2" != "$ROLLBACK_DIR/device.previous.rootfs/files/tmp/first" ]; }
if recover_pending_update; then exit 0; else exit 1; fi
''')
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertTrue((self.backup / "pending-update.json").exists())


if __name__ == "__main__":
    unittest.main()
