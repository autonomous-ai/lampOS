"""lamp-mcp against the SDK's fake lamp, through an MCP client session in
memory: what each tool sends, the grants, the leases, the waits and the
refusals. No lamp needed, and nothing here stands in for a lamp check."""

from __future__ import annotations

import asyncio
import contextlib
import json
from contextlib import asynccontextmanager
from pathlib import Path

import pytest
from lampos import Keyring, Lamp, schema
from lampos.testing import FakeLamp
from mcp.shared.memory import create_connected_server_and_client_session
from pydantic import AnyUrl

from lamp_mcp.server import Limits, build, gestures

ALL = {
    "lamp_state", "lamp_wait_for", "lamp_express", "lamp_gesture", "lamp_look", "lamp_move", "lamp_play", "lamp_joints",
    "lamp_light", "lamp_say", "lamp_sound", "lamp_events", "lamp_stop",
}
# The schema's ops a tool doesn't send itself: pairing and hello are the
# SDK's, before any tool; leases go with each body tool.
NOT_A_TOOL = {"pair", "hello", "lease.take", "lease.renew", "lease.release"}
# Offered only with both body and look.
FOLLOW = {"lamp_follow_hand", "lamp_follow_stop"}


def run(coroutine):
    return asyncio.run(asyncio.wait_for(coroutine, timeout=20))


@asynccontextmanager
async def connected(
    folder: Path, grants=("body", "light", "speak"), auto_done=True, clock=None, message_handler=None, **fake_options
):
    """A fake lamp, this computer paired with it, and an MCP session on the
    server built for it."""
    async with FakeLamp(grants=tuple(grants), auto_done=auto_done, **fake_options) as fake:
        ring = Keyring(folder)
        await Lamp.pair(fake.code, fake.host, pin=fake.pin, port=fake.port, name="test laptop", keyring=ring)
        async with await Lamp.connect(keyring=ring) as lamp:
            server = build(lamp, client="test laptop", **({"clock": clock} if clock else {}))
            async with create_connected_server_and_client_session(server._mcp_server, message_handler=message_handler) as session:
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
                "lamp_events",
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
            assert json.loads(text(state)) == {"presence": {"count": 0, "near": 0.0, "facing": False}}

    run(go())


def test_every_op_of_the_schema_is_sent_by_a_tool(tmp_path):
    """A new op in the schema fails here until a tool sends it (or it's
    named in NOT_A_TOOL), so an agent can do all a computer can."""
    import inspect

    import lamp_mcp.server as module

    source = inspect.getsource(module)
    # Sent through the SDK's own calls rather than by name.
    through = {"get": "lamp.get(", "subscribe": "lamp.on(", "unsubscribe": "lamp.off("}
    sent = set()
    for op in schema.ops():
        if op in NOT_A_TOOL:
            continue
        if f'"{op}"' in source or (op in through and through[op] in source):
            sent.add(op)
    assert sent == set(schema.ops()) - NOT_A_TOOL, set(schema.ops()) - NOT_A_TOOL - sent


def test_play_joints_and_sound_each_send_their_op(tmp_path):
    async def go():
        async with connected(tmp_path, grants=("body", "speak")) as (fake, _, session):
            assert not (await session.call_tool("lamp_play", {"name": "wave"})).isError
            assert fake.asked("play") == [{"name": "wave"}]
            joints = await session.call_tool("lamp_joints", {"within_ms": 800, "pitch": 10, "elbow": -5})
            assert not joints.isError, text(joints)
            [asked] = fake.asked("joints")
            # Under a body lease of its own, named in the request.
            assert asked["targets"] == {"pitch": 10, "elbow": -5} and asked["within_ms"] == 800
            assert asked["lease"].startswith("l_")
            none = await session.call_tool("lamp_joints", {"within_ms": 800})
            assert none.isError and "needs at least one of" in text(none)
            assert not (await session.call_tool("lamp_sound", {"name": "hum"})).isError
            assert fake.asked("sound") == [{"name": "hum"}]

    run(go())


def test_body_tools_in_a_row_reuse_the_held_lease_rather_than_be_refused_busy(tmp_path):
    """A move handed to the body leaves its lease to lapse; the next body tool
    renews and uses it, since the lamp refuses a second take even from its
    own holder."""

    async def go():
        async with connected(tmp_path, auto_done=False) as (fake, _, session):
            plain = fake._ok

            async def handed(ws, id_, **fields):
                if any(r["id"] == id_ and r["op"] in ("move", "play") for r in fake.requests):
                    fields["to"] = "body"
                await plain(ws, id_, **fields)

            fake._ok = handed
            frames = [[0, 0, 0, 0, 0], [20, 0, 30, 0, 0], [20, 0, 30, 0, 0], [0, 0, 0, 0, 0]]  # 1.5 s
            for _ in range(2):
                moved = await session.call_tool("lamp_move", {"step_ms": 500, "frames": frames})
                assert not moved.isError, text(moved)
            played = await session.call_tool("lamp_play", {"name": "wave"})
            assert not played.isError, text(played)
            assert len(fake.asked("lease.take")) == 1 and len(fake.asked("lease.renew")) == 2

    run(go())


def test_a_done_that_comes_before_its_answer_is_read_still_ends_the_tool(tmp_path):
    async def go():
        # The fake says `done` right after its answer.
        async with connected(tmp_path, grants=("body",)) as (fake, _, session):
            started = asyncio.get_running_loop().time()
            played = await session.call_tool("lamp_play", {"name": "wave"})
            assert not played.isError and text(played) == "play finished"
            assert asyncio.get_running_loop().time() - started < 5
            assert fake.asked("lease.release") and not fake.leases

    run(go())


def test_a_lamp_that_doesnt_route_them_says_so_as_the_tools_error(tmp_path):
    from lampos.testing import LAMP_CONTROL_REFUSES

    async def go():
        async with connected(tmp_path, grants=("body", "speak"), refuses=LAMP_CONTROL_REFUSES) as (fake, _, session):
            for tool, arguments in (("lamp_play", {"name": "wave"}), ("lamp_joints", {"pitch": 5})):
                result = await session.call_tool(tool, arguments)
                assert result.isError and "refused: not routed yet" in text(result), (tool, text(result))
            # The hum is routed, as lamp-control's `sound` is (#4228).
            assert not (await session.call_tool("lamp_sound", {"name": "hum"})).isError
            assert fake.asked("sound") == [{"name": "hum"}]

    run(go())


def test_lamp_events_gives_what_the_lamp_said_since_a_seq_and_waits_for_one(tmp_path):
    async def go():
        async with connected(tmp_path, grants=("body", "look")) as (fake, _, session):
            first = json.loads(text(await session.call_tool("lamp_events", {})))
            assert first == {"events": [], "latest": 0}
            await until(lambda: "person.arrived" in {e for s in fake.subscribed.values() for e in s})
            await fake.emit("person.arrived", count=1, near=0.3)
            await fake.emit("touch", kind="tap")
            await until(lambda: True)
            said = json.loads(text(await session.call_tool("lamp_events", {"since": 0, "wait_s": 5})))
            while said["latest"] < 2:
                said = json.loads(text(await session.call_tool("lamp_events", {"since": 0, "wait_s": 5})))
            assert [(e["seq"], e["ev"]) for e in said["events"]] == [(1, "person.arrived"), (2, "touch")]
            # Only what came after `since`, and a wait that ends at the next one.
            waiting = asyncio.create_task(session.call_tool("lamp_events", {"since": 2, "wait_s": 10}))
            await asyncio.sleep(0.2)
            assert not waiting.done()
            await fake.emit("person.left", count=0)
            later = json.loads(text(await waiting))
            assert [(e["seq"], e["ev"], e["count"]) for e in later["events"]] == [(3, "person.left", 0)]
            # Nothing new: an empty list at once without wait_s.
            assert json.loads(text(await session.call_tool("lamp_events", {"since": 3})))["events"] == []

    run(go())


def test_lamp_events_never_hears_what_the_grants_dont_allow(tmp_path):
    async def go():
        async with connected(tmp_path, grants=("body",)) as (fake, _, session):
            await session.call_tool("lamp_events", {})
            await until(lambda: any(fake.subscribed.values()))
            heard = {e for s in fake.subscribed.values() for e in s}
            assert "touch" in heard and "button" in heard
            assert not heard & {"person.arrived", "hand", "name.heard", "sound.heard", "phrase.heard"}

    run(go())


def test_a_client_subscribed_to_the_events_is_told_of_each(tmp_path):
    from mcp import types

    told: list[str] = []

    async def notified(message) -> None:
        if isinstance(message, types.ServerNotification) and isinstance(message.root, types.ResourceUpdatedNotification):
            told.append(str(message.root.params.uri))

    async def go():
        async with connected(tmp_path, grants=("body", "look"), message_handler=notified) as (fake, _, session):
            assert session.get_server_capabilities().resources.subscribe is True
            await session.subscribe_resource(AnyUrl("lamp://events"))
            await session.subscribe_resource(AnyUrl("lamp://state/presence"))
            await until(lambda: "person.arrived" in {e for s in fake.subscribed.values() for e in s})
            await fake.emit("person.arrived", count=1, near=0.3)
            await until(lambda: "lamp://state/presence" in told and "lamp://events" in told)
            await fake.emit("touch", kind="tap")
            await until(lambda: told.count("lamp://events") == 2)
            # A touch changes no state.
            assert told.count("lamp://state/presence") == 1
            stream = json.loads((await session.read_resource(AnyUrl("lamp://events"))).contents[0].text)
            assert [e["ev"] for e in stream["events"]] == ["person.arrived", "touch"] and stream["latest"] == 2
            await session.unsubscribe_resource(AnyUrl("lamp://events"))
            await fake.emit("touch", kind="pet")
            await until(lambda: True)
            await asyncio.sleep(0.2)
            assert told.count("lamp://events") == 2

    run(go())


def test_a_look_takes_its_own_lease_and_lets_go_at_done(tmp_path):
    async def go():
        # The test sends the look's `done` itself.
        async with connected(tmp_path, auto_done=False) as (fake, _, session):
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
            # A look waits for its done; a move is handed to the body and ends at once (#3903's fake, as lamp-control).
            call = asyncio.create_task(session.call_tool("lamp_look", {"yaw": 10, "pitch": 0}))
            await until(lambda: fake.asked("look"))
            await fake.lease_event(next(iter(fake.leases)), "lost", why="hand")
            result = await call
            assert result.isError and "lease_lost: hand" in text(result)
            # Lost is gone already: nothing to let go of, and no retry.
            assert not fake.asked("lease.release") and len(fake.asked("look")) == 1

    run(go())


def test_a_lease_whose_action_is_refused_or_never_ends_is_not_listed_held(tmp_path):
    """The Reviewer-lead's P3 on #3802: `lamp://lease` lists only leases
    still held."""
    now = [1000.0]

    async def leases(session) -> list:
        return json.loads((await session.read_resource("lamp://lease")).contents[0].text)

    async def go():
        async with connected(tmp_path, auto_done=False, clock=lambda: now[0]) as (fake, _, session):
            fake.refuse_next("look", "quiet_hours", "it's night")
            result = await session.call_tool("lamp_look", {"at": "ahead"})
            assert result.isError and "quiet_hours" in text(result)
            # Refused: its lease is let go of at once.
            assert len(fake.asked("lease.release")) == 1 and await leases(session) == []
            now[0] += 20  # past the tool's own 10 s wait without a done
            frames = [[0, 0, 0, 0, 0], [20, 0, 30, 0, 0], [20, 0, 30, 0, 0], [0, 0, 0, 0, 0]]  # 1.5 s
            call = asyncio.create_task(session.call_tool("lamp_move", {"step_ms": 500, "frames": frames}))
            await until(lambda: fake.asked("move"))
            assert len(await leases(session)) == 1
            now[0] += 11  # it lapses by itself on the lamp
            assert await leases(session) == []
            call.cancel()

    run(go())


def test_an_action_handed_to_a_part_ends_at_once(tmp_path):
    """blue's P3 on #3813: lamp-control answers `move` with `{"to": "body"}`
    and sends no `done`, so the tool doesn't wait out its lease."""

    async def go():
        async with connected(tmp_path, auto_done=False) as (fake, _, session):
            plain = fake._ok

            async def handed(ws, id_, **fields):
                if any(r["id"] == id_ and r["op"] == "move" for r in fake.requests):
                    fields["to"] = "body"
                await plain(ws, id_, **fields)

            fake._ok = handed
            frames = [[0, 0, 0, 0, 0], [20, 0, 30, 0, 0], [20, 0, 30, 0, 0], [0, 0, 0, 0, 0]]  # 1.5 s
            started = asyncio.get_running_loop().time()
            result = await session.call_tool("lamp_move", {"step_ms": 500, "frames": frames})
            assert not result.isError and text(result).startswith("move taken by the body")
            assert asyncio.get_running_loop().time() - started < 5

    run(go())


def test_a_lamp_without_look_is_not_promised_who_is_there(tmp_path):
    """blue's P3 on #3802."""

    async def go():
        async with connected(tmp_path) as (_, _, session):
            tool = next(t for t in (await session.list_tools()).tools if t.name == "lamp_state")
            assert "who is there" not in tool.description and "presence" not in tool.description
        async with connected(tmp_path / "look", grants=("body", "look")) as (_, _, session):
            tool = next(t for t in (await session.list_tools()).tools if t.name == "lamp_state")
            assert "who is there" in tool.description

    run(go())


def test_a_wait_for_a_phrase_ends_only_on_its_own_phrases(tmp_path):
    """The Reviewer-lead's P2 on #3813: an earlier wait's phrase, still heard,
    never ends a later wait, and no phrase is kept for later subscribes."""

    async def go():
        async with connected(tmp_path, grants=("hear",)) as (fake, lamp, session):
            first = await session.call_tool(
                "lamp_wait_for", {"events": ["phrase.heard"], "phrases": ["please stop now"], "timeout_s": 0.2}
            )
            assert json.loads(text(first)) == {"timed_out": True}
            call = asyncio.create_task(
                session.call_tool("lamp_wait_for", {"events": ["phrase.heard"], "phrases": ["good night"], "timeout_s": 5})
            )
            await until(lambda: any("good night" in ask.get("phrases", []) for ask in fake.asked("subscribe")))
            await fake.emit("phrase.heard", phrase="please stop now")
            await asyncio.sleep(0.2)
            assert not call.done()
            await fake.emit("phrase.heard", phrase="good night")
            assert json.loads(text(await call))["phrase"] == "good night"
            # One subscribe a wait, carrying its phrases: none without them.
            assert all(ask.get("phrases") for ask in fake.asked("subscribe"))

    run(go())


def test_a_wait_cancelled_before_its_subscribe_reply_leaves_nothing(tmp_path):
    """Reviewer-two's P2 on #3813: cancelled while `on()` awaits the reply,
    a wait leaves no handler, no subscription and no recorded event."""

    async def go():
        async with FakeLamp(grants=("hear",)) as fake:
            ring = Keyring(tmp_path)
            await Lamp.pair(fake.code, fake.host, pin=fake.pin, port=fake.port, name="test laptop", keyring=ring)
            async with await Lamp.connect(keyring=ring) as lamp:
                server = build(lamp, client="test laptop")
                plain = fake._ok

                async def slow(ws, id_, **fields):
                    if any(r["id"] == id_ and r["op"] == "subscribe" for r in fake.requests):
                        await asyncio.sleep(0.3)
                    await plain(ws, id_, **fields)

                fake._ok = slow
                for tries in range(1, 4):
                    call = asyncio.create_task(
                        server.call_tool(
                            "lamp_wait_for", {"events": ["phrase.heard"], "phrases": ["please stop now"], "timeout_s": 5}
                        )
                    )
                    await until(lambda: len(fake.asked("subscribe")) >= tries)
                    call.cancel()
                    with contextlib.suppress(asyncio.CancelledError):
                        await call
                    assert not lamp._handlers.get("phrase.heard"), f"after {tries} cancelled"
                await asyncio.sleep(0.5)
                assert len(fake.asked("unsubscribe")) == 3
                await fake.emit("phrase.heard", phrase="please stop now")
                await asyncio.sleep(0.1)
                recent = await server.read_resource("lamp://events/recent")
                assert json.loads(list(recent)[0].content) == []

    run(go())


def test_a_phrase_wait_without_phrases_is_refused(tmp_path):
    """The Reviewer-lead's P3 on #3813, as the lamp refuses it."""

    async def go():
        async with connected(tmp_path, grants=("hear",)) as (fake, _, session):
            result = await session.call_tool("lamp_wait_for", {"events": ["phrase.heard"], "timeout_s": 1})
            assert result.isError and "bad_request: phrase.heard needs the phrases" in text(result)
            assert not fake.asked("subscribe")

    run(go())


def test_a_second_phrase_wait_while_one_runs_is_busy(tmp_path):
    """The Reviewer-lead's P3 on #3813: the lamp keeps one list of phrases a
    session, so a second wait would silence the first."""

    async def go():
        async with connected(tmp_path, grants=("hear",)) as (fake, _, session):
            call = asyncio.create_task(
                session.call_tool("lamp_wait_for", {"events": ["phrase.heard"], "phrases": ["please stop now"], "timeout_s": 5})
            )
            await until(lambda: fake.asked("subscribe"))
            second = await session.call_tool(
                "lamp_wait_for", {"events": ["phrase.heard"], "phrases": ["go to sleep now"], "timeout_s": 1}
            )
            assert second.isError and "busy: " in text(second)
            await fake.emit("phrase.heard", phrase="please stop now")
            assert json.loads(text(await call))["phrase"] == "please stop now"

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
        # No `done` comes, so the look's lease is left held.
        async with connected(tmp_path, auto_done=False) as (fake, _, session):
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


def test_a_lamp_with_its_rates_off_slows_nothing_here():
    """Hello's `limits` with the rates off (the fake's, as lamp-control's):
    only its two guards, neither of which an op counts against here."""
    from lampos.testing import LIMITS

    limits = Limits(LIMITS, clock=lambda: 0.0)
    for _ in range(50):
        for op in ("gesture", "move", "look", "light", "say", "express"):
            limits.take(op)


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


def test_verbose_turns_the_sdks_debug_log_on_and_stdout_stays_the_mcp_stream(monkeypatch, capsys):
    import lamp_mcp.server as server_module

    seen = {}

    async def serve(lamp_id, client):
        import os

        seen["log"] = os.environ.get("LAMPOS_LOG")

    monkeypatch.delenv("LAMPOS_LOG", raising=False)
    monkeypatch.setattr(server_module, "serve", serve)
    server_module.main([])
    assert seen["log"] is None  # off by default
    server_module.main(["--verbose"])
    assert seen["log"] == "debug"
    assert capsys.readouterr().out == ""
EVERY_GRANT = ("body", "light", "speak", "look", "hear", "sound")


def test_an_argument_a_tool_does_not_take_is_refused(tmp_path):
    """`for_s` for `for_ms` was lit for the default 10 s, silently (a P3
    from a check on lamp-a0ae): now it is the tool's error, naming what the
    tool takes, and nothing reaches the lamp. Each schema says so too."""

    async def go():
        async with connected(tmp_path, grants=EVERY_GRANT) as (fake, _, session):
            for tool in (await session.list_tools()).tools:
                assert tool.inputSchema.get("additionalProperties") is False, tool.name
            wrong = await session.call_tool("lamp_light", {"colour": "warm", "for_s": 5})
            assert wrong.isError
            assert text(wrong) == "bad_request: lamp_light takes no for_s; it takes colour, for_ms, level, pattern"
            assert fake.asked("light") == []
            stop = await session.call_tool("lamp_stop", {"now": True})
            assert stop.isError
            assert text(stop) == "bad_request: lamp_stop takes no now; it takes no arguments"
            assert fake.asked("stop") == []
            # What it takes is still taken.
            assert not (await session.call_tool("lamp_light", {"colour": "warm", "for_ms": 5_000})).isError
            assert fake.asked("light") == [{"colour": "warm", "level": 3, "for_ms": 5_000}]

    run(go())


def test_follow_hand_is_offered_only_with_body_and_look(tmp_path):
    async def go():
        async with connected(tmp_path) as (_, _, session):
            assert "lamp_follow_hand" not in {tool.name for tool in (await session.list_tools()).tools}
        async with connected(tmp_path / "look", grants=("body", "look")) as (_, _, session):
            assert "lamp_follow_hand" in {tool.name for tool in (await session.list_tools()).tools}

    run(go())


def readme_rows() -> dict[str, list[list[str]]]:
    """The README's tools table: each tool's rows, cells stripped."""
    tools: dict[str, list[list[str]]] = {}
    current = None
    lines = (Path(__file__).parents[1] / "README.md").read_text().splitlines()
    start = lines.index("| tool | parameter | type | allowed | default | grant | lease |")
    for line in lines[start + 2 :]:
        if not line.startswith("|"):
            break
        cells = [cell.strip() for cell in line.strip("|").split("|")]
        if cells[0]:
            current = cells[0].strip("`")
            tools[current] = []
        tools[current].append(cells)
    return tools


def test_the_readme_gives_every_tool_its_parameters_defaults_and_every_resource(tmp_path):
    """The README's table is the server's own: every tool, each of its
    parameters, a number default as the server has it, and every resource."""

    async def go():
        rows = readme_rows()
        readme = (Path(__file__).parents[1] / "README.md").read_text()
        async with connected(tmp_path, grants=EVERY_GRANT) as (_, _, session):
            listed = (await session.list_tools()).tools
            assert {tool.name for tool in listed} == ALL | FOLLOW == set(rows)
            for tool in listed:
                by_parameter = {row[1].strip("`"): row for row in rows[tool.name]}
                properties = tool.inputSchema.get("properties", {})
                if not properties:
                    assert list(by_parameter) == ["none"], tool.name
                assert set(properties) <= set(by_parameter), (tool.name, sorted(properties))
                for name, schema_of in properties.items():
                    default = schema_of.get("default")
                    if isinstance(default, (int, float)) and not isinstance(default, bool):
                        written = by_parameter[name][4].replace(",", "")
                        assert f"{default:g}" in written, (tool.name, name, default, written)
            for resource in (await session.list_resources()).resources:
                assert f"| `{resource.uri}` |" in readme, resource.uri

    run(go())


def test_follow_hand_follows_until_the_hand_is_gone_and_says_only_where_it_was(tmp_path):
    from lampos.testing import hand_events, hand_track

    async def go():
        async with connected(tmp_path, grants=("body", "look")) as (fake, _, session):
            call = asyncio.create_task(session.call_tool("lamp_follow_hand", {"seconds": 10}))
            await until(lambda: fake.following)
            track = hand_track("across", seconds=0.5)
            await fake.emit_hand_track(track)
            result = await call
            assert not result.isError
            said = json.loads(text(result))
            told = [event for _, event in hand_events(track) if event["seen"]]
            assert (said["how"], said["why"]) == ("finished", "the hand is gone")
            assert said["hands_seen"] == len(told)
            assert said["last"] == {k: told[-1][k] for k in ("x", "y", "size", "open")}
            assert fake.asked("look") == [{"at": "hand"}]
            await until(lambda: not fake.leases)
            assert fake.subscribed and all("hand" not in events for events in fake.subscribed.values())

    run(go())


def test_a_call_missing_what_a_tool_needs_is_said_in_the_lamps_form(tmp_path):
    """Not the validator's "1 validation error for lamp_lightArguments …":
    the tool's error names what it needs, and nothing reaches the lamp."""

    async def go():
        async with connected(tmp_path, grants=EVERY_GRANT) as (fake, _, session):
            missing = await session.call_tool("lamp_light", {"level": 2})
            assert missing.isError
            assert text(missing) == "bad_request: lamp_light needs colour"
            moved = await session.call_tool("lamp_move", {})
            assert text(moved) == "bad_request: lamp_move needs step_ms, frames"
            assert fake.asked("light") == [] and fake.asked("move") == []

    run(go())


def test_follow_hand_ends_when_its_time_is_up_or_at_stop(tmp_path):
    async def go():
        async with connected(tmp_path, grants=("body", "look")) as (fake, _, session):
            said = json.loads(text(await session.call_tool("lamp_follow_hand", {"seconds": 0.2})))
            assert (said["how"], said["why"], said["hands_seen"], said["last"]) == ("finished", "time up", 0, None)
            assert not fake.leases
            call = asyncio.create_task(session.call_tool("lamp_follow_hand", {"seconds": 10}))
            await until(lambda: fake.following)
            assert not (await session.call_tool("lamp_stop", {})).isError
            said = json.loads(text(await call))
            assert (said["how"], said["why"]) == ("finished", "stopped")
            assert not fake.leases and not fake.following

    run(go())


def test_server_info_says_lamp_mcps_own_version(tmp_path):
    """`serverInfo.version` is lamp-mcp's, not the MCP library's."""
    from importlib.metadata import version

    async def go():
        async with FakeLamp(grants=("light",)) as fake:
            ring = Keyring(tmp_path)
            await Lamp.pair(fake.code, fake.host, pin=fake.pin, port=fake.port, name="test laptop", keyring=ring)
            async with await Lamp.connect(keyring=ring) as lamp:
                server = build(lamp, client="test laptop")
                options = server._mcp_server.create_initialization_options()
                assert options.server_name == "lamp"
                assert options.server_version == version("lamp-mcp") != version("mcp")

    run(go())


def test_wait_for_hears_a_hand_with_look(tmp_path):
    from lampos.testing import hand_track

    async def go():
        async with connected(tmp_path, grants=("body", "look")) as (fake, _, session):
            call = asyncio.create_task(session.call_tool("lamp_wait_for", {"events": ["hand"], "timeout_s": 5}))
            await until(lambda: any("hand" in events for events in fake.subscribed.values()))
            await fake.emit_hand_track(hand_track("still", seconds=0.1))
            event = json.loads(text(await call))
            assert event["ev"] == "hand" and event["seen"] and set(event) >= {"x", "y", "size", "open", "confidence"}
        async with connected(tmp_path / "nolook") as (_, _, session):
            assert (await session.call_tool("lamp_wait_for", {"events": ["hand"]})).isError

    run(go())


def test_a_required_argument_given_as_null_is_missing(tmp_path):
    """harness-reviewer's P3 on #4215: `colour: null` is refused in the
    lamp's form too, never in the validator's words."""

    async def go():
        async with connected(tmp_path, grants=EVERY_GRANT) as (fake, _, session):
            refused = await session.call_tool("lamp_light", {"colour": None, "level": 2})
            assert refused.isError
            assert text(refused) == "bad_request: lamp_light needs colour"
            assert fake.asked("light") == []

    run(go())


def test_a_follow_stop_while_a_follow_starts_ends_it_once_started(tmp_path):
    async def go():
        async with connected(tmp_path, grants=("body", "look")) as (fake, _, session):
            plain = fake._ok

            async def slow(ws, re, **fields):
                await asyncio.sleep(0.3)
                await plain(ws, re, **fields)

            fake._ok = slow
            started = asyncio.get_running_loop().time()
            call = asyncio.create_task(session.call_tool("lamp_follow_hand", {"seconds": 30}))
            await until(lambda: fake.asked("lease.take"))
            assert text(await session.call_tool("lamp_follow_stop", {})) == "stopped following"
            said = json.loads(text(await call))
            assert (said["how"], said["why"]) == ("finished", "stopped")
            assert asyncio.get_running_loop().time() - started < 5
            assert not fake.leases and not fake.following

    run(go())


def test_a_second_follow_is_busy_and_follow_stop_ends_the_first(tmp_path):
    async def go():
        async with connected(tmp_path, grants=("body", "look")) as (fake, _, session):
            assert text(await session.call_tool("lamp_follow_stop", {})) == "not following"
            call = asyncio.create_task(session.call_tool("lamp_follow_hand", {"seconds": 30}))
            await until(lambda: fake.following)
            lease = next(iter(fake.leases))
            listed = await session.read_resource("lamp://lease")
            assert lease in listed.contents[0].text
            second = await session.call_tool("lamp_follow_hand", {"seconds": 5})
            assert second.isError and "busy: already following a hand" in text(second)
            assert len(fake.asked("look")) == 1 and len(fake.asked("lease.take")) == 1
            assert text(await session.call_tool("lamp_follow_stop", {})) == "stopped following"
            said = json.loads(text(await call))
            assert (said["how"], said["why"]) == ("finished", "stopped")
            assert not fake.leases and not fake.following
            assert lease not in (await session.read_resource("lamp://lease")).contents[0].text
            # Free again once it ended.
            again = asyncio.create_task(session.call_tool("lamp_follow_hand", {"seconds": 0.2}))
            assert not (await again).isError

    run(go())


GRANT_WORDS = {"body", "light", "speak", "look", "hear", "sound"}


def readme_grants(cell: str) -> set[str]:
    """The grants a README grant cell says the tool is offered with: its
    words before any parenthesis; "none" and "the event's" need none."""
    if cell.startswith(("none", "the event's")):
        # Offered with none: `lamp_wait_for` asks each event's own grant.
        return set()
    head = cell.split("(")[0]
    return {word.strip(" ,`") for word in head.split()} & GRANT_WORDS


def test_the_readme_grant_column_is_the_servers_own(tmp_path):
    """harness-reviewer's P3 on #4215: each tool is offered with exactly the
    grants its README row names, and not with any one of them missing."""
    rows = readme_rows()

    async def offered(folder, grants) -> set[str]:
        async with connected(folder, grants=tuple(grants)) as (_, _, session):
            return {tool.name for tool in (await session.list_tools()).tools}

    async def go():
        for n, (tool, tool_rows) in enumerate(sorted(rows.items())):
            needs = readme_grants(tool_rows[0][5])
            assert tool in await offered(tmp_path / f"{n}-all", needs), (tool, needs)
            for m, grant in enumerate(sorted(needs)):
                without = await offered(tmp_path / f"{n}-{m}", needs - {grant})
                assert tool not in without, (tool, grant)

    run(go())


def test_the_readme_lease_column_is_the_servers_own(tmp_path):
    """A tool the README says takes a lease takes a body lease when called;
    one it says doesn't, doesn't."""
    rows = readme_rows()
    calls = {
        "lamp_look": {"at": "ahead"},
        "lamp_move": {"step_ms": 100, "frames": [[0, 0, 0, 0, 0], [5, 0, 0, 0, 0], [0, 0, 0, 0, 0]]},
        "lamp_gesture": {"name": "nod"},
        "lamp_light": {"colour": "warm"},
        "lamp_express": {"feeling": "glad"},
        "lamp_say": {"text": "Hello."},
        "lamp_state": {},
    }

    async def go():
        for n, (tool, arguments) in enumerate(calls.items()):
            takes = rows[tool][0][6].startswith("yes")
            async with connected(tmp_path / str(n), grants=EVERY_GRANT) as (fake, _, session):
                await session.call_tool(tool, arguments)
                assert bool(fake.asked("lease.take")) == takes, (tool, takes)

    run(go())


def test_a_follow_refused_or_not_followed_lets_go_of_its_lease(tmp_path):
    async def go():
        async with connected(tmp_path, grants=("body", "look"), follows_hand=False) as (fake, _, session):
            result = await session.call_tool("lamp_follow_hand", {"seconds": 5})
            assert result.isError and "refused: this lamp doesn't follow a hand yet" in text(result)
            assert not fake.leases and fake.asked("lease.release")
        async with connected(tmp_path / "2", grants=("body", "look")) as (fake, _, session):
            fake.refuse_next("look", "busy", "another client holds it")
            result = await session.call_tool("lamp_follow_hand", {"seconds": 5})
            assert result.isError and "busy: another client holds it" in text(result)
            assert not fake.leases and fake.asked("lease.release")
            # A refused follow leaves nothing running: the next one starts.
            assert text(await session.call_tool("lamp_follow_stop", {})) == "not following"

    run(go())
