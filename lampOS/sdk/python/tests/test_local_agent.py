"""On the lamp itself, as the user the owner turned on, `Lamp.connect()` needs no pairing: it uses the
agent socket (docs/control.md, "The local agent"). A pairing, or a `host`, keeps the old way."""

from __future__ import annotations

import asyncio
import base64
import json

import pytest
from websockets.asyncio.server import unix_serve

from lampos import Keyring, Lamp
from lampos.client import AGENT_SOCKET_ENV
from lampos.errors import NotPaired


def run(coroutine):
    return asyncio.run(asyncio.wait_for(coroutine, 30))


async def lamp_door(path: str, seen: list):
    """What lamp-control's agent socket says: a welcome, then hello answered with the grants."""

    async def handler(ws):
        await ws.send(json.dumps({"ev": "welcome", "nonce": base64.b64encode(bytes(32)).decode(), "lamp": {"id": "lamp-t"}}))
        hello = json.loads(await ws.recv())
        seen.append(hello)
        await ws.send(json.dumps({"ok": True, "re": hello["id"], "lamp": {"id": "lamp-t", "name": "Lamp", "v": 1},
                                  "grants": ["body", "light", "look", "skills", "speak"], "limits": {}}))
        async for _ in ws:
            pass

    return unix_serve(handler, path)


def test_with_no_pairing_the_agent_socket_is_used_and_hello_names_no_key(tmp_path, monkeypatch):
    path = str(tmp_path / "agent.sock")
    monkeypatch.setenv(AGENT_SOCKET_ENV, path)
    seen: list = []

    async def go():
        async with await lamp_door(path, seen):
            async with await Lamp.connect(keyring=Keyring(tmp_path / "home")) as lamp:
                assert sorted(lamp.grants) == ["body", "light", "look", "skills", "speak"]

    run(go())
    assert seen[0]["op"] == "hello" and seen[0]["client"] == "c_0"
    assert base64.b64decode(seen[0]["sig"]) == bytes(64)


def test_without_a_pairing_or_a_socket_it_is_not_paired(tmp_path, monkeypatch):
    monkeypatch.setenv(AGENT_SOCKET_ENV, str(tmp_path / "none.sock"))
    with pytest.raises(NotPaired):
        run(Lamp.connect(keyring=Keyring(tmp_path / "home")))


def test_a_file_that_is_not_a_socket_is_not_one(tmp_path, monkeypatch):
    plain = tmp_path / "agent.sock"
    plain.write_text("x")
    monkeypatch.setenv(AGENT_SOCKET_ENV, str(plain))
    with pytest.raises(NotPaired):
        run(Lamp.connect(keyring=Keyring(tmp_path / "home")))


def test_a_host_asked_for_never_uses_the_socket(tmp_path, monkeypatch):
    path = str(tmp_path / "agent.sock")
    monkeypatch.setenv(AGENT_SOCKET_ENV, path)

    async def go():
        async with await lamp_door(path, []):
            await Lamp.connect(host="lamp-x.local", keyring=Keyring(tmp_path / "home"))

    with pytest.raises(NotPaired):
        run(go())
