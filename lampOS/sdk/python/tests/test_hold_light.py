"""hold_light keeps a light on past one ask's 30 s, and stops when told."""

from __future__ import annotations

import asyncio
from pathlib import Path

from lampos import Keyring, Lamp
from lampos.testing import FakeLamp


def run(coroutine):
    return asyncio.run(asyncio.wait_for(coroutine, 30))


async def connected(fake: FakeLamp, folder: Path) -> Lamp:
    keyring = Keyring(folder)
    await Lamp.pair(fake.code, fake.host, pin=fake.pin, port=fake.port, keyring=keyring)
    return await Lamp.connect(keyring=keyring)


def test_hold_light_asks_again_until_stopped(tmp_path):
    async def go():
        async with FakeLamp() as fake:
            async with await connected(fake, tmp_path) as lamp:
                lamp.limits["hold_max_ms"] = 2000  # a lamp that holds a light 2 s, so the test needn't wait 15 s
                holding = await lamp.hold_light("warm", 3)
                assert len(fake.asked("light")) == 1
                assert fake.asked("light")[0]["for_ms"] == 2000
                await asyncio.sleep(2.3)
                holding.stop()
                asks = len(fake.asked("light"))
                assert asks >= 3, asks
                await asyncio.sleep(1.2)
                assert len(fake.asked("light")) == asks

    run(go())


def test_a_refused_renewal_goes_to_on_error(tmp_path):
    async def go():
        async with FakeLamp(refuses={}) as fake:
            async with await connected(fake, tmp_path) as lamp:
                lamp.limits["hold_max_ms"] = 2000
                failures = []
                holding = await lamp.hold_light("warm", 3, on_error=failures.append)
                fake.refuses["light"] = "test"
                await asyncio.sleep(1.5)
                holding.stop()
                assert failures

    run(go())
