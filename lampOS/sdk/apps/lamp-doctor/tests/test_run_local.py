"""`run_local.py` runs from the app's folder under `-I` (the isolated mode drops the current folder from the path)."""

import json
import subprocess
import sys
from pathlib import Path

APP = Path(__file__).resolve().parent.parent


def test_run_local_works_isolated_from_the_app_folder(tmp_path):
    rows = tmp_path / "rows.json"
    rows.write_text(json.dumps([{"request": 1, "piece_at": 1_700_000_000_000, "first_audio_ms": 1500}]))
    out = subprocess.run([sys.executable, "-I", "run_local.py", str(rows), "--day", "2023-11-14"],
                         cwd=APP, capture_output=True, text=True, timeout=30)
    assert out.returncode == 0, out.stderr
    assert "Lamp score:" in out.stdout or "Lamp Doctor, 2023-11-1" in out.stdout
