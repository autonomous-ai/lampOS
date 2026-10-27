"""lamp-mcp against the SDK's fake lamp, through an MCP client session in
memory: what each tool sends, the grants, the leases, the waits and the
refusals. No lamp needed, and nothing here stands in for a lamp check."""

from __future__ import annotations

import asyncio
import json
from contextlib import asynccontextmanager
from pathlib import Path

import pytest
from lampos import Keyring, Lamp, schema
from lampos.testing import FakeLamp
from mcp.shared.memory import create_connected_server_and_client_session

from lamp_mcp.server import Limits, build, gestures

ALL = {"lamp_state", "lamp_wait_for", "lamp_express", "lamp_gesture", "lamp_look", "lamp_move", "lamp_light", "lamp_say", "lamp_stop"}


def run(coroutine):
    return asyncio.run(asyncio.wait_for(coroutine, timeout=20))


@asynccontextmanager
async def connected(folder: Path, grants=("body", "light", "speak"), auto_done=True):
    """A fake lamp, this computer paired with it, and an MCP session on the
    server built for it."""
    async with FakeLamp(grants=tuple(grants), auto_done=auto_done) as fake:
        ring = Keyring(folder)
        await Lamp.pair(fake.code, fake.host, pin=fake.pin, port=fake.port, name="test laptop", keyring=ring)
        async with await Lamp.connect(keyring=ring) as lamp:
            server = build(lamp, client="test laptop")
            async with create_connected_server_and_client_session(server._mcp_server) as session:
                yield fake, lamp, session


def text(result) -> str:
    return "".join(getattr(part, "text", "") for part in result.content)


async def until(check, *, within=5.0):
    """Until `check()` holds, polling the fake lamp's record."""
    deadline = asyncio.get_running_loop().time() + within
    while not check():
        assert asyncio.get_running_loop().time() < deadline, "the fake lamp never got it"
        await asyncio.sleep(0.01)


def test_tools_are_listed_by_the_lamps_grants(tmp_path):
    async def go():
        async with connected(tmp_path) as (_, _, session):
            assert {tool.name for tool in (await session.list_tools()).tools} == ALL
        async with connected(tmp_path / "light", grants=("light",)) as (_, _, session):
            assert {tool.name for tool in (await session.list_tools()).tools} == {
                "lamp_state",
                "lamp_wait_for",
                "lamp_light",
                "lamp_stop",
            }

    run(go())


def test_the_gestures_offered_are_the_lamps_less_idle_and_scanning(tmp_path):
    assert set(gestures()) == set(schema.gestures()) - {"idle", "scanning"}

    async def go():
        async with connected(tmp_path) as (_, _, session):
            tool = next(t for t in (await session.list_tools()).tools if t.name == "lamp_gesture")
            offered = json.dumps(tool.inputSchema)
            assert all(f'"{name}"' in offered for name in gestures())
            assert '"idle"' not in offered and '"scanning"' not in offered

    run(go())


def test_each_tool_sends_its_one_op(tmp_path):
    async def go():
        async with connected(tmp_path, grants=("body", "light", "speak", "look")) as (fake, _, session):
            assert not (await session.call_tool("lamp_gesture", {"name": "nod"})).isError
            assert not (await session.call_tool("lamp_light", {"colour": "warm", "level": 2, "for_ms": 90_000})).isError
            assert not (await session.call_tool("lamp_say", {"text": "Build passed."})).isError
            assert not (await session.call_tool("lamp_express", {"feeling": "glad"})).isError
            assert fake.asked("gesture") == [{"name": "nod"}]
            # 30 s at most, whatever was asked.
            assert fake.asked("light") == [{"colour": "warm", "level": 2, "for_ms": 30_000}]
            assert fake.asked("say") == [{"text": "Build passed."}]
            assert fake.asked("express") == [{"feeling": "glad", "strength": 0.5}]
            state = await session.call_tool("lamp_state", {"which": ["presence"]})
            assert json.loads(text(state)) == {"presence": {"count": 0}}

    run(go())


def test_a_look_takes_its_own_lease_and_lets_go_at_done(tmp_path):
    async def go():
        async with connected(tmp_path) as (fake, _, session):
            call = asyncio.create_task(session.call_tool("lamp_look", {"yaw": -10, "pitch": 5}))
            await until(lambda: fake.asked("look"))
            (taken,) = fake.asked("lease.take")
            assert taken["parts"] == ["body"] and taken["for_ms"] == 10_000
            assert taken["why"] == "MCP: test laptop"
            lease = next(iter(fake.leases))
            look = next(r for r in fake.requests if r["op"] == "look")
            await fake.done(look["id"], "finished")
            result = await call
            assert not result.isError and text(result) == "look finished"
            await until(lambda: fake.asked("lease.release"))
            assert fake.asked("lease.release") == [{"lease": lease}] and not fake.leases
            assert look["at"] == {"yaw": -10.0, "pitch": 5.0}

    run(go())


def test_a_lease_the_lamp_takes_back_is_the_tools_error(tmp_path):
    async def go():
        async with connected(tmp_path, auto_done=False) as (fake, _, session):
            frames = [[0, 0, 0, 0, 0], [20, 0, 30, 0, 0], [0, 0, 0, 0, 0]]
            call = asyncio.create_task(session.call_tool("lamp_move", {"step_ms": 40, "frames": frames}))
            await until(lambda: fake.asked("move"))
            await fake.lease_event(next(iter(fake.leases)), "lost", why="hand")
            result = await call
            assert result.isError and "lease_lost: hand" in text(result)
            # Lost is gone already: nothing to let go of, and no retry.
            assert not fake.asked("lease.release") and len(fake.asked("move")) == 1

    run(go())


def test_a_body_another_holds_is_busy_and_never_retried(tmp_path):
    async def go():
        async with connected(tmp_path) as (fake, _, session):
            fake.refuse_next("lease.take", "busy", "another client holds it")
            result = await session.call_tool("lamp_look", {"at": "ahead"})
            assert result.isError and "busy: another client holds it" in text(result)
            assert len(fake.asked("lease.take")) == 0 and not fake.asked("look")
            fake.refuse_next("gesture", "busy", "another client holds it")
            result = await session.call_tool("lamp_gesture", {"name": "nod"})
            assert result.isError and "busy" in text(result)

    run(go())


def test_a_refusal_keeps_the_lamps_code_words_and_retry(tmp_path):
    async def go():
        async with connected(tmp_path) as (fake, _, session):
            fake.refuse_next("say", "rate", "six a minute", retry_ms=900)
            result = await session.call_tool("lamp_say", {"text": "Hello."})
            assert result.isError and "rate: six a minute (try again in 900 ms)" in text(result)
            fake.refuse_next("gesture", "quiet_hours", "it's night")
            result = await session.call_tool("lamp_gesture", {"name": "nod"})
            assert result.isError and "quiet_hours: it's night" in text(result)
            result = await session.call_tool("lamp_say", {"text": "x" * 301})
            assert result.isError and not fake.asked("say")

    run(go())


def test_wait_for_gives_the_first_event_then_unsubscribes(tmp_path):
    async def go():
        async with connected(tmp_path, grants=("body", "light", "speak", "look")) as (fake, _, session):
            call = asyncio.create_task(
                session.call_tool("lamp_wait_for", {"events": ["person.arrived", "touch"], "timeout_s": 5})
            )
            await until(lambda: fake.asked("subscribe"))
            await fake.emit("person.arrived", count=1, near=0.4)
            result = await call
            event = json.loads(text(result))
            assert event["ev"] == "person.arrived" and event["count"] == 1
            await until(lambda: len(fake.asked("unsubscribe")) == 2)
            assert sorted(u["events"][0] for u in fake.asked("unsubscribe")) == ["person.arrived", "touch"]
            recent = await session.read_resource("lamp://events/recent")
            assert json.loads(recent.contents[0].text)[-1]["ev"] == "person.arrived"
            # Nothing comes: timed out, and unsubscribed again.
            result = await session.call_tool("lamp_wait_for", {"events": ["touch"], "timeout_s": 0.2})
            assert json.loads(text(result)) == {"timed_out": True}
            await until(lambda: len(fake.asked("unsubscribe")) == 3)
            result = await session.call_tool("lamp_wait_for", {"events": ["a.transcript"]})
            assert result.isError

    run(go())


def test_stop_lets_go_of_a_lease_left_to_lapse(tmp_path):
    async def go():
        async with connected(tmp_path) as (fake, _, session):
            call = asyncio.create_task(session.call_tool("lamp_look", {"at": "ahead"}))
            await until(lambda: fake.asked("look"))
            lease = next(iter(fake.leases))
            stop = await session.call_tool("lamp_stop", {})
            assert not stop.isError and fake.asked("stop") == [{}]
            assert fake.asked("lease.release") == [{"lease": lease}]
            call.cancel()

    run(go())


def test_the_state_resources_read_the_lamp(tmp_path):
    async def go():
        async with connected(tmp_path, grants=("body", "light", "speak", "look")) as (fake, _, session):
            fake.state["presence"] = {"count": 2, "near": 0.3}
            read = await session.read_resource("lamp://state/presence")
            assert json.loads(read.contents[0].text) == {"presence": {"count": 2, "near": 0.3}}
            uris = {str(r.uri) for r in (await session.list_resources()).resources}
            assert {"lamp://state/presence", "lamp://state/lamp", "lamp://lease", "lamp://events/recent"} <= uris

    run(go())


def test_without_look_presence_is_neither_offered_nor_asked(tmp_path):
    """blue's P2 on #3802: `get presence` and `person.*` need `look`."""

    async def go():
        async with connected(tmp_path) as (fake, _, session):
            state = await session.call_tool("lamp_state", {})
            assert not state.isError, text(state)
            assert "presence" not in json.loads(text(state))
            assert all("presence" not in get["state"] for get in fake.asked("get"))
            uris = {str(r.uri) for r in (await session.list_resources()).resources}
            assert "lamp://state/presence" not in uris and "lamp://state/lamp" in uris
            asked = await session.call_tool("lamp_state", {"which": ["presence"]})
            assert asked.isError and "no_grant: " in text(asked)
            waited = await session.call_tool("lamp_wait_for", {"events": ["person.arrived"], "timeout_s": 1})
            assert waited.isError and "no_grant: " in text(waited)
            assert not fake.asked("subscribe")

    run(go())


def test_the_lamps_limits_are_kept_here_too():
    now = [0.0]
    limits = Limits({"say": [{"most": 2, "within_ms": 60_000}], "joints": 20}, clock=lambda: now[0])
    limits.take("say")
    limits.take("say")
    with pytest.raises(Exception, match=r"rate: at most 2 say in 60 s \(try again in"):
        limits.take("say")
    now[0] = 61.0
    limits.take("say")
    # Ops with no window, and shapes it doesn't know, are left to the lamp.
    for _ in range(50):
        limits.take("gesture")


def test_an_unpaired_computer_is_told_in_one_line(tmp_path, monkeypatch):
    from lamp_mcp.server import main

    monkeypatch.setenv("HOME", str(tmp_path))
    with pytest.raises(SystemExit, match=r"^lamp-mcp: not_paired: .*lamp pair"):
        main([])
