"""Run Lamp Doctor's laptop scorer from this folder: python3 -I run_local.py metrics.json --html report.html

`-I` drops the current folder from the module path, so this puts `src` on it first. Standard input works too:
    curl -s 'http://127.0.0.1:8790/metrics?since=0&until=9999999999999' | python3 -I run_local.py - --html report.html
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent / "src"))

from lamp_doctor.local import main  # noqa: E402

raise SystemExit(main())
