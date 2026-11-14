"""Inside a skill's run on the lamp, `Lamp.connect()` with no pairing uses the run's token
(`Lamp.local(token)`), as a skill's script written with `Lamp.connect()` needs: no `NotPaired`.
A pairing, a `host`, or an empty token keeps the old way."""

from __future__ import annotations

import asyncio

import pytest

from lampos import Keyring, Lamp, NotPaired, Pairing, errors
from lampos.client import AGENT_SOCKET_ENV
from lampos.testing import FakeLamp


def run(coroutine):
    return asyncio.run(asyncio.wait_for(coroutine, 30))


def alone(monkeypatch, tmp_path, fake=None, token=None):
    """No agent socket, no pairing folder, and the run's environment (or none)."""
    monkeypatch.setenv(AGENT_SOCKET_ENV, str(tmp_path / "no-agent.sock"))
    monkeypatch.delenv("LAMP_CONTROL_TOKEN", raising=False)
    monkeypatch.delenv("LAMP_CONTROL_SOCKET", raising=False)
    if fake is not None:
        monkeypatch.setenv("LAMP_CONTROL_SOCKET", fake.local_path)
    if token is not None:
        monkeypatch.setenv("LAMP_CONTROL_TOKEN", token)
    return Keyring(tmp_path / "keys")


def test_connect_with_no_pairing_and_a_run_token_is_the_runs_local_connection(monkeypatch, tmp_path):
    async def go():
        async with FakeLamp(local=True, grants=("body", "light")) as fake:
            keyring = alone(monkeypatch, tmp_path, fake, fake.mint_token(["light"], run="r1", skill="water"))
            async with await Lamp.connect(keyring=keyring) as lamp:
                # The token's grants and no more, the run it is for.
                assert (lamp.grants, lamp.run, lamp.skill) == (["light"], "r1", "water")
                assert (await lamp.light("warm", 3, 1000))["to"] == "ring"
                with pytest.raises(errors.BY_CODE["no_grant"], match="not granted: body"):
                    await lamp.gesture("nod")
            assert not (tmp_path / "keys").exists(), "no key or pairing was made"

    run(go())


def test_an_empty_or_blank_token_is_not_set_and_a_host_is_never_given_the_token(monkeypatch, tmp_path):
    async def go():
        async with FakeLamp(local=True, grants=("light",)) as fake:
            token = fake.mint_token(["light"])
            for unset in ("", "   "):
                keyring = alone(monkeypatch, tmp_path, fake, unset)
                with pytest.raises(NotPaired):
                    await Lamp.connect(keyring=keyring)
            keyring = alone(monkeypatch, tmp_path, fake, token)
            with pytest.raises(NotPaired):
                await Lamp.connect(host="127.0.0.1", keyring=keyring)

    run(go())


def test_a_pairing_wins_over_the_token(monkeypatch, tmp_path):
    async def go():
        async with FakeLamp(local=True, grants=("light",)) as fake:
            keyring = alone(monkeypatch, tmp_path, fake, fake.mint_token(["light"]))
            # A pairing to a door nobody listens at: connect goes there, not to the control socket.
            keyring.remember(Pairing(lamp="lamp-x", name="x", client="c_1", grants=["light"], fp="00" * 32, host="127.0.0.1", port=1))
            with pytest.raises(Exception) as caught:
                await Lamp.connect(keyring=keyring)
            assert not isinstance(caught.value, errors.NotPaired)
            assert not isinstance(caught.value, errors.LampError), repr(caught.value)

    run(go())


def test_a_refused_token_is_never_in_the_error(monkeypatch, tmp_path):
    async def go():
        async with FakeLamp(local=True, grants=("light",)) as fake:
            bad = "B" * 43
            keyring = alone(monkeypatch, tmp_path, fake, bad)
            with pytest.raises(errors.BY_CODE["no_grant"]) as caught:
                await Lamp.connect(keyring=keyring)
            assert bad not in str(caught.value) and bad not in repr(caught.value)

    run(go())
