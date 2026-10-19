"""Makefile slot variables: one dlserver slot per port, defaulting to the active one."""

import os
import shutil
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
pytestmark = pytest.mark.skipif(shutil.which("make") is None, reason="needs make")


def _dry(tmp_path: Path, *args: str) -> str:
    r = subprocess.run(
        ["make", "-n", "--no-print-directory", "-C", str(ROOT),
         f"RUN_DIR={tmp_path}", f"LOG_ROOT={tmp_path}/logs", *args],
        capture_output=True, text=True, timeout=60,
    )
    assert r.returncode == 0, r.stderr
    return r.stdout


def test_default_slot_keeps_legacy_paths(tmp_path):
    out = _dry(tmp_path, "start-runpod-dlserver-slot")
    assert f"--pid-file {tmp_path}/dlserver.pid " in out
    assert f"--wrapper-pid-file {tmp_path}/dlserver-wrapper.pid " in out
    assert f"--log-dir {tmp_path}/logs/dlserver " in out
    assert "--port 8001 " in out
    assert f"{tmp_path}/dlserver.rev" in out


def test_second_slot_gets_its_own_paths(tmp_path):
    out = _dry(tmp_path, "start-runpod-dlserver-slot", "DLSERVER_PORT=8002")
    assert f"--pid-file {tmp_path}/dlserver-8002.pid " in out
    assert f"--wrapper-pid-file {tmp_path}/dlserver-8002-wrapper.pid " in out
    assert f"--log-dir {tmp_path}/logs/dlserver-8002 " in out
    assert "--port 8002 " in out


def test_slot_start_skips_install(tmp_path):
    assert "pip install" not in _dry(tmp_path, "start-runpod-dlserver-slot", "DLSERVER_PORT=8002")


def test_default_port_follows_state_file(tmp_path):
    (tmp_path / "dlserver-active").write_text("http://127.0.0.1:8002\n")
    out = _dry(tmp_path, "stop-runpod-dlserver")
    assert (
        f"stop-tree.sh dlserver 8002 {tmp_path}/dlserver-8002-wrapper.pid "
        f"{tmp_path}/dlserver-8002.pid {tmp_path}/logs/dlserver-8002"
    ) in out


def test_garbage_state_file_falls_back_to_slot_a(tmp_path):
    (tmp_path / "dlserver-active").write_text("nonsense\n")
    assert "stop-tree.sh dlserver 8001 " in _dry(tmp_path, "stop-runpod-dlserver")


def test_lbserver_gets_state_file_and_log_dir(tmp_path):
    out = _dry(tmp_path, "start-runpod-lbserver")
    assert f"LB__STATE_FILE={tmp_path}/dlserver-active " in out
    assert f"--log-dir {tmp_path}/logs/lbserver " in out


def test_print_dlserver_pid_follows_state_file(tmp_path):
    (tmp_path / "dlserver-active").write_text("http://127.0.0.1:8002\n")
    r = subprocess.run(
        ["make", "-s", "--no-print-directory", "-C", str(ROOT), f"RUN_DIR={tmp_path}", "print-dlserver-pid"],
        capture_output=True, text=True, timeout=60,
    )
    assert r.returncode == 0, r.stderr
    assert r.stdout.strip() == f"{tmp_path}/dlserver-8002.pid"


def test_default_port_uses_first_line_in_two_slot_mode(tmp_path):
    (tmp_path / "dlserver-active").write_text("http://127.0.0.1:8002\nhttp://127.0.0.1:8001\n")
    out = _dry(tmp_path, "stop-runpod-dlserver")
    assert f"stop-tree.sh dlserver 8002 {tmp_path}/dlserver-8002-wrapper.pid " in out


def test_in_place_restart_refuses_in_two_slot_mode(tmp_path):
    # `install` would rewrite the venv under the second slot that keeps serving.
    (tmp_path / "dlserver-active").write_text("http://127.0.0.1:8001\nhttp://127.0.0.1:8002\n")
    # Stand-in pip/nvidia-smi so a regression can never install anything on the dev box.
    bin_ = tmp_path / "bin"
    bin_.mkdir()
    for name in ("pip", "nvidia-smi"):
        (bin_ / name).write_text(f"#!/bin/sh\necho FAKE-{name} >> {tmp_path}/called\nexit 1\n")
        (bin_ / name).chmod(0o755)
    r = subprocess.run(
        ["make", "--no-print-directory", "-C", str(ROOT), f"RUN_DIR={tmp_path}",
         f"LOG_ROOT={tmp_path}/logs", "start-runpod-dlserver"],
        capture_output=True, text=True, timeout=60,
        env=dict(os.environ, PATH=f"{bin_}:{os.environ['PATH']}"),
    )
    assert r.returncode != 0
    assert "make dlserver-single first" in r.stdout + r.stderr
    assert not (tmp_path / "called").exists()  # install never ran
