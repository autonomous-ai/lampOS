# The apps' fixed launcher (#7415, piece 2): root's, shipped beside the helper,
# never in a package. `python3 -I` ignores the working folder and PYTHONPATH,
# so an app's own `src/` is not importable under it; this puts it on the path
# and runs the entry named in `entry.env` (the unit's EnvironmentFile).
#
# It refuses what the helper would have refused (a bad name or entry, or a
# module that is not there) with exit 78 (EX_CONFIG), which the unit lists in
# RestartPreventExitStatus= so a refused entry does not loop. A crash inside
# the app's own code is not a refusal: its exit is the app's.
#
# LAMP_APPS_ROOT is for tests; the unit never sets it.
import importlib.util
import os
import re
import runpy
import sys

REFUSED = 78
NAME = re.compile(r"[a-z][a-z0-9-]{0,23}\Z")
PART = r"[a-z_][a-z0-9_]*"
ENTRY = re.compile(PART + r"(\." + PART + r"){0,7}\Z")


def refuse(why):
    print("lamp-apps launcher: " + why, file=sys.stderr)
    sys.exit(REFUSED)


name = os.environ.get("LAMP_APP_NAME", "")
entry = os.environ.get("ENTRY", "")
if not NAME.match(name):
    refuse("not an app name")
if len(entry) > 128 or not ENTRY.match(entry):
    refuse("not an entry")

root = os.environ.get("LAMP_APPS_ROOT", "/opt/lampOS/apps")
sys.path.insert(0, os.path.join(root, name, "current", "src"))
sys.dont_write_bytecode = True

try:
    found = importlib.util.find_spec(entry)
except (ImportError, ValueError):
    found = None
if found is None:
    refuse("the entry is not in the app's src")

runpy.run_module(entry, run_name="__main__", alter_sys=True)
