"""The lamp's connection for lamp-mcp, kept across the lamp's restarts.

Every update restarts the lamp and closes each connection. lamp-mcp serves an
agent for hours, so it connects again by itself: `Relink` stands where a
`Lamp` would, forwards every call to the connection of the moment, keeps the
subscriptions the tools made (`on`, `off`), and makes them again on each new
connection. A tool called while the lamp is away gets the SDK's own `closed`
refusal, and the next call after it is back works.

It stops connecting again on what can't mend that way, as
`Lamp.keep_connected` does: no pairing, another lamp at the address, a hello
the lamp refuses (a revoked computer). Then the tools keep answering `closed`
and the reason is said once on stderr.
"""

from __future__ import annotations

import asyncio
import sys
from typing import Any, Callable

from lampos import Lamp
from lampos.errors import LampError
from websockets.exceptions import WebSocketException

# Seconds before connecting again: the first after a closed connection, then
# longer while the lamp can't be reached (a restart takes a few seconds; an
# update about a minute). The same as the SDK's `keep_connected`.
PAUSES = (1.0, 2.0, 5.0, 10.0)

# Events that answer one request on one connection (a `done` matches its
# request's number, a `lease` its lease): never made again on a new
# connection, whose request numbers start over (Second's P2 on #5830).
REQUEST_SCOPED = ("done", "lease")


class Relink:
    """A `Lamp` that connects again after the lamp restarts."""

    def __init__(self, connect: Callable[[], Any], lamp: Lamp, *, pauses: tuple[float, ...] = PAUSES):
        self._connect = connect
        self._lamp = lamp
        self._pauses = pauses
        # Each subscription a tool holds now, in the order made: (event,
        # handler, phrases), made again on a new connection.
        self._held: list[tuple[str, Any, list[str] | None]] = []
        self.connections = 1
        self.gave_up: BaseException | None = None
        # Ends when the connection of the moment does: a wait for a request
        # takes it when it asks, so it ends `closed` with its connection.
        self.gone: asyncio.Future = asyncio.get_running_loop().create_future()
        self._task = asyncio.ensure_future(self._keep())

    @classmethod
    async def connect(cls, lamp_id: str | None = None, *, keyring: Any = None, pauses: tuple[float, ...] = PAUSES) -> "Relink":
        async def again() -> Lamp:
            return await Lamp.connect(lamp_id, keyring=keyring)

        return cls(again, await again(), pauses=pauses)

    # Read at build time: the first connection's grants and limits. A new
    # connection carries the same pairing's, unless the owner re-paired it.
    def __getattr__(self, name: str) -> Any:
        return getattr(self._lamp, name)

    def on(self, event: str, handler: Any, *, phrases: list[str] | None = None):
        if event not in REQUEST_SCOPED:
            self._held.append((event, handler, phrases))
        return self._lamp.on(event, handler, phrases=phrases)

    def off(self, event: str, handler: Any) -> None:
        for at, (held_event, held_handler, _) in enumerate(self._held):
            if held_event == event and held_handler is handler:
                del self._held[at]
                break
        self._lamp.off(event, handler)

    async def close(self) -> None:
        self._task.cancel()
        if not self.gone.done():
            self.gone.set_result(None)
        await self._lamp.close()

    async def _keep(self) -> None:
        while True:
            # A subscription's refusal nobody awaited comes out of
            # `wait_closed` while the connection still runs: wait on, so the
            # live connection is never left open behind a new one (Second's
            # note on #5830). `wait_closed` takes each refusal it raises.
            while True:
                try:
                    await self._lamp.wait_closed()
                    break
                except LampError:
                    continue
            if not self.gone.done():
                self.gone.set_result(None)
            failures = 0
            while True:
                await asyncio.sleep(self._pauses[min(failures, len(self._pauses) - 1)])
                try:
                    lamp = await self._connect()
                except LampError as error:
                    if error.code != "closed":
                        self.gave_up = error
                        print(f"lamp-mcp: not connecting again: {error}", file=sys.stderr, flush=True)
                        return
                except (OSError, asyncio.TimeoutError, WebSocketException, EOFError):
                    pass
                else:
                    break
                failures += 1
            self._lamp = lamp
            self.gone = asyncio.get_running_loop().create_future()
            self.connections += 1
            print(f"lamp-mcp: connected again ({self.connections})", file=sys.stderr, flush=True)
            for entry in list(self._held):
                # Only what a tool still holds now: a wait that ended while
                # an earlier one was being made again took its own off
                # (Second's P2 on #5830).
                if not self._holds(entry):
                    continue
                event, handler, phrases = entry
                try:
                    await lamp.on(event, handler, phrases=phrases)
                except LampError as refused:
                    print(f"lamp-mcp: {event} not subscribed again: {refused}", file=sys.stderr, flush=True)
                    continue
                if not self._holds(entry):
                    # Taken off while its subscribe was on the way.
                    lamp.off(event, handler)

    def _holds(self, entry: tuple) -> bool:
        return any(held is entry for held in self._held)
