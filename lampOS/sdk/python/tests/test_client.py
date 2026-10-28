"""The SDK against the fake lamp: pairing, hello, events, actions, leases and
refusals, as docs/control.md says them. No lamp needed."""

from __future__ import annotations

import asyncio
import base64
import hashlib
import hmac
import json
import stat
from pathlib import Path

import pytest

from lampos import Keyring, Lamp, LampError, NotPaired, Subscription, WrongLamp, errors, schema
from lampos.testing import FakeLamp

REPO_SCHEMA = Path(__file__).resolve().parents[3] / "docs" / "control.schema.json"


def run(coroutine):
    return asyncio.run(asyncio.wait_for(coroutine, timeout=20))


async def paired(fake: FakeLamp, folder: Path) -> Keyring:
    ring = Keyring(folder)
    await Lamp.pair(fake.code, fake.host, pin=fake.pin, port=fake.port, name="test laptop", keyring=ring)
    return ring


def test_the_packaged_schema_is_the_repositorys():
    # Outside the repository (an installed copy) there is nothing to compare;
    # the lampOS SDK job in CI runs it in the repository.
    if not REPO_SCHEMA.exists():
        pytest.skip("docs/control.schema.json is not beside this copy")
    assert json.loads(REPO_SCHEMA.read_text()) == schema.schema()


def test_proofs_name_what_they_are_for():
    from lampos.keys import HELLO, PAIR, proof

    assert (PAIR, HELLO) == (b"lampos/v1/pair\0", b"lampos/v1/hello\0")
    key, nonce = bytes(32), bytes(range(32))
    assert proof("12345678", bytes(32), key, nonce) != hmac.new(b"12345678", key + nonce, hashlib.sha256).digest()


def test_every_op_has_a_call_and_every_error_code_a_class():
    calls = {"lease.take", "lease.renew", "lease.release", "pair", "hello", "subscribe", "unsubscribe", "get"}
    named = {"skill.push": "push_skill", "skill.list": "skills", "skill.remove": "remove_skill"}
    for op in schema.ops():
        assert op in calls or callable(getattr(Lamp, named.get(op, op), None)), op
    for code in schema.error_codes():
        assert errors.BY_CODE[code].code == code
    assert errors.from_reply({"code": "rate", "why": "x", "retry_ms": 900}).retry_ms == 900
    assert isinstance(errors.from_reply({"code": "no_grant", "why": "x"}), errors.BY_CODE["no_grant"])


def test_pairing_keeps_the_key_private_and_the_lamp_pinned(tmp_path):
    async def go():
        async with FakeLamp(grants=("body", "light")) as fake:
            ring = await paired(fake, tmp_path)
            pairing = ring.pairing()
            assert (pairing.lamp, pairing.fp, pairing.grants) == ("lamp-fake", fake.pin, ["body", "light"])
            assert fake.clients[pairing.client]["name"] == "test laptop"

    run(go())
    assert stat.S_IMODE((tmp_path / "key").stat().st_mode) == 0o600
    assert "12345678" not in (tmp_path / "lamps.json").read_text()


def test_a_wrong_code_is_refused_and_nothing_is_kept(tmp_path):
    async def go():
        async with FakeLamp() as fake:
            with pytest.raises(errors.BY_CODE["unpaired"]):
                await Lamp.pair("87654321", fake.host, pin=fake.pin, port=fake.port, keyring=Keyring(tmp_path))
            with pytest.raises(LampError):
                await Lamp.pair("1234", fake.host, pin=fake.pin, port=fake.port, keyring=Keyring(tmp_path))

    run(go())
    assert Keyring(tmp_path).pairings() == {}


def test_another_certificate_than_the_pinned_one_is_refused(tmp_path):
    async def go():
        async with FakeLamp() as fake:
            # Another certificate than the pin: refused before any frame is sent.
            with pytest.raises(WrongLamp):
                await Lamp.pair(fake.code, fake.host, pin="0" * 64, port=fake.port, keyring=Keyring(tmp_path))
            # No pin, or a short one (mDNS's hint): no pairing, and no connection.
            for pin in ("", fake.fp):
                with pytest.raises(WrongLamp):
                    await Lamp.pair(fake.code, fake.host, pin=pin, port=fake.port, keyring=Keyring(tmp_path))
            assert fake.received == []
            ring = await paired(fake, tmp_path)
        # The same lamp with a new certificate (a reflashed lamp, or another
        # machine at its address) is not trusted.
        async with FakeLamp() as other:
            pairing = ring.pairing()
            pairing.port = other.port
            ring.remember(pairing)
            with pytest.raises(WrongLamp):
                await Lamp.connect(keyring=ring)

    run(go())


def test_connect_without_a_pairing_says_so(tmp_path):
    with pytest.raises(NotPaired):
        run(Lamp.connect(keyring=Keyring(tmp_path)))


def test_hello_with_another_computers_key_is_unpaired(tmp_path):
    async def go():
        async with FakeLamp() as fake:
            ring = await paired(fake, tmp_path / "a")
            stranger = Keyring(tmp_path / "b")
            stranger.remember(ring.pairing())
            with pytest.raises(errors.BY_CODE["unpaired"]):
                await Lamp.connect(keyring=stranger)

    run(go())


def test_greet_a_person_who_arrives(tmp_path):
    async def go():
        async with FakeLamp(grants=("body", "look")) as fake:
            ring = await paired(fake, tmp_path)
            async with await Lamp.connect(keyring=ring) as lamp:
                greeted = asyncio.Event()

                async def greet(event):
                    await lamp.gesture("greeting")
                    greeted.set()

                lamp.on("person.arrived", greet)
                while not fake.asked("subscribe"):
                    await asyncio.sleep(0.01)
                await fake.emit("person.arrived", count=1, near=0.3)
                await greeted.wait()
            assert fake.asked("gesture") == [{"name": "greeting"}]

    run(go())


def test_phrases_go_with_their_subscription_and_events_carry_only_the_phrase(tmp_path):
    async def go():
        async with FakeLamp(grants=("body", "light", "speak", "hear")) as fake:
            ring = await paired(fake, tmp_path)
            async with await Lamp.connect(keyring=ring) as lamp:
                heard = asyncio.Queue()
                lamp.on("phrase.heard", heard.put_nowait, phrases=["good night lamp"])
                while not fake.asked("subscribe"):
                    await asyncio.sleep(0.01)
                assert fake.asked("subscribe") == [{"events": ["phrase.heard"], "phrases": ["good night lamp"]}]
                await fake.emit("phrase.heard", phrase="good night lamp")
                assert (await heard.get())["phrase"] == "good night lamp"

    run(go())


# The lamp's floor and its words (lamp-control's PHRASE_MIN_LETTERS, #3901),
# with the phrases Fourteen tried there: a short one never leaves the computer.
SHORT_PHRASES = ["is a", "it is", "ok lamp", "you are", "i want to", "t\u00f4i mu\u1ed1n"]
# The Hindi lines have vowel signs, which the lamp counts as letters: the
# SDK must never be stricter than the lamp (Nine's and harness-reviewer's P2).
LONG_PHRASES = ["thank you", "what is the", "i don't know", "t\u00f4i kh\u00f4ng", "12345678", "नमस्ते दोस्त", "सुनो लैंप"]


def test_a_phrase_under_the_lamps_floor_is_refused_before_sending(tmp_path):
    async def go():
        async with FakeLamp(grants=("body", "light", "speak", "hear")) as fake:
            ring = await paired(fake, tmp_path)
            async with await Lamp.connect(keyring=ring) as lamp:
                for phrase in SHORT_PHRASES:
                    with pytest.raises(errors.BY_CODE["bad_request"]) as raised:
                        lamp.on("phrase.heard", print, phrases=[phrase, "good night lamp"])
                    assert raised.value.why == f'phrases: "{phrase}" is too short: 8 letters at least'
                    with pytest.raises(errors.BY_CODE["bad_request"]):
                        await lamp.subscribe(["phrase.heard"], [phrase])
                assert fake.asked("subscribe") == []
                await lamp.subscribe(["phrase.heard"], LONG_PHRASES)
                assert fake.asked("subscribe") == [{"events": ["phrase.heard"], "phrases": LONG_PHRASES}]
                # The refused on() kept no handler and no phrase.
                assert lamp._phrases == [] and not lamp._handlers.get("phrase.heard")

    run(go())


def test_the_fake_answers_express_as_lamp_control_does(tmp_path):
    # blue's P2 on #4020: the skill's three feelings, each its move, and any
    # other name bad_request.
    async def go():
        async with FakeLamp() as fake:
            ring = await paired(fake, tmp_path)
            async with await Lamp.connect(keyring=ring) as lamp:
                answer = await lamp.express("glad")
                assert (answer["to"], answer["move"], answer["light"]) == ("body", "glad-lift", True)
                assert (await lamp.express("glad", 0.25))["move"] == "glad-lift-50"
                assert (await lamp.express("low", 0.25))["move"] == "low-droop-75"
                assert (await lamp.express("glad", 0.0625))["move"] == "glad-lift-25"
                with pytest.raises(errors.BY_CODE["bad_request"]) as raised:
                    await lamp.express("happy")
                assert raised.value.why == "feeling: one of glad, lively, low"

    run(go())


def test_actions_send_the_schemas_fields_and_wait_for_done(tmp_path):
    async def go():
        async with FakeLamp() as fake:
            ring = await paired(fake, tmp_path)
            async with await Lamp.connect(keyring=ring) as lamp:
                await lamp.light("warm", 3, 10_000, pattern="breathe")
                await lamp.say("Build passed.")
                await lamp.express("glad")
                taken = await lamp.gesture("nod", wait=True)
                assert taken["to"] == "body"
                await lamp.stop()
            assert fake.asked("light") == [{"colour": "warm", "level": 3, "pattern": "breathe", "for_ms": 10000}]
            assert fake.asked("say") == [{"text": "Build passed."}]
            assert fake.asked("express") == [{"feeling": "glad"}]
            assert fake.asked("stop") == [{}]

    run(go())


def test_names_outside_the_schema_never_leave_this_computer(tmp_path):
    async def go():
        async with FakeLamp() as fake:
            ring = await paired(fake, tmp_path)
            async with await Lamp.connect(keyring=ring) as lamp:
                for call in (lamp.gesture("scanning"), lamp.gesture("idle"), lamp.light("magenta", 3, 1000)):
                    with pytest.raises(errors.LampError) as refused:
                        await call
                    assert refused.value.code == "bad_request"
                with pytest.raises(LampError):
                    lamp.on("camera.frame", print)
            assert not [frame for frame in fake.received if frame.get("op") in ("gesture", "light")]

    run(go())


def test_a_move_needs_a_body_lease_which_renews_itself_and_is_released(tmp_path):
    async def go():
        async with FakeLamp() as fake:
            ring = await paired(fake, tmp_path)
            async with await Lamp.connect(keyring=ring) as lamp:
                with pytest.raises(errors.BY_CODE["no_lease"]):
                    await lamp.look(yaw=-10, pitch=5)
                async with lamp.lease("body", for_ms=1000, why="test") as lease:
                    assert lease.held
                    await lamp.look(yaw=-10, pitch=5)
                    await lamp.joints(lease, 300, pitch=4.0, yaw=-12.0)
                    await asyncio.sleep(1.2)
                    assert len(fake.asked("lease.renew")) >= 2
                assert fake.leases == {}
            assert fake.asked("look") == [{"at": {"yaw": -10, "pitch": 5}}]
            assert fake.asked("joints") == [{"lease": "l_1", "targets": {"pitch": 4.0, "yaw": -12.0}, "within_ms": 300}]
            assert fake.asked("lease.release") == [{"lease": "l_1"}]

    run(go())


def test_a_lease_the_lamp_takes_back_is_lost_with_its_why(tmp_path):
    async def go():
        async with FakeLamp() as fake:
            ring = await paired(fake, tmp_path)
            async with await Lamp.connect(keyring=ring) as lamp:
                async with lamp.lease("body", for_ms=1000) as lease:
                    await fake.lose_lease(lease.id, "hand")
                    while lease.state != "lost":
                        await asyncio.sleep(0.01)
                    assert lease.lost_why == "hand"
                    with pytest.raises(errors.BY_CODE["no_lease"]):
                        await lamp.move([[0, 0, 0, 0, 0], [20, 0, 0, 0, 0], [0, 0, 0, 0, 0]], 40)
                assert fake.asked("lease.release") == []

    run(go())


def test_refusals_are_typed_and_say_when_to_try_again(tmp_path):
    async def go():
        async with FakeLamp(grants=("body",)) as fake:
            ring = await paired(fake, tmp_path)
            async with await Lamp.connect(keyring=ring) as lamp:
                with pytest.raises(errors.BY_CODE["no_grant"]):
                    await lamp.say("hello")
                fake.refuse_next("gesture", "rate", "1 every 2 s", retry_ms=1500)
                with pytest.raises(errors.BY_CODE["rate"]) as refused:
                    await lamp.gesture("nod")
                assert refused.value.retry_ms == 1500
                fake.refuse_next("gesture", "private", "the privacy switch is closed")
                with pytest.raises(errors.BY_CODE["private"]):
                    await lamp.gesture("nod")

    run(go())


def test_get_answers_the_states_asked(tmp_path):
    async def go():
        async with FakeLamp(grants=("look",)) as fake:
            fake.state["presence"] = {"count": 2, "near": 0.4, "facing": True}
            ring = await paired(fake, tmp_path)
            async with await Lamp.connect(keyring=ring) as lamp:
                state = await lamp.get("presence", "lamp")
            assert state["presence"]["count"] == 2 and state["lamp_state"]["private"] is False

    run(go())


def test_the_examples_compile_and_call_only_what_the_client_has():
    import ast

    examples = sorted((Path(__file__).resolve().parents[1] / "examples").glob("*.py"))
    assert len(examples) >= 5
    for example in examples:
        tree = ast.parse(example.read_text(), str(example))
        for node in ast.walk(tree):
            if isinstance(node, ast.Attribute) and isinstance(node.value, ast.Name) and node.value.id == "lamp":
                assert hasattr(Lamp, node.attr), f"{example.name}: lamp.{node.attr}"


def test_off_takes_a_handler_off_and_unsubscribes_the_last(tmp_path):
    async def go():
        async with FakeLamp() as fake:
            ring = await paired(fake, tmp_path)
            async with await Lamp.connect(keyring=ring) as lamp:
                seen = []
                first, second = (lambda e: seen.append(1)), (lambda e: seen.append(2))
                lamp.on("touch", first)
                lamp.on("touch", second)
                while len(fake.asked("subscribe")) < 2:
                    await asyncio.sleep(0.01)
                lamp.off("touch", first)
                await fake.emit("touch", kind="pet")
                while not seen:
                    await asyncio.sleep(0.01)
                assert seen == [2] and fake.asked("unsubscribe") == []
                lamp.off("touch", second)
                while not fake.asked("unsubscribe"):
                    await asyncio.sleep(0.01)
                assert fake.asked("unsubscribe") == [{"events": ["touch"]}]

    run(go())


def test_the_fake_says_done_and_lease_changes_to_whoever_they_are_for(tmp_path):
    async def go():
        async with FakeLamp(auto_done=False) as fake:
            ring = await paired(fake, tmp_path)
            async with await Lamp.connect(keyring=ring) as lamp:
                async with lamp.lease("body", for_ms=5000) as lease:
                    waiting = asyncio.ensure_future(lamp.play("wave", wait=True))
                    while not fake.asked("play"):
                        await asyncio.sleep(0.01)
                    await fake.lease_event(lease.id, "suspended", "conversation")
                    while lease.state != "suspended":
                        await asyncio.sleep(0.01)
                    await fake.done(fake.requests[-1]["id"], "cut_short", "conversation")
                    assert (await waiting)["how"] == "cut_short"
                    await fake.lease_event(lease.id, "held")
                    while not lease.held:
                        await asyncio.sleep(0.01)

    run(go())


def test_the_proof_holds_the_pin_so_a_relayed_one_fails_at_the_lamp():
    from lampos.keys import PAIR, proof

    key, nonce = bytes(32), bytes(range(32))
    pin, other = bytes([1]) * 32, bytes([2]) * 32
    assert proof("12345678", pin, key, nonce) == hmac.new(b"12345678", PAIR + pin + key + nonce, hashlib.sha256).digest()
    assert proof("12345678", pin, key, nonce) != proof("12345678", other, key, nonce)


def test_a_welcome_nonce_not_32_bytes_is_refused():
    from lampos.keys import unb64

    with pytest.raises(ValueError):
        unb64("AAAA", 32)
    with pytest.raises(ValueError):
        unb64("A" * 43 + "!", 32)
    assert unb64("A" * 43 + "=", 32) == bytes(32)


def test_the_pairing_store_is_this_users_alone_whatever_the_umask(tmp_path):
    import os

    old = os.umask(0o002)
    try:
        folder = tmp_path / "lampos"
        ring = Keyring(folder)
        ring.public()
        from lampos.keys import Pairing

        ring.remember(Pairing(lamp="l", client="c_1", grants=[], fp="0" * 64, host="h", port=1))
        # A folder made group-writable before is closed again.
        os.chmod(folder, 0o775)
        ring.remember(Pairing(lamp="l", client="c_1", grants=[], fp="1" * 64, host="h", port=1))
    finally:
        os.umask(old)
    assert stat.S_IMODE(folder.stat().st_mode) == 0o700
    assert stat.S_IMODE((folder / "key").stat().st_mode) == 0o600
    assert stat.S_IMODE((folder / "lamps.json").stat().st_mode) == 0o600


def test_lamp_fake_pairs_here_and_says_an_arrival(tmp_path, monkeypatch):
    import argparse

    from lampos import cli

    monkeypatch.setenv("LAMPOS_HOME", str(tmp_path))

    async def go():
        faking = asyncio.ensure_future(cli._fake(argparse.Namespace(every=1.5, cycles=2)))
        while not (tmp_path / "lamps.json").exists():
            await asyncio.sleep(0.02)
        async with await Lamp.connect() as lamp:
            assert set(lamp.grants) == set(schema.grants())
            arrived = asyncio.Event()
            lamp.on("person.arrived", lambda event: arrived.set())
            await arrived.wait()
        assert await faking == 0

    run(go())


def test_a_connection_that_only_listens_pings_so_the_lamp_keeps_it_open(tmp_path, monkeypatch):
    from lampos import client

    monkeypatch.setattr(client, "KEEPALIVE_S", 0.05)

    async def go():
        async with FakeLamp() as fake:
            ring = await paired(fake, tmp_path)
            async with await Lamp.connect(keyring=ring) as lamp:
                lamp.on("person.arrived", lambda event: None)
                # The latency is measured from a ping's pong: 0 until one came back.
                while lamp._ws.latency == 0:
                    await asyncio.sleep(0.02)

    run(go())


def test_a_refused_subscription_is_raised_never_lost(tmp_path):
    async def go():
        async with FakeLamp(grants=("body",)) as fake:
            ring = await paired(fake, tmp_path)
            async with await Lamp.connect(keyring=ring) as lamp:
                # Awaited: the lamp's refusal, typed.
                with pytest.raises(errors.BY_CODE["no_grant"]):
                    await lamp.on("person.arrived", lambda event: None)
                assert "person.arrived" not in lamp._handlers, "a refused on() keeps no handler"
            async with await Lamp.connect(keyring=ring) as lamp:
                # Not awaited, as a plain listener writes it: wait_closed
                # raises it rather than waiting forever.
                lamp.on("person.arrived", lambda event: None)
                with pytest.raises(errors.BY_CODE["no_grant"]):
                    await asyncio.wait_for(lamp.wait_closed(), timeout=5)

    run(go())


def test_an_awaited_refusal_handled_by_the_caller_is_not_raised_again(tmp_path):
    async def go():
        async with FakeLamp(grants=("body",)) as fake:
            ring = await paired(fake, tmp_path)
            async with await Lamp.connect(keyring=ring) as lamp:
                try:
                    await lamp.on("person.arrived", lambda event: None)
                except errors.BY_CODE["no_grant"]:
                    await lamp.on("touch", lambda event: None)
                # Still listening: wait_closed waits for the close.
                with pytest.raises(asyncio.TimeoutError):
                    await asyncio.wait_for(lamp.wait_closed(), timeout=0.5)

    run(go())


def test_a_subscription_cut_by_a_closed_connection_is_no_refusal(tmp_path):
    async def go():
        async with FakeLamp() as fake:
            ring = await paired(fake, tmp_path)
            async with await Lamp.connect(keyring=ring) as lamp:
                cut = asyncio.get_running_loop().create_future()
                cut.set_exception(LampError("the connection to the lamp closed", code="closed"))
                lamp._subscribed(Subscription(cut), "touch", lambda event: None)
                assert lamp._refusals == []

    run(go())


def test_a_refused_waiting_action_leaves_nothing_behind(tmp_path):
    async def go():
        async with FakeLamp(grants=("light",)) as fake:
            ring = await paired(fake, tmp_path)
            async with await Lamp.connect(keyring=ring) as lamp:
                with pytest.raises(errors.BY_CODE["no_grant"]):
                    await lamp.gesture("nod", wait=True)
                assert lamp._done == {}

    run(go())


def test_a_cancelled_wait_closed_leaves_no_wait_running(tmp_path):
    async def go():
        async with FakeLamp() as fake:
            ring = await paired(fake, tmp_path)
            async with await Lamp.connect(keyring=ring) as lamp:
                before = len(asyncio.all_tasks())
                for _ in range(8):
                    waiting = asyncio.ensure_future(lamp.wait_closed())
                    await asyncio.sleep(0.01)
                    waiting.cancel()
                    with pytest.raises(asyncio.CancelledError):
                        await waiting
                await asyncio.sleep(0)
                assert len(asyncio.all_tasks()) == before

    run(go())


def test_a_refusal_awaited_after_it_came_is_the_callers_alone(tmp_path):
    async def go():
        async with FakeLamp(grants=("body",)) as fake:
            ring = await paired(fake, tmp_path)
            async with await Lamp.connect(keyring=ring) as lamp:
                subscription = lamp.on("person.arrived", lambda event: None)
                # The refusal comes, and is kept for wait_closed, before the await.
                for _ in range(200):
                    if lamp._refusals:
                        break
                    await asyncio.sleep(0.01)
                assert subscription.done() and lamp._refusals
                with pytest.raises(errors.BY_CODE["no_grant"]):
                    await subscription
                with pytest.raises(asyncio.TimeoutError):
                    await asyncio.wait_for(lamp.wait_closed(), timeout=0.5)

    run(go())


def test_a_waiting_gesture_or_move_returns_at_the_bodys_answer(tmp_path):
    async def go():
        # lamp-control's reply shape: `{"re":N,"ok":true,"to":"body"}` and no
        # `done` after it; auto_done off, so nothing else ever comes.
        async with FakeLamp(auto_done=False) as fake:
            ring = await paired(fake, tmp_path)
            async with await Lamp.connect(keyring=ring) as lamp:
                async with lamp.lease("body", for_ms=5000):
                    nod = await asyncio.wait_for(lamp.gesture("nod", wait=True), timeout=2)
                    move = await asyncio.wait_for(lamp.move([[0, 0, 0, 0, 0], [20, 0, 0, 0, 0]], 40, wait=True), timeout=2)
                assert (nod["ok"], nod["to"]) == (True, "body")
                assert (move["ok"], move["to"]) == (True, "body")
                assert lamp._done == {}

    run(go())


def test_every_refusal_nobody_awaited_reaches_wait_closed_in_turn(tmp_path):
    async def go():
        # The reviewer-lead's case on #3829: two refused, one awaited and
        # handled; the other still comes out of wait_closed, never lost.
        async with FakeLamp(grants=("body",)) as fake:
            ring = await paired(fake, tmp_path)
            async with await Lamp.connect(keyring=ring) as lamp:
                a = lamp.on("person.arrived", lambda event: None)
                b = lamp.on("sound.heard", lambda event: None)
                for _ in range(200):
                    if len(lamp._refusals) == 2:
                        break
                    await asyncio.sleep(0.01)
                with pytest.raises(errors.BY_CODE["no_grant"]):
                    await a
                with pytest.raises(errors.BY_CODE["no_grant"]) as raised:
                    await asyncio.wait_for(lamp.wait_closed(), timeout=2)
                assert raised.value is b.task.exception()
                # Both are spent: now it waits for the close.
                with pytest.raises(asyncio.TimeoutError):
                    await asyncio.wait_for(lamp.wait_closed(), timeout=0.3)

    run(go())


def test_wait_closed_woken_by_a_refusal_taken_elsewhere_keeps_waiting(tmp_path):
    async def go():
        async with FakeLamp() as fake:
            ring = await paired(fake, tmp_path)
            async with await Lamp.connect(keyring=ring) as lamp:
                waiting = asyncio.ensure_future(lamp.wait_closed())
                await asyncio.sleep(0.01)
                # A refusal is kept, waking wait_closed; in the same turn an
                # await takes it back, before wait_closed runs again (the
                # reviewer-lead's P3 on #3829). The list says, not the wake.
                lamp._refusals.append(LampError("not granted", code="no_grant"))
                lamp._refused.set()
                lamp._refusals.clear()
                await asyncio.sleep(0.2)
                assert not waiting.done(), "wait_closed returned as if the lamp had closed"
                waiting.cancel()

    run(go())


def test_a_waiting_gesture_keeps_no_done(tmp_path):
    async def go():
        async with FakeLamp(auto_done=False) as fake:
            ring = await paired(fake, tmp_path)
            async with await Lamp.connect(keyring=ring) as lamp:
                pending = asyncio.ensure_future(lamp.gesture("nod", wait=True))
                await asyncio.sleep(0)
                assert lamp._done == {}, "a done kept for an action that gets none"
                await pending

    run(go())


def test_a_request_after_the_lamp_has_gone_raises_closed(tmp_path):
    async def go():
        async with FakeLamp() as fake:
            ring = await paired(fake, tmp_path)
            lamp = await Lamp.connect(keyring=ring)
            await lamp._ws.close()
            with pytest.raises(LampError) as raised:
                await asyncio.wait_for(lamp.get("light"), timeout=2)
            assert raised.value.code == "closed"
            assert lamp._waiting == {}

    run(go())


def test_a_send_cut_after_the_reader_failed_its_future_leaves_nothing_unread(tmp_path):
    async def go():
        # Reviewer-two's P3 on #3829: the reader ends during send and fails
        # the waiting future first; the send then raises ConnectionClosed.
        from websockets.exceptions import ConnectionClosedOK

        unread = []
        loop = asyncio.get_running_loop()
        loop.set_exception_handler(lambda loop, context: unread.append(context.get("message")))
        async with FakeLamp() as fake:
            ring = await paired(fake, tmp_path)
            lamp = await Lamp.connect(keyring=ring)

            async def send(_text):
                for future in lamp._waiting.values():
                    future.set_exception(LampError("the connection to the lamp closed", code="closed"))
                raise ConnectionClosedOK(None, None)

            lamp._ws.send = send

            async def ask() -> str:
                try:
                    await lamp.get("light")
                except LampError as error:
                    return error.code
                return "answered"

            assert await ask() == "closed"
            await lamp._ws.close()
        import gc

        # The future is gone with the request's frame; asyncio reports an
        # unread exception only when it is collected.
        gc.collect()
        await asyncio.sleep(0)
        assert not [m for m in unread if m and "never retrieved" in m], unread

    run(go())


def test_a_fake_like_lamp_control_refuses_what_it_doesnt_route(tmp_path):
    async def go():
        from lampos.testing import LAMP_CONTROL_REFUSES

        async with FakeLamp(grants=schema.grants(), refuses=LAMP_CONTROL_REFUSES) as fake:
            ring = await paired(fake, tmp_path)
            async with await Lamp.connect(keyring=ring) as lamp:
                with pytest.raises(errors.BY_CODE["refused"]) as raised:
                    async with lamp.lease("body", for_ms=5000):
                        await lamp.play("wave")
                assert raised.value.why == "not routed yet: waits for client packages"
                await lamp.express("glad")
                assert (await lamp.gesture("nod"))["to"] == "body"
                await lamp.light("warm", 3, 1_000)

    run(go())


def test_the_fake_checks_the_lease_before_the_routing_refusal(tmp_path):
    # As lamp-control's session.rs: no lease is no_lease, and only a held
    # lease reaches the "not routed yet" refusal (Reviewer-two's P3 on #3903).
    async def go():
        from lampos.testing import LAMP_CONTROL_REFUSES

        async with FakeLamp(grants=schema.grants(), refuses=LAMP_CONTROL_REFUSES) as fake:
            ring = await paired(fake, tmp_path)
            async with await Lamp.connect(keyring=ring) as lamp:
                with pytest.raises(errors.BY_CODE["no_lease"]):
                    await lamp.look("face")
                async with lamp.lease("body", for_ms=5000):
                    with pytest.raises(errors.BY_CODE["refused"]) as raised:
                        await lamp.look("face")
                    assert raised.value.why.startswith("not routed yet")

    run(go())


def _skill(folder: Path, name: str, rules: str = 'version = "0.1.0"\n[permissions]\nlight = true\n') -> Path:
    path = folder / name
    path.mkdir(parents=True)
    (path / "SKILL.md").write_text(f"---\nname: {name}\ndescription: A test skill.\n---\n")
    (path / "skill.toml").write_text(rules)
    (path / ".DS_Store").write_bytes(b"\0mac")
    return path


def test_lamp_skill_push_list_and_remove_run_as_the_js_clis_do(tmp_path):
    import os
    import sys

    async def lamp_cli(home: Path, *args: str) -> tuple[int, str, str]:
        # A process of its own, as a person runs it; the fake answers on
        # this event loop meanwhile.
        process = await asyncio.create_subprocess_exec(
            sys.executable, "-m", "lampos.cli", *args,
            env={**os.environ, "LAMPOS_HOME": str(home)},
            stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE,
        )
        out, err = await process.communicate()
        return process.returncode, out.decode().strip(), err.decode().strip()

    async def go():
        async with FakeLamp(grants=("light", "skills")) as fake:
            home = tmp_path / "home"
            await paired(fake, home)
            assert await lamp_cli(home, "skill", "push", str(_skill(tmp_path, "desk-wave"))) == (0, "desk-wave 0.1.0 pushed, on", "")
            assert (await lamp_cli(home, "skill", "list"))[1] == "desk-wave\t0.1.0\ton"
            assert (await lamp_cli(home, "skill", "remove", "desk-wave"))[1] == "desk-wave removed"
            assert (await lamp_cli(home, "skill", "list"))[1] == ""
            core = await lamp_cli(home, "skill", "push", str(_skill(tmp_path, "core", 'version = "0.1.0"\nkind = "core"\n')))
            assert (core[0], core[2]) == (1, 'lamp: refused: kind = "core": a pushed skill is installable')
            assert fake.asked("skill.remove") == [{"name": "desk-wave"}]

    run(go())


def test_a_signed_package_names_its_author_and_any_change_is_refused(tmp_path):
    import io
    import tarfile

    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

    from lampos.push import SIGNATURE, open_package, pack

    folder = _skill(tmp_path, "desk-wave")
    assert open_package(pack(folder).data).author is None
    ring = Keyring(tmp_path / "home")
    assert ring.author_key() is None
    key = ring.author_key(make=True)
    assert ring.author_key() is not None and (tmp_path / "home" / "author.pem").stat().st_mode & 0o777 == 0o600
    data = pack(folder, author=key).data
    with tarfile.open(fileobj=io.BytesIO(data)) as tar:
        # The signature is the second entry, as lamp-skills writes it.
        assert tar.getnames()[:2] == ["lamp-skill.json", SIGNATURE]
        signed = json.loads(tar.extractfile(SIGNATURE).read())
    opened = open_package(data)
    assert opened.author == signed["key"] and len(signed["key"]) == 16
    assert set(opened.files) == {"SKILL.md", "skill.toml"}

    def changed(rename=None, body=None):
        out = io.BytesIO()
        with tarfile.open(fileobj=io.BytesIO(data)) as src, tarfile.open(fileobj=out, mode="w", format=tarfile.USTAR_FORMAT) as dst:
            for member in src.getmembers():
                content = src.extractfile(member).read()
                if member.name == "desk-wave/skill.toml" and body is not None:
                    content, member.size = body, len(body)
                if member.name == SIGNATURE and rename is not None:
                    content = json.dumps({**json.loads(content), **rename}).encode()
                    member.size = len(content)
                dst.addfile(member, io.BytesIO(content))
        return out.getvalue()

    for bad, says in [
        (changed(body=b'version = "0.1.0"\n[permissions]\nbody = true\n'), "does not match"),
        (changed(rename={"key": "0" * 16}), "not the fingerprint"),
        (changed(rename={"public": base64.b64encode(bytes(32)).decode()}), "not the fingerprint"),
    ]:
        with pytest.raises(LampError) as raised:
            open_package(bad)
        assert says in raised.value.why


def test_lamp_skill_keygen_pack_and_check_a_package(tmp_path):
    import os
    import subprocess
    import sys

    env = {**os.environ, "LAMPOS_HOME": str(tmp_path / "home")}

    def lamp(*args: str) -> tuple[int, str]:
        done = subprocess.run([sys.executable, "-m", "lampos.cli", *args], env=env, capture_output=True, text=True)
        return done.returncode, (done.stdout + done.stderr).strip()

    folder = _skill(tmp_path, "desk-wave")
    out = tmp_path / "out"
    assert lamp("skill", "pack", str(folder), "--sign", "--out", str(out)) == (
        1, "lamp: bad_request: no author key: make one with `lamp skill keygen`")
    code, said = lamp("skill", "keygen")
    fpr = said.split()[2]
    assert code == 0 and said.endswith("author.pem") and len(fpr) == 16
    assert lamp("skill", "keygen")[1].startswith(f"author key {fpr} already there")
    code, said = lamp("skill", "pack", str(folder), "--sign", "--out", str(out))
    assert code == 0 and said.endswith(" signed")
    package = out / "desk-wave-0.1.0.tar"
    assert lamp("skill", "check", str(package)) == (0, f"desk-wave: ok, format 1, signed by key {fpr}, not the Store")
    package.write_bytes(package.read_bytes().replace(b"light = true", b"light = TRUE"))
    code, said = lamp("skill", "check", str(package))
    assert code == 1 and "does not match lamp-skill.json" in said


def test_a_shared_package_is_fetched_over_https_only_and_capped():
    # Fourteen's point 4 on #3504: https only, no redirect to another scheme,
    # and reading stops at 1 MiB, not after the whole download.
    import io
    import urllib.request

    from lampos.push import PACKAGE_MAX, _HttpsOnly, fetch, read_capped

    for url in ("http://example.com/a.tar", "file:///etc/passwd", "ftp://example.com/a.tar"):
        with pytest.raises(LampError) as raised:
            fetch(url)
        assert "https only" in raised.value.why
    request = urllib.request.Request("https://example.com/a.tar")
    with pytest.raises(LampError) as raised:
        _HttpsOnly().redirect_request(request, None, 302, "Found", {}, "http://example.com/a.tar")
    assert "away from https" in raised.value.why
    assert _HttpsOnly().redirect_request(request, None, 302, "Found", {}, "https://mirror.example/a.tar") is not None

    class Counted(io.BytesIO):
        asked = 0

        def read(self, size=-1):
            Counted.asked = size
            return super().read(size)

    assert read_capped(io.BytesIO(b"x" * PACKAGE_MAX)) == b"x" * PACKAGE_MAX
    with pytest.raises(LampError):
        read_capped(Counted(b"x" * (PACKAGE_MAX * 4)))
    assert Counted.asked == PACKAGE_MAX + 1


def test_a_skill_is_packed_as_lamp_skills_packs_it(tmp_path):
    import io
    import tarfile

    from lampos.push import pack

    folder = _skill(tmp_path, "desk-wave")
    (folder / "moves").mkdir()
    (folder / "moves" / "wave.json").write_text("{}")
    package = pack(folder)
    assert package.name == "desk-wave" and package.version == "0.1.0"
    assert pack(folder).data == package.data, "the same folder packs to the same bytes"
    with tarfile.open(fileobj=io.BytesIO(package.data)) as tar:
        members = tar.getmembers()
        manifest = json.loads(tar.extractfile("lamp-skill.json").read())
    assert [m.name for m in members] == ["lamp-skill.json", "desk-wave/SKILL.md", "desk-wave/moves/wave.json", "desk-wave/skill.toml"]
    assert all(m.isfile() and m.mode == 0o644 and m.mtime == 0 and m.uid == m.gid == 0 for m in members)
    assert [f["path"] for f in manifest["files"]] == ["SKILL.md", "moves/wave.json", "skill.toml"]
    assert manifest["format"] == 1 and manifest["version"] == "0.1.0"


def test_a_skill_is_pushed_in_parts_listed_and_removed(tmp_path, monkeypatch):
    from lampos import push

    monkeypatch.setattr(push, "PART_BYTES", 600)  # several parts from a small skill
    async def go():
        async with FakeLamp(grants=("light", "skills")) as fake:
            ring = await paired(fake, tmp_path / "keys")
            async with await Lamp.connect(keyring=ring) as lamp:
                skill = await lamp.push_skill(_skill(tmp_path, "desk-wave"))
                assert (skill["name"], skill["version"], skill["on"]) == ("desk-wave", "0.1.0", True)
                pushes = fake.asked("skill.push")
                assert len(pushes) > 1 and [p["part"] for p in pushes] == list(range(len(pushes)))
                assert [s["name"] for s in await lamp.skills()] == ["desk-wave"]
                await lamp.remove_skill("desk-wave")
                assert await lamp.skills() == []

    run(go())


def test_a_push_beyond_the_pairing_is_refused_before_a_byte_is_sent(tmp_path):
    async def go():
        async with FakeLamp(grants=("light", "skills")) as fake:
            ring = await paired(fake, tmp_path / "keys")
            async with await Lamp.connect(keyring=ring) as lamp:
                with pytest.raises(errors.BY_CODE["no_grant"]):
                    await lamp.push_skill(_skill(tmp_path, "mover", 'version = "0.1.0"\n[permissions]\nbody = true\n'))
                with pytest.raises(LampError) as broken:
                    await lamp.push_skill(_skill(tmp_path, "broken", 'version = "0.1.0"\nverison = 1\n'))
                assert broken.value.code == "bad_request"
                assert fake.asked("skill.push") == []
        async with FakeLamp(grants=("light",)) as fake:
            ring = await paired(fake, tmp_path / "keys2")
            async with await Lamp.connect(keyring=ring) as lamp:
                with pytest.raises(errors.BY_CODE["no_grant"]):
                    await lamp.push_skill(_skill(tmp_path / "b", "lit"))
                assert fake.asked("skill.push") == []

    run(go())


@pytest.mark.parametrize(
    "rules, code, says",
    [
        ('kind = "core"\n', "refused", "installable"),
        ('kind = "addon"\n', "refused", "installable"),
        # As the door answers the helper's hear and sound refusal (push.rs).
        ('[permissions]\ntopics = ["voice/heard"]\n', "no_grant", "format 2's phrase"),
        ('[permissions]\ntopics = ["ears/classes"]\n', "no_grant", "format 2's phrase"),
        ('[permissions]\ntopics = ["room/activity"]\n', "refused", "raw topic"),
        ('[permissions]\ntopics = ["humans/faces/tracked"]\n', "no_grant", "look grant"),
        ('[permissions]\nmessages = true\n', "refused", "outside the protocol"),
        ('[permissions]\nspeak = true\n', "no_grant", "speak grant"),
    ],
)
def test_a_push_refuses_on_the_computer_what_the_lamp_refuses(tmp_path, rules, code, says):
    from lampos.push import asks_beyond

    folder = _skill(tmp_path, "pushed", 'version = "0.1.0"\n' + rules)
    # Only a grant the pairing lacks is no_grant, as the lamp answers.
    assert asks_beyond(folder, ["body", "light", "skills"])[0] == code
    assert says in asks_beyond(folder, ["body", "light", "skills"])[1]


def test_a_picture_or_presence_skill_is_left_to_the_lamp(tmp_path):
    from lampos.push import asks_beyond

    seer = _skill(tmp_path, "seer", 'version = "0.1.0"\n[permissions]\ncamera = true\n')
    assert asks_beyond(seer, ["skills"]) is None
    there = _skill(tmp_path, "there", 'version = "0.1.0"\n[permissions]\ntopics = ["humans/faces/tracked"]\n')
    assert asks_beyond(there, ["look", "skills"]) is None


def test_a_push_takes_every_topic_lamp_skills_takes(tmp_path):
    # Reviewer-lead's P2 on #4029: lamp-skills' PUSHED_TOPICS (#3879) takes the
    # topics the protocol's events need no grant for, so a pairing with none
    # but `skills` pushes them, and never stricter than the lamp.
    from lampos.push import asks_beyond

    for topic in ("clock/local", "voice/state", "lamp/pause", "controls/touch"):
        folder = _skill(tmp_path / topic.replace("/", "-"), "open", f'version = "0.1.0"\n[permissions]\ntopics = ["{topic}"]\n')
        assert asks_beyond(folder, ["skills"]) is None


# harness-reviewer's P3 on #4029: the tar crate's layout, byte for byte (seven
# octal digits and a NUL for the checksum, two zero blocks and no record
# padding at the end), so this, the JS SDK and `lamp-skills pack` give one
# SHA-256 for one folder. The JS test pins the same digest.
PACKED_SHA256 = "4b956fc5e58e17f1165e68ccb16024df1a135a14e65d50b94febe372bf4a8321"


def test_a_skill_packs_to_the_bytes_lamp_skills_writes(tmp_path):
    import io
    import re
    import tarfile

    from lampos.push import pack

    folder = tmp_path / "desk-golden"
    (folder / "moves").mkdir(parents=True)
    (folder / "SKILL.md").write_text("---\nname: desk-golden\ndescription: A test skill.\n---\n")
    (folder / "skill.toml").write_text('version = "0.1.0"\n[permissions]\nlight = true\n')
    (folder / "moves" / "wave.json").write_text("{}")
    data = pack(folder).data
    assert hashlib.sha256(data).hexdigest() == PACKED_SHA256
    assert re.fullmatch(rb"[0-7]{7}\0", data[148:156])
    assert len(data) == 4 * 1024 + 1024 and data[-1024:] == bytes(1024)
    names = [member.name for member in tarfile.open(fileobj=io.BytesIO(data))]
    assert names == ["lamp-skill.json", "desk-golden/SKILL.md", "desk-golden/moves/wave.json", "desk-golden/skill.toml"]


def test_a_path_lamp_skills_refuses_or_no_header_holds_is_refused_before_packing(tmp_path):
    # Reviewer-lead's P3 on #4029: lamp-skills' path rule (package.rs
    # `path_ok`) first, then the tar crate's ustar cut, never a corrupt header.
    import io
    import tarfile

    from lampos.push import pack

    def folder(name, path):
        root = tmp_path / name
        (root / path).parent.mkdir(parents=True, exist_ok=True)
        (root / "skill.toml").write_text('version = "0.1.0"\n')
        (root / path).write_text("{}")
        return root

    with pytest.raises(LampError) as raised:
        pack(folder("desk-part", "m" * 101 + ".json"))
    assert "at most 4 parts" in raised.value.why
    with pytest.raises(LampError) as raised:
        pack(folder("desk-deep", "a/b/c/d/e.json"))
    assert "at most 4 parts" in raised.value.why
    # Cut after its 100-byte folder: the prefix holds the folder, the name the file.
    data = pack(folder("desk-long", "a" * 100 + "/w.json")).data
    names = [member.name for member in tarfile.open(fileobj=io.BytesIO(data))]
    assert "desk-long/" + "a" * 100 + "/w.json" in names
    # The last `/` within 155 bytes leaves a 107-byte name: a GNU entry in the crate.
    with pytest.raises(LampError) as raised:
        pack(folder("desk-over", "a" * 100 + "/" + "b" * 100 + "/c.json"))
    assert "longer than a tar header holds" in raised.value.why


@pytest.mark.parametrize(
    "rules, asks",
    [
        ('version = "0.1.0"\n[permissions]\nlight = true\nspeak = true\n', ["light", "speak"]),
        ('version = "0.1.0"\n[permissions]\nbody = true\ntopics = ["room/stood_up"]\n', ["body", "look"]),
        # Format 1's look is the picture; format 2's is presence alone.
        ('version = "0.1.0"\n[permissions]\nlook = true\n', ["camera"]),
        ('format = 2\nversion = "0.1.0"\n[permissions]\nlook = true\n', ["look"]),
        ('format = 2\nversion = "0.1.0"\n[permissions]\nlook = true\ncamera = true\n', ["camera", "look"]),
        ('version = "0.1.0"\n', []),
    ],
)
def test_asks_are_the_grant_names_a_person_is_shown(rules, asks):
    from lampos.push import asks_of, tomllib

    assert asks_of(tomllib.loads(rules)) == asks


@pytest.mark.parametrize("path", ["../escape.txt", "/etc/x", "a/../../b", ".hidden", "a//b", "a/b/c/d/e", "back\\slash"])
def test_a_package_path_outside_its_folder_is_refused_before_anything_is_written(tmp_path, path):
    import hashlib
    import io
    import json
    import tarfile

    from lampos.push import open_package

    body = b"x"
    manifest = {"format": 1, "name": "desk-wave", "version": "0.1.0",
                "files": [{"path": path, "size": 1, "sha256": hashlib.sha256(body).hexdigest()}]}
    out = io.BytesIO()
    with tarfile.open(fileobj=out, mode="w", format=tarfile.USTAR_FORMAT) as tar:
        for name, data in (("lamp-skill.json", json.dumps(manifest).encode()), (f"desk-wave/{path}", body)):
            info = tarfile.TarInfo(name)
            info.size = len(data)
            tar.addfile(info, io.BytesIO(data))
    with pytest.raises(LampError) as raised:
        open_package(out.getvalue())
    assert "a skill's paths are at most 4 parts" in raised.value.why


def test_a_shared_package_is_installed_with_its_asks_on_part_0(tmp_path, monkeypatch):
    from lampos import push

    monkeypatch.setattr(push, "PART_BYTES", 600)
    author = Keyring(tmp_path / "author").author_key(make=True)
    rules = 'version = "0.2.0"\n[permissions]\nlight = true\nspeak = true\n'
    signed = push.pack(_skill(tmp_path, "desk-wave", rules), author=author).data

    async def go():
        async with FakeLamp(grants=("light", "speak", "skills")) as fake:
            ring = await paired(fake, tmp_path / "keys")
            async with await Lamp.connect(keyring=ring) as lamp:
                skill = await lamp.install_package(signed)
                assert (skill["name"], skill["version"], skill["on"]) == ("desk-wave", "0.2.0", True)
                fpr = push.open_package(signed).author
                assert (skill["author"], skill["asks"]) == (fpr, ["light", "speak"])
                assert [s.get("author") for s in await lamp.skills()] == [fpr]
                pushes = fake.asked("skill.push")
                assert len(pushes) > 1 and pushes[0]["asks"] == ["light", "speak"]
                assert all("asks" not in later for later in pushes[1:])
                assert pushes[0]["sha256"] == hashlib.sha256(signed).hexdigest()
                # Beyond the pairing, or changed after signing: nothing more is sent.
                body = push.pack(_skill(tmp_path / "b", "desk-lift", 'version = "0.1.0"\n[permissions]\nbody = true\n'), author=author).data
                with pytest.raises(errors.BY_CODE["no_grant"]):
                    await lamp.install_package(body)
                with pytest.raises(LampError, match="does not match lamp-skill.json"):
                    await lamp.install_package(signed.replace(b"speak = true", b"speak = TRUE"))
                assert len(fake.asked("skill.push")) == len(pushes)
                # A signed package sent without asks, as a plain push would: the lamp requires them.
                with pytest.raises(errors.BY_CODE["bad_request"], match="asks on part 0"):
                    await lamp._send_package(push.Package("desk-wave", "0.2.0", signed))

    run(go())


def test_lamp_skill_install_shows_the_signer_and_asks_then_asks_yes(tmp_path):
    import os
    import sys

    from lampos import push

    author = Keyring(tmp_path / "author").author_key(make=True)
    package = tmp_path / "desk-wave-0.1.0.tar"
    package.write_bytes(push.pack(_skill(tmp_path, "desk-wave", 'version = "0.1.0"\n[permissions]\nlight = true\n'), author=author).data)
    fpr = push.open_package(package.read_bytes()).author

    async def lamp_cli(home: Path, *args: str, typed: str = "") -> tuple[int, str]:
        process = await asyncio.create_subprocess_exec(
            sys.executable, "-m", "lampos.cli", *args,
            env={**os.environ, "LAMPOS_HOME": str(home)},
            stdin=asyncio.subprocess.PIPE, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE,
        )
        out, err = await process.communicate(typed.encode())
        return process.returncode, (out.decode() + err.decode()).strip()

    async def go():
        async with FakeLamp(grants=("light", "skills")) as fake:
            home = tmp_path / "home"
            await paired(fake, home)
            code, said = await lamp_cli(home, "skill", "install", str(package), typed="n\n")
            assert code == 1 and said.endswith("not installed"), said
            assert said.splitlines()[:3] == ["desk-wave 0.1.0", f"  signed by key {fpr}, not the Store", "  asks to change the light (light)"]
            assert fake.asked("skill.push") == []
            code, said = await lamp_cli(home, "skill", "install", str(package), typed="y\n")
            assert code == 0 and said.endswith("desk-wave 0.1.0 installed, on"), said
            assert (await lamp_cli(home, "skill", "list"))[1] == f"desk-wave\t0.1.0\ton\tsigned by key {fpr}"
            code, said = await lamp_cli(home, "skill", "install", "http://example.com/desk-wave.tar", "--yes")
            assert code == 1 and "https only" in said

    run(go())
