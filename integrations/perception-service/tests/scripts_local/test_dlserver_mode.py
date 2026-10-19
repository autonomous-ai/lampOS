"""dlserver-mode.sh and deploy-dlserver.sh in two-slot mode, against shimmed tools (#612)."""

import json
import os
import shutil
import signal
import subprocess
import sys
import textwrap
import time
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
pytestmark = pytest.mark.skipif(shutil.which("bash") is None, reason="needs bash")

A, B, LB = "8001", "8002", "7999"

SHIMS = {
    "ss": """
        port=${@: -1}; port=${port##*:}
        f="$FAKE/listen-$port"
        [ -f "$f" ] && echo "LISTEN 0 0 127.0.0.1:$port users:((\\"python\\",pid=$(cat "$f"),fd=3))"
        exit 0
    """,
    "make": """
        echo "make $*" >>"$FAKE/make.log"
        target=""; port=""
        for a in "$@"; do case $a in DLSERVER_PORT=*) port=${a#*=};; -*|*=*) ;; *) target=$a;; esac; done
        case $target in
          start-runpod-dlserver-slot)
            [ -f "$FAKE/fail-start-$port" ] && exit 1
            sleep 600 >/dev/null 2>&1 & echo $! >"$FAKE/listen-$port"
            if [ "$port" = 8001 ]; then r=$RUN_DIR/dlserver.rev; else r=$RUN_DIR/dlserver-$port.rev; fi
            echo abc123 >"$r" ;;
          stop-runpod-dlserver)
            [ -f "$FAKE/listen-$port" ] && kill "$(cat "$FAKE/listen-$port")" 2>/dev/null
            rm -f "$FAKE/listen-$port" ;;
        esac
        exit 0
    """,
    "curl": """
        url=${@: -1}; port=${url#http://127.0.0.1:}; port=${port%%/*}
        [ -f "$FAKE/listen-$port" ] || exit 7
        if [ -f "$FAKE/missing-$port" ]; then echo '{"models":{"pose":true,"fer":false}}'
        else echo '{"models":{"pose":true,"fer":true}}'; fi
    """,
    "nvidia-smi": """
        case "$*" in *memory.used*) echo "9000, ${FAKE_FREE:-20000}, 32760";; *) echo "${FAKE_FREE:-20000}";; esac
    """,
    "flock": "exit 0",
    "git": """
        case $1 in rev-parse) echo abc123;; diff) exit "${FAKE_GIT_DIFF:-0}";; *) exit 0;; esac
    """,
    "python": f'exec "{sys.executable}" "$@"',
}

FAKE_LB = textwrap.dedent("""
    import json, os, signal, sys, time
    from pathlib import Path
    state, fake = Path(sys.argv[1]), Path(sys.argv[2])
    def ack(*_):
        if (fake / "lb-ignore").exists():
            return
        urls = [l.strip() for l in state.read_text().splitlines() if l.strip()]
        tmp = state.with_name(state.name + ".applied.tmp")
        tmp.write_text(json.dumps({"pid": os.getpid(), "backends": urls}))
        tmp.replace(state.with_name(state.name + ".applied"))
    signal.signal(signal.SIGHUP, ack)
    ack()
    while True:
        time.sleep(1)
""")


@pytest.fixture
def env(tmp_path):
    fake, bin_ = tmp_path / "fake", tmp_path / "bin"
    fake.mkdir()
    bin_.mkdir()
    for name, body in SHIMS.items():
        p = bin_ / name
        p.write_text("#!/usr/bin/env bash\n" + textwrap.dedent(body))
        p.chmod(0o755)
    state = tmp_path / "dlserver-active"
    state.write_text(f"http://127.0.0.1:{A}\n")
    slot_a = subprocess.Popen(["sleep", "600"])
    (fake / f"listen-{A}").write_text(str(slot_a.pid))
    (tmp_path / "dlserver.rev").write_text("abc123\n")
    lb = subprocess.Popen([sys.executable, "-c", FAKE_LB, str(state), str(fake)])
    (fake / f"listen-{LB}").write_text(str(lb.pid))
    for _ in range(100):
        if state.with_name(state.name + ".applied").exists():
            break
        time.sleep(0.02)
    e = dict(os.environ, PATH=f"{bin_}:{os.environ['PATH']}", FAKE=str(fake), RUN_DIR=str(tmp_path),
             LOG_ROOT=str(tmp_path / "logs"), DLSERVER_STATE_FILE=str(state), DL_API_KEY="k",
             DEPLOY_DRAIN_SECONDS="0", DEPLOY_READY_TIMEOUT="10")
    yield {"env": e, "state": state, "fake": fake, "tmp": tmp_path}
    for f in fake.glob("listen-*"):
        try:
            os.kill(int(f.read_text()), signal.SIGKILL)
        except (ProcessLookupError, ValueError):
            pass
    lb.kill()
    slot_a.kill()


def _run(ctx, *args, script="dlserver-mode.sh", **extra):
    e = dict(ctx["env"], **extra)
    return subprocess.run(["bash", f"scripts/{script}", *args], cwd=ROOT, env=e,
                          capture_output=True, text=True, timeout=120)


def _state(ctx):
    return [l.rsplit(":", 1)[1] for l in ctx["state"].read_text().split()]


def _acked(ctx):
    raw = ctx["state"].with_name(ctx["state"].name + ".applied").read_text()
    return [u.rsplit(":", 1)[1] for u in json.loads(raw)["backends"]]


def _listening(ctx, port):
    return (ctx["fake"] / f"listen-{port}").exists()


def test_two_slot_starts_idle_and_serves_both(env):
    r = _run(env, "two")
    assert r.returncode == 0, r.stdout + r.stderr
    assert _state(env) == [A, B] and _acked(env) == [A, B]
    assert _listening(env, B)


def test_two_slot_refuses_when_already_two(env):
    assert _run(env, "two").returncode == 0
    r = _run(env, "two")
    assert r.returncode == 1 and "already in two-slot mode" in r.stdout


def test_two_slot_refuses_when_checkout_differs(env):
    r = _run(env, "two", FAKE_GIT_DIFF="1")
    assert r.returncode == 1 and "make deploy-dlserver first" in r.stdout
    assert not _listening(env, B) and _state(env) == [A]


def test_two_slot_refuses_low_gpu_memory(env):
    r = _run(env, "two", FAKE_FREE="8000")
    assert r.returncode == 1 and not _listening(env, B)


def test_two_slot_missing_models_stops_idle_and_keeps_traffic(env):
    (env["fake"] / f"missing-{B}").touch()
    r = _run(env, "two")
    assert r.returncode == 1 and "missing models: fer" in r.stdout
    assert not _listening(env, B) and _state(env) == [A]


def test_two_slot_unacked_switch_rolls_back_to_active(env):
    # lbserver ignores the SIGHUP: its ack keeps naming only 8001, which is exactly
    # the rollback target, so the rollback counts as confirmed and 8002 is stopped.
    (env["fake"] / "lb-ignore").touch()
    r = _run(env, "two")
    assert r.returncode == 1 and "did not confirm two slots" in r.stdout
    assert _state(env) == [A] and not _listening(env, B)


def test_single_returns_to_8001_and_stops_8002(env):
    assert _run(env, "two").returncode == 0
    r = _run(env, "single")
    assert r.returncode == 0, r.stdout + r.stderr
    assert _state(env) == [A] and _acked(env) == [A]
    assert not _listening(env, B) and _listening(env, A)


def test_single_when_already_single_is_a_no_op(env):
    r = _run(env, "single")
    assert r.returncode == 0 and "already single" in r.stdout
    assert _listening(env, A)


def test_single_keeps_live_slot_when_8001_is_dead(env):
    assert _run(env, "two").returncode == 0
    os.kill(int((env["fake"] / f"listen-{A}").read_text()), signal.SIGKILL)
    (env["fake"] / f"listen-{A}").unlink()
    r = _run(env, "single")
    assert r.returncode == 0, r.stdout + r.stderr
    assert _state(env) == [B] and _listening(env, B)


def test_status_reports_mode(env):
    assert "mode: single" in _run(env, "status").stdout
    assert _run(env, "two").returncode == 0
    assert "mode: two-slot" in _run(env, "status").stdout


def test_deploy_in_two_slot_mode_ends_in_two_slot_on_new_code(env):
    assert _run(env, "two").returncode == 0
    r = _run(env, script="deploy-dlserver.sh")
    assert r.returncode == 0, r.stdout + r.stderr
    assert sorted(_state(env)) == [A, B] and sorted(_acked(env)) == [A, B]
    assert _listening(env, A) and _listening(env, B)
    log = (env["fake"] / "make.log").read_text().splitlines()
    starts = [l for l in log if "start-runpod-dlserver-slot" in l]
    assert len(starts) == 3  # two-slot on, the deploy's new slot, two-slot back on


def test_deploy_in_single_mode_is_unchanged(env):
    r = _run(env, script="deploy-dlserver.sh")
    assert r.returncode == 0, r.stdout + r.stderr
    assert _state(env) == [B] and _acked(env) == [B]
    assert _listening(env, B) and not _listening(env, A)
