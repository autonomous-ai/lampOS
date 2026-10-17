# The units lamp-4ace runs

Copies of the systemd units on lamp-4ace, the office lab prototype: its voice
(`lamp-voice.service`), body (`lamp-body.service`) and eyes
(`lamp-eyes.service`) as they were installed on 2026-10-13, and its hand
controls (`lamp-controls.service`, watching only) since 2026-10-14. The working copies are
beside the deploy scripts on the machine that installs them; these are here
so that what the lamp runs is in the repository. They name one lamp
(`ConditionHost=lamp-4ace`), its devices and its measured poses, and are not
a template for another unit. No credential is in them: the provider's are in
a private file the voice unit only names.

`lamp-selftest.service` is the self-test, a oneshot started only by hand
(`sudo systemctl start lamp-selftest`), never at boot, with `CAP_SYSLOG` for
its storage part ([selftest.md](../../docs/selftest.md)).

The long hexadecimal directories in `ExecStart` are the SHA-256 of the binary
installed there. The character file the voice unit's configuration names is
[`../character/lamp.txt`](../character/lamp.txt).

`controls.json` and `controls-pins.json` are the hand controls' settings and
the board's wiring ([controls.md](../../docs/controls.md)). On the lamp they
sit beside the build they were installed with, not in `/etc/lampOS`, which
only root may read; the controls run as `orangepi`. `lean-first-software.json`
is the first software's body model in the lean check's form, a stand-in
([lean.md](../../docs/lean.md), "lamp-4ace, 14 October").

`fold-and-start.sh` is step 4a of the Sunday sheet, run on the lamp by the
operator with the owner at the arm: it checks the body is stopped and nothing
holds the servo port, reads the arm, waits while the arm is folded by hand,
re-reads it (shoulder and elbow within 80 of `--folded-arm`, torque on), asks
for the elbow and cable check, starts the body through the operator's
`body-integration.sh` and prints `lamp-body exit-report`. It never writes to a
servo: the torque off and on for the shoulder and elbow are the operator's
manual steps. Run it with `--dry-run` first to see every command.
