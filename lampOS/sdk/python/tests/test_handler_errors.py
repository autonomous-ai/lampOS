"""A handler that raises is said on the `lampos` logger and does not end the connection (it used to end it silently)."""

from __future__ import annotations

import asyncio
import logging
from pathlib import Path

from lampos import Keyring, Lamp
from lampos.testing import FakeLamp


def run(coroutine):
    return asyncio.run(asyncio.wait_for(coroutine, 20))


async def connected(fake: FakeLamp, folder: Path) -> Lamp:
    keyring = Keyring(folder)
    await Lamp.pair(fake.code, fake.host, pin=fake.pin, port=fake.port, keyring=keyring)
    return await Lamp.connect(keyring=keyring)


def test_a_sync_handler_that_raises_is_said_and_the_connection_lives(tmp_path, caplog):
    async def go():
        async with FakeLamp(grants=("body", "light", "speak", "look")) as fake:
            async with await connected(fake, tmp_path) as lamp:
                def bad(event):
                    raise RuntimeError("my handler bug")

                await lamp.on("person.arrived", bad)
                await fake.emit("person.arrived", count=1, near=0.3)
                await asyncio.sleep(0.2)
                # The reader still reads: a request is still answered.
                assert "lamp" in await lamp.get("lamp")

    with caplog.at_level(logging.ERROR, logger="lampos"):
        run(go())
    assert any("your handler for person.arrived raised" in r.getMessage() and "my handler bug" in str(r.exc_info) for r in caplog.records)


def test_an_async_handler_that_raises_is_said_too(tmp_path, caplog):
    async def go():
        async with FakeLamp(grants=("body", "light", "speak", "look")) as fake:
            async with await connected(fake, tmp_path) as lamp:
                async def bad(event):
                    raise RuntimeError("my async bug")

                await lamp.on("person.arrived", bad)
                await fake.emit("person.arrived", count=1, near=0.3)
                await asyncio.sleep(0.2)
                assert "lamp" in await lamp.get("lamp")

    with caplog.at_level(logging.ERROR, logger="lampos"):
        run(go())
    assert any("my async bug" in str(r.exc_info) for r in caplog.records)


def test_the_other_handlers_and_the_next_event_still_arrive(tmp_path, caplog):
    """The case the fix exists for: a plain and a coroutine handler raise, a third handler and a second event still come."""

    async def go():
        async with FakeLamp(grants=("body", "light", "speak", "look")) as fake:
            async with await connected(fake, tmp_path) as lamp:
                seen = []

                def plain(event):
                    return event["no_such_key"]

                async def coroutine(event):
                    raise RuntimeError("my async bug")

                await lamp.on("touch", plain)
                await lamp.on("touch", coroutine)
                await lamp.on("touch", lambda event: seen.append(event["kind"]))
                await fake.emit("touch", kind="tap")
                await fake.emit("touch", kind="pet")
                for _ in range(100):
                    if len(seen) == 2:
                        break
                    await asyncio.sleep(0.02)
                await asyncio.sleep(0.1)
                assert seen == ["tap", "pet"]

    with caplog.at_level(logging.ERROR, logger="lampos"):
        run(go())
    assert len([r for r in caplog.records if "your handler for touch raised" in r.getMessage()]) == 4
