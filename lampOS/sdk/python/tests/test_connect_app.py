"""An installed app's socket (#7415), to nine's messages (piece 3): `Lamp.connect_app()` reads LAMP_APP_SOCKET, needs no
pairing, key or token, and gets the manifest's grants; `listen_for`, `took`, `settings` and `panel` are its ops, and
`app.trigger`, `app.settings` and `phrase.heard` its events. The fake lamp serves the same."""

from __future__ import annotations

import asyncio

import pytest

from lampos import Lamp, LampError
from lampos.client import APP_SOCKET_ENV
from lampos.errors import BY_CODE
from lampos.testing import FakeLamp


def run(coroutine):
    return asyncio.run(asyncio.wait_for(coroutine, 30))


async def settle():
    await asyncio.sleep(0.05)


def test_connect_app_by_path_gets_the_manifests_grants():
    async def go():
        async with FakeLamp(app=True, app_name="lessons", app_grants=("speak", "hear"), app_version="1.2.0") as fake:
            async with await Lamp.connect_app(path=fake.app_path) as lamp:
                assert lamp.grants == ["speak", "hear"]
                assert (lamp.app, lamp.app_version) == ("lessons", "1.2.0")

    run(go())


def test_connect_app_reads_the_environment(monkeypatch):
    async def go():
        async with FakeLamp(app=True) as fake:
            monkeypatch.setenv(APP_SOCKET_ENV, fake.app_path)
            async with await Lamp.connect_app() as lamp:
                assert "speak" in lamp.grants

    run(go())


def test_connect_app_without_a_socket_says_which_variable(monkeypatch, tmp_path):
    monkeypatch.setenv(APP_SOCKET_ENV, str(tmp_path / "none.sock"))
    with pytest.raises(LampError) as refused:
        run(Lamp.connect_app())
    assert refused.value.code == "no_app_socket"
    assert APP_SOCKET_ENV in str(refused.value)


def test_pairing_is_refused_on_an_apps_socket():
    async def go():
        async with FakeLamp(app=True) as fake:
            async with await Lamp.connect_app(path=fake.app_path) as lamp:
                with pytest.raises(LampError) as refused:
                    await lamp.request("pair", name="x", key="AAAA", proof="AAAA")
                assert refused.value.code == "bad_request"

    run(go())


def test_the_apps_ops_are_not_the_doors_on_a_paired_computer():
    async def go():
        async with FakeLamp(app=True) as fake:
            async with await Lamp.connect_app(path=fake.app_path) as lamp:
                pass
        async with FakeLamp() as fake:
            from lampos import Keyring
            import tempfile

            with tempfile.TemporaryDirectory() as folder:
                keyring = Keyring(folder)
                pairing = await Lamp.pair(fake.code, "127.0.0.1", pin=fake.pin, port=fake.port, name="t", keyring=keyring)
                async with await Lamp.connect(host="127.0.0.1", keyring=keyring) as lamp:
                    with pytest.raises(LampError) as refused:
                        await lamp.settings()
                    assert refused.value.code == "bad_request"
                    assert pairing

    run(go())


def test_listen_for_replaces_the_phrases_whole_and_tells_only_the_phrase():
    async def go():
        async with FakeLamp(app=True) as fake:
            async with await Lamp.connect_app(path=fake.app_path) as lamp:
                heard: list[dict] = []
                answer = await lamp.listen_for(["lesson time", "next word please"], heard.append)
                assert answer["phrases"] == 2
                await fake.hear_phrase("lesson time")
                await fake.hear_phrase("something else")
                await settle()
                assert [e["phrase"] for e in heard] == ["lesson time"]
                assert set(heard[0]) == {"ev", "at", "phrase"}  # never what was said
                # Replaced whole: the old phrase is no longer listened for; an empty list clears.
                await lamp.listen_for(["another phrase"])
                await fake.hear_phrase("lesson time")
                await settle()
                assert len(heard) == 1
                assert (await lamp.listen_for([]))["phrases"] == 0

    run(go())


def test_listen_for_keeps_the_doors_limits_and_needs_hear():
    async def go():
        async with FakeLamp(app=True, app_grants=("speak",)) as fake:
            async with await Lamp.connect_app(path=fake.app_path) as lamp:
                with pytest.raises(LampError) as refused:
                    await lamp.listen_for(["lesson time"])
                assert refused.value.code == "no_grant" and "hear" in str(refused.value)
        async with FakeLamp(app=True) as fake:
            async with await Lamp.connect_app(path=fake.app_path) as lamp:
                with pytest.raises(LampError) as refused:
                    await lamp.listen_for(["short"])
                assert refused.value.code == "bad_request"
                with pytest.raises(LampError) as refused:
                    await lamp.listen_for([f"phrase number {chr(97 + n)}{chr(97 + n)}" for n in range(17)])
                assert refused.value.code == "bad_request"
                # The lamp's own words reach the app when the SDK didn't catch it first: ask the raw op.
                with pytest.raises(LampError) as refused:
                    await lamp.request("phrases.set", phrases=["a" * 65])
                assert refused.value.code == "bad_request"

    run(go())


def test_a_manifest_trigger_comes_unasked_and_took_it_is_kept():
    async def go():
        async with FakeLamp(app=True) as fake:
            async with await Lamp.connect_app(path=fake.app_path) as lamp:
                got: list[dict] = []
                await lamp.on("app.trigger", got.append)
                await fake.trigger("start", "start my english lesson", turn="t-9")
                await settle()
                assert got and got[0]["trigger"] == "start" and got[0]["turn"] == "t-9"
                assert got[0]["phrase"] == "start my english lesson"
                await lamp.took(got[0]["turn"])
                assert fake.app_took_turns == ["t-9"]

    run(go())


def test_settings_get_and_the_owners_change_event():
    async def go():
        async with FakeLamp(app=True, app_settings={"level": "a1", "hints": True}) as fake:
            async with await Lamp.connect_app(path=fake.app_path) as lamp:
                assert await lamp.settings() == {"level": "a1", "hints": True}
                changes: list[dict] = []
                await lamp.on("app.settings", changes.append)
                await fake.set_app_setting(level="b2")
                await settle()
                assert changes[0]["changed"] == ["level"]
                assert changes[0]["settings"] == {"level": "b2", "hints": True}
                assert (await lamp.settings())["level"] == "b2"

    run(go())


def test_the_panel_counts_up_and_is_one_a_minute():
    async def go():
        async with FakeLamp(app=True) as fake:
            async with await Lamp.connect_app(path=fake.app_path) as lamp:
                assert await lamp.panel("Lessons", ["3 words today", "streak: 4"]) == 1
                assert fake.app_panel == {"title": "Lessons", "lines": ["3 words today", "streak: 4"], "rev": 1}
                with pytest.raises(LampError) as over:
                    await lamp.panel("Lessons", ["again"])
                assert over.value.code == "rate" and over.value.retry_ms > 0
        async with FakeLamp(app=True, panel_rate_s=0.0) as fake:
            async with await Lamp.connect_app(path=fake.app_path) as lamp:
                assert await lamp.panel("A") == 1
                assert await lamp.panel("B") == 2

    run(go())


def test_the_panel_is_text_in_its_limits():
    async def go():
        async with FakeLamp(app=True) as fake:
            async with await Lamp.connect_app(path=fake.app_path) as lamp:
                for title, lines in (("", []), ("x" * 41, []), ("ok", ["l"] * 7), ("ok", ["x" * 81]), ("ok", ["a\nb"]), ("a\x07", [])):
                    with pytest.raises(LampError) as refused:
                        await lamp.panel(title, lines)
                    assert refused.value.code == "bad_request", (title, lines)

    run(go())


def test_the_owner_stopping_the_app_closes_with_the_reason_and_no_event():
    async def go():
        async with FakeLamp(app=True) as fake:
            async with await Lamp.connect_app(path=fake.app_path) as lamp:
                seen: list[dict] = []
                await lamp.on("app.settings", seen.append)
                await fake.stop_app("the owner stopped the app")
                await lamp.wait_closed()
                assert lamp.close_reason == "the owner stopped the app"
                assert seen == []

    run(go())


def test_off_unsubscribes_a_door_event_on_an_apps_socket():
    # Heron's P2 on #7672: `on()` subscribes a door event there, so `off()` must take it back.
    async def go():
        async with FakeLamp(app=True, app_grants=("speak", "hear", "look")) as fake:
            async with await Lamp.connect_app(path=fake.app_path) as lamp:
                handler = lambda event: None
                await lamp.on("person.arrived", handler)
                lamp.off("person.arrived", handler)
                await settle()
                assert [r["op"] for r in fake.requests] == ["subscribe", "unsubscribe"]
                # phrase.heard and the app's own events were never subscribed: nothing to take back.
                lamp.on("app.trigger", handler)
                lamp.off("app.trigger", handler)
                await settle()
                assert [r["op"] for r in fake.requests] == ["subscribe", "unsubscribe"]

    run(go())


def test_on_phrase_heard_with_phrases_on_an_apps_socket_points_to_listen_for():
    async def go():
        async with FakeLamp(app=True) as fake:
            async with await Lamp.connect_app(path=fake.app_path) as lamp:
                with pytest.raises(LampError) as refused:
                    lamp.on("phrase.heard", lambda event: None, phrases=["lesson time"])
                assert refused.value.code == "bad_request" and "listen_for" in str(refused.value)

    run(go())


def test_a_refused_listen_for_leaves_no_handler_behind():
    async def go():
        async with FakeLamp(app=True, app_grants=("speak",)) as fake:
            async with await Lamp.connect_app(path=fake.app_path) as lamp:
                with pytest.raises(LampError):
                    await lamp.listen_for(["lesson time"], lambda event: None)
                assert not lamp._handlers.get("phrase.heard")

    run(go())


def test_an_empty_panel_line_is_refused_by_the_sdk_and_the_fake_alike():
    async def go():
        async with FakeLamp(app=True) as fake:
            async with await Lamp.connect_app(path=fake.app_path) as lamp:
                with pytest.raises(LampError) as refused:
                    await lamp.panel("ok", ["a", "", "b"])
                assert refused.value.code == "bad_request"
                with pytest.raises(LampError) as refused:
                    await lamp.request("app.panel.set", title="ok", lines=["a", "", "b"])
                assert refused.value.code == "bad_request"

    run(go())


# What an app tells Lamp's memory: `remember` (apps.remember) and `context` (apps.context), with the `remember` grant.
MEMORY_GRANTS = ("speak", "hear", "remember")
MEMORY_REFUSALS = ("off", "no_grant", "bad_request", "private", "unavailable", "rate", "refused")


def test_remember_keeps_a_fact_and_says_kept():
    async def go():
        async with FakeLamp(app=True, app_grants=MEMORY_GRANTS) as fake:
            async with await Lamp.connect_app(path=fake.app_path) as lamp:
                assert await lamp.remember("squats 3x10, the 6th too shallow") is True
                assert fake.asked("apps.remember") == [{"text": "squats 3x10, the 6th too shallow"}]
                assert fake.app_facts == ["squats 3x10, the 6th too shallow"]

    run(go())


def test_remember_and_context_need_the_remember_grant():
    async def go():
        async with FakeLamp(app=True) as fake:
            async with await Lamp.connect_app(path=fake.app_path) as lamp:
                for call in (lamp.remember("squats"), lamp.context("set 1 of 3")):
                    with pytest.raises(BY_CODE["no_grant"]):
                        await call

    run(go())


def test_each_memory_refusal_is_its_own_error():
    async def go():
        async with FakeLamp(app=True, app_grants=MEMORY_GRANTS) as fake:
            async with await Lamp.connect_app(path=fake.app_path) as lamp:
                for op, call in (("apps.remember", lambda: lamp.remember("squats")), ("apps.context", lambda: lamp.context("set 1 of 3"))):
                    for code in MEMORY_REFUSALS:
                        fake.refuse_next(op, code, f"the lamp says {code}", 1234 if code == "rate" else None)
                        with pytest.raises(BY_CODE[code]) as refused:
                            await call()
                        assert (refused.value.code, refused.value.why) == (code, f"the lamp says {code}")
                        assert refused.value.retry_ms == (1234 if code == "rate" else None)
                assert fake.app_facts == [] and fake.app_context is None

    run(go())


def test_remember_is_three_a_minute_with_retry_ms():
    async def go():
        async with FakeLamp(app=True, app_grants=MEMORY_GRANTS) as fake:
            async with await Lamp.connect_app(path=fake.app_path) as lamp:
                for n in range(3):
                    assert await lamp.remember(f"fact {n}")
                with pytest.raises(BY_CODE["rate"]) as over:
                    await lamp.remember("fact 3")
                assert 0 < over.value.retry_ms <= 60_000
                assert fake.app_facts == ["fact 0", "fact 1", "fact 2"]

    run(go())


def test_remember_is_refused_while_private_and_over_280():
    async def go():
        async with FakeLamp(app=True, app_grants=MEMORY_GRANTS) as fake:
            async with await Lamp.connect_app(path=fake.app_path) as lamp:
                with pytest.raises(BY_CODE["bad_request"]):
                    await lamp.remember("x" * 281)
                await fake.set_private(True)
                with pytest.raises(BY_CODE["private"]):
                    await lamp.remember("squats")
                with pytest.raises(BY_CODE["private"]):
                    await lamp.context("set 1 of 3")
                assert fake.app_facts == []

    run(go())


def test_context_is_renewed_each_rep_and_cleared_at_the_end():
    async def go():
        async with FakeLamp(app=True, app_grants=MEMORY_GRANTS) as fake:
            now = [100.0]
            fake.clock = lambda: now[0]
            async with await Lamp.connect_app(path=fake.app_path) as lamp:
                await lamp.context("set 2 of 3, 6 squats", for_ms=30_000)
                now[0] += 1
                await lamp.context("set 2 of 3, 7 squats", for_ms=30_000)
                assert fake.app_context == {"line": "set 2 of 3, 7 squats", "for_ms": 30_000}
                now[0] += 1
                await lamp.context("set 3 of 3, 1 squat")
                assert fake.app_context == {"line": "set 3 of 3, 1 squat", "for_ms": 60_000}
                await lamp.clear_context()
                assert fake.app_context is None
                now[0] += 1
                await lamp.context("again")
                await lamp.context("")
                assert fake.app_context is None
                assert fake.asked("apps.context")[-1] == {"line": ""}
                for line, for_ms in (("x" * 121, 1000), ("ok", 0), ("ok", 600_001)):
                    with pytest.raises(BY_CODE["bad_request"]):
                        await lamp.context(line, for_ms=for_ms)

    run(go())


def test_context_answers_ok_alone():
    async def go():
        async with FakeLamp(app=True, app_grants=MEMORY_GRANTS) as fake:
            async with await Lamp.connect_app(path=fake.app_path) as lamp:
                for fields in ({"line": "set 1 of 3", "for_ms": 1000}, {"line": ""}):
                    reply = await lamp.request("apps.context", **fields)
                    assert reply == {"re": reply["re"], "ok": True}

    run(go())


def test_context_is_one_line_a_second_and_only_set_lines_count():
    async def go():
        async with FakeLamp(app=True, app_grants=MEMORY_GRANTS) as fake:
            now = [100.0]
            fake.clock = lambda: now[0]
            async with await Lamp.connect_app(path=fake.app_path) as lamp:
                # Refused lines (too long, private) are not counted: the first set line goes.
                with pytest.raises(BY_CODE["bad_request"]):
                    await lamp.context("x" * 121)
                await fake.set_private(True)
                with pytest.raises(BY_CODE["private"]):
                    await lamp.context("rep 1")
                await fake.set_private(False)
                await lamp.context("rep 1")
                now[0] += 0.4
                with pytest.raises(BY_CODE["rate"]) as over:
                    await lamp.context("rep 2")
                assert (over.value.why, over.value.retry_ms) == ("one line a second", 600)
                # The rated line did not count: a second after the set one, the next goes.
                now[0] += 0.6
                await lamp.context("rep 2")
                assert fake.app_context["line"] == "rep 2"

    run(go())


def test_a_clear_is_never_rated_nor_refused_while_private():
    async def go():
        async with FakeLamp(app=True, app_grants=MEMORY_GRANTS) as fake:
            now = [100.0]
            fake.clock = lambda: now[0]
            async with await Lamp.connect_app(path=fake.app_path) as lamp:
                await lamp.context("rep 1")
                await lamp.clear_context()
                await lamp.clear_context()
                assert fake.app_context is None
                await fake.set_private(True)
                await lamp.clear_context()
                await lamp.context("")

    run(go())


def test_a_clear_with_for_ms_is_a_bad_request():
    async def go():
        async with FakeLamp(app=True, app_grants=MEMORY_GRANTS) as fake:
            async with await Lamp.connect_app(path=fake.app_path) as lamp:
                with pytest.raises(BY_CODE["bad_request"]):
                    await lamp.request("apps.context", line="", for_ms=1000)

    run(go())
