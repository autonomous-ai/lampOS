"""The SDK against the fake lamp: pairing, hello, events, actions, leases and
refusals, as docs/control.md says them. No lamp needed."""

from __future__ import annotations

import asyncio
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
    for op in schema.ops():
        assert op in calls or callable(getattr(Lamp, op, None)), op
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


def test_actions_send_the_schemas_fields_and_wait_for_done(tmp_path):
    async def go():
        async with FakeLamp() as fake:
            ring = await paired(fake, tmp_path)
            async with await Lamp.connect(keyring=ring) as lamp:
                await lamp.light("warm", 3, 10_000, pattern="breathe")
                await lamp.say("Build passed.")
                await lamp.express("glad")
                done = await lamp.gesture("nod", wait=True)
                assert done["how"] == "finished"
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
                    waiting = asyncio.ensure_future(lamp.move([[0, 0, 0, 0, 0], [20, 0, 0, 0, 0], [0, 0, 0, 0, 0]], 40, wait=True))
                    while not fake.asked("move"):
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
                assert not lamp._refused.done()

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
