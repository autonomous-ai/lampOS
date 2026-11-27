"""Lamp Doctor v2: the report app. Not the Store manifest's entry (that stays v1, `app.main`).

Grants `turns` and `report` (and `device`), never `hear`, `speech`, `microphone`, `camera`, `notes` or `remember`:
the lamp refuses `report` beside any of them. So the day's counts live here in memory, and in the report, never in
notes. Each `turns.finished` event is a turn record (no text); once an hour, if there are new records and at least
three questions, the day so far is scored and sent with `log.send` (one accepted report an hour; the cloud keeps one
per lamp per day, so the last one of the day is the one that counts). lamp-control sets `day` and `build`."""

from __future__ import annotations

import asyncio
from typing import Any, Awaitable, Callable

from .report import build_report
from .score import score_day

GRANTS = ("turns", "report", "device")
SEND_EVERY_S = 3600
MAX_RECORDS = 5000  # a day of voice turns is far below this; the oldest go first so the memory stays bounded
Sleep = Callable[[float], Awaitable[None]]


class Ledger:
    """The turn records of the app's run, in memory, as the voice finished them."""

    def __init__(self) -> None:
        self.records: list[dict[str, Any]] = []
        self.sent_count = 0

    def add(self, event: dict[str, Any]) -> None:
        record = {k: v for k, v in event.items() if k != "ev"}
        self.records.append(record)
        if len(self.records) > MAX_RECORDS:
            del self.records[: len(self.records) - MAX_RECORDS]
            self.sent_count = max(0, self.sent_count - 1)

    def fresh(self) -> bool:
        return len(self.records) > self.sent_count

    def report(self) -> dict[str, Any] | None:
        """The day so far, or None under three questions."""
        return build_report(score_day(self.records), [])


async def send_once(lamp: Any, ledger: Ledger) -> float:
    """Send the day so far if there is something new. Returns seconds to wait before the next try."""
    if not ledger.fresh():
        return SEND_EVERY_S
    report = ledger.report()
    if report is None:
        return SEND_EVERY_S
    count = len(ledger.records)
    try:
        await lamp.request("log.send", report=report)
    except Exception as error:  # LampError: no_grant, off, bad_request, rate, refused
        code = getattr(error, "code", "")
        retry_ms = getattr(error, "retry_ms", None)
        if code == "rate" and retry_ms:
            return max(1.0, retry_ms / 1000)
        if code in ("no_grant", "bad_request"):
            raise  # a fault of this app, not a wait: say it
        return SEND_EVERY_S  # off (the lamp is private or the unit is off) or refused: try next hour
    ledger.sent_count = count
    return SEND_EVERY_S


async def run(lamp: Any, ledger: Ledger | None = None, sleep: Sleep = asyncio.sleep, rounds: int | None = None) -> None:
    ledger = ledger or Ledger()
    lamp.on("turns.finished", ledger.add)
    n = 0
    while rounds is None or n < rounds:
        n += 1
        await sleep(await send_once(lamp, ledger))


async def main() -> None:
    from lampos import Lamp

    async with await Lamp.connect_app() as lamp:
        working = asyncio.ensure_future(run(lamp))
        closed = asyncio.ensure_future(lamp.wait_closed())
        done, _ = await asyncio.wait({working, closed}, return_when=asyncio.FIRST_COMPLETED)
        for task in (working, closed):
            task.cancel()
        for task in done:
            task.result()


if __name__ == "__main__":
    asyncio.run(main())
