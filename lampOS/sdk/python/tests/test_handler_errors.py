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
