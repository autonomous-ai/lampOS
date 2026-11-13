"""A skill's run whose engine has no per-run wrapper (OpenClaw's one gateway, #6685): in the run's own folder,
`Lamp.connect()` / `Lamp.local()` read the run's token file once and delete it. A token in the environment wins,
and anywhere else nothing is read."""

from __future__ import annotations

import asyncio
import os

import pytest

from lampos import Keyring, Lamp, NotPaired, errors
from lampos import client
from lampos.client import AGENT_SOCKET_ENV
from lampos.testing import FakeLamp


def run(coroutine):
    return asyncio.run(asyncio.wait_for(coroutine, 30))


def lamp_dirs(monkeypatch, tmp_path, run_id="r-1"):
    """The run's folder and the token folder under tmp, the process working in the run's folder; a short wait."""
    monkeypatch.setattr(client, "RUN_TOKEN_WAIT_S", 0.3)
    folders = tmp_path / "harnesses" / "lamp-tasks"
    tokens = tmp_path / "runs"
    work = folders / f"skill-{run_id}"
    work.mkdir(parents=True)
    tokens.mkdir(mode=0o700)
    monkeypatch.setattr(client, "RUN_FOLDERS", str(folders.resolve()))
    monkeypatch.setattr(client, "RUN_TOKENS", str(tokens))
    monkeypatch.chdir(work)
    monkeypatch.setenv(AGENT_SOCKET_ENV, str(tmp_path / "no-agent.sock"))
    monkeypatch.delenv("LAMP_CONTROL_TOKEN", raising=False)
    return work, tokens


def put(tokens, run_id, text):
    path = tokens / run_id
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "w") as file:
        file.write(text)
    return path


def test_in_the_runs_folder_connect_reads_the_token_once_and_deletes_it(monkeypatch, tmp_path):
    async def go():
        async with FakeLamp(local=True, grants=("light",)) as fake:
            _, tokens = lamp_dirs(monkeypatch, tmp_path)
            monkeypatch.setenv("LAMP_CONTROL_SOCKET", fake.local_path)
            file = put(tokens, "r-1", fake.mint_token(["light"], run="r-1", skill="water") + "\n")
            async with await Lamp.connect(keyring=Keyring(tmp_path / "keys")) as lamp:
                assert (lamp.grants, lamp.run, lamp.skill) == (["light"], "r-1", "water")
                assert not file.exists(), "read once, then deleted"
            # A second connect in the same run finds nothing left to read.
            with pytest.raises(NotPaired):
                await Lamp.connect(keyring=Keyring(tmp_path / "keys"))
            with pytest.raises(errors.BY_CODE["no_grant"]):
                await Lamp.local()

    run(go())


def test_a_token_in_the_environment_wins_and_the_file_is_left_to_its_owner(monkeypatch, tmp_path):
    async def go():
        async with FakeLamp(local=True, grants=("light",)) as fake:
            _, tokens = lamp_dirs(monkeypatch, tmp_path)
            monkeypatch.setenv("LAMP_CONTROL_SOCKET", fake.local_path)
            file = put(tokens, "r-1", "not-this-one")
            monkeypatch.setenv("LAMP_CONTROL_TOKEN", fake.mint_token(["light"], run="r-1"))
            async with await Lamp.local() as lamp:
                assert lamp.run == "r-1"
            assert file.exists(), "never read when the wrapper gave the token"

    run(go())


def test_outside_a_runs_own_folder_nothing_is_read(monkeypatch, tmp_path):
    work, tokens = lamp_dirs(monkeypatch, tmp_path)
    file = put(tokens, "r-1", "tok")
    for elsewhere in (work.parent, tmp_path, work / "sub"):
        elsewhere.mkdir(exist_ok=True)
        monkeypatch.chdir(elsewhere)
        assert client._run_token() is None
    # A folder that only looks like a run's under another parent.
    other = tmp_path / "other" / "skill-r-1"
    other.mkdir(parents=True)
    monkeypatch.chdir(other)
    assert client._run_token() is None
    # A link to the run's folder resolves to it: `pwd -P` is the run's.
    link = tmp_path / "here"
    link.symlink_to(work)
    monkeypatch.chdir(link)
    assert file.exists()
    assert client._run_token() == "tok"
    assert not file.exists()


@pytest.mark.parametrize("name", ["skill-", "skill-R1", "skill-a_b", "skill-" + "a" * 65, "skill-a.b", "run-1"])
def test_a_run_id_that_is_not_plain_reads_nothing(monkeypatch, tmp_path, name):
    _, tokens = lamp_dirs(monkeypatch, tmp_path)
    folder = tmp_path / "harnesses" / "lamp-tasks" / name
    folder.mkdir()
    monkeypatch.chdir(folder)
    assert client._run_token() is None


def test_a_linked_or_odd_token_file_is_never_followed(monkeypatch, tmp_path):
    _, tokens = lamp_dirs(monkeypatch, tmp_path)
    secret = tmp_path / "secret"
    secret.write_text("someone-elses")
    (tokens / "r-1").symlink_to(secret)
    assert client._run_token() is None
    assert not (tokens / "r-1").is_symlink(), "the link is taken away"
    assert secret.read_text() == "someone-elses"
    (tokens / "r-1").mkdir()
    assert client._run_token() is None
    os.rmdir(tokens / "r-1")
    put(tokens, "r-1", "  \n")
    assert client._run_token() is None, "a blank file is no token"
    assert not (tokens / "r-1").exists()
    assert client._run_token() is None, "no file, no token"


def test_a_token_file_written_just_after_the_script_starts_is_still_read(monkeypatch, tmp_path):
    async def go():
        async with FakeLamp(local=True, grants=("light",)) as fake:
            _, tokens = lamp_dirs(monkeypatch, tmp_path)
            monkeypatch.setattr(client, "RUN_TOKEN_WAIT_S", 5.0)
            monkeypatch.setenv("LAMP_CONTROL_SOCKET", fake.local_path)
            secret = fake.mint_token(["light"], run="r-1")

            async def later():
                await asyncio.sleep(0.4)
                put(tokens, "r-1", secret)

            writer = asyncio.create_task(later())
            async with await Lamp.local() as lamp:
                assert lamp.run == "r-1"
            await writer
            assert not (tokens / "r-1").exists()

    run(go())


def test_outside_a_run_nothing_is_waited_for(monkeypatch, tmp_path):
    monkeypatch.setattr(client, "RUN_TOKEN_WAIT_S", 30.0)
    monkeypatch.chdir(tmp_path)
    monkeypatch.delenv("LAMP_CONTROL_TOKEN", raising=False)
    started = asyncio.run(asyncio.wait_for(client._run_token_waited(), 2))
    assert started is None
