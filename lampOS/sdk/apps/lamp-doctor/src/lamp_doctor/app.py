"""Lamp Doctor.

Store v1 (`main`, run as the app's entry): the grants that ship today, `hear`, `speech`, `device` and `notes`,
and the panel. It counts the answers after the lamp's name and the failed calls, and shows the day on the
panel (`watch.py`).

v2, not in the manifest yet (`run_day`, `build_day`): once a day read the `turns`, score them and send the fixed
schema with `report`; the model diagnosis (grants `conversation`, `model`) is behind `with_model`, off, until a
later version asks for those grants."""

from __future__ import annotations

import asyncio
from typing import Any, Awaitable, Callable

from .diagnose import diagnose
from .report import build_report
from .score import score_day

RETRY_S = 3600  # `log.send` takes at most one report an hour per app, and none while the lamp is private
TRIES = 3


async def build_day(ops: Any, with_model: bool = False) -> dict[str, Any] | None:
    """The day's report, or None when there were no turns."""
    records = await ops.turns()
    if not records:
        return None
    day = score_day(records)
    if not with_model or day.get("score") is None:
        return build_report(day, [])
    try:
        heard = await ops.heard_today()
    except Exception:
        heard = []
    return build_report(day, await diagnose(ops, day, heard))


async def run_day(
    ops: Any,
    tries: int = TRIES,
    retry_s: float = RETRY_S,
    sleep: Callable[[float], Awaitable[None]] = asyncio.sleep,
    with_model: bool = False,
) -> bool:
    """True if a report was sent. A refused report is sent again later, unchanged, up to `tries` times."""
    report = await build_day(ops, with_model)
    if report is None:
        return False
    for attempt in range(tries):
        try:
            await ops.log_send(report)
            return True
        except Exception:
            if attempt + 1 < tries:
                await sleep(retry_s)
    return False


async def main() -> None:
    """The app: connect on the app socket, watch, and end when the lamp's connection ends (the unit restarts it)."""
    from lampos import Lamp

    from .watch import Day, run

    async with await Lamp.connect_app() as lamp:
        watching = asyncio.ensure_future(run(lamp, Day()))
        closed = asyncio.ensure_future(lamp.wait_closed())
        done, _ = await asyncio.wait({watching, closed}, return_when=asyncio.FIRST_COMPLETED)
        for task in (watching, closed):
            task.cancel()
        for task in done:
            task.result()  # a failure of the watch is said, not lost


if __name__ == "__main__":
    asyncio.run(main())
