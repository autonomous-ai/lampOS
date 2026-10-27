"""`lamp-mcp`: one paired lamp as MCP tools and resources, for an agent on
the developer's computer (docs/control.md, "The doors"; Nine's design on
#3504). It runs there, never on a lamp, and speaks to the lamp only through
the `lampos` SDK and the pairing `lamp pair` made.

    claude mcp add lamp -- lamp-mcp

Each tool is one op of the v1 protocol, offered only when the lamp's
`hello` granted what it needs. An agent can't hold a live push, so events
come through `lamp_wait_for`, and the state through resources.
"""

# No `from __future__ import annotations`: FastMCP reads each tool's
# annotations at registration, and the gesture, colour and pattern names are
# built from the schema here, not importable by name.

import argparse
import asyncio
import collections
import socket
import time
from typing import Any, Literal

from lampos import Lamp, LampError, schema
from mcp.server.fastmcp import FastMCP
from mcp.server.fastmcp.exceptions import ToolError

# The body's clips a client may name, less the two the lamp refuses through
# this door: `idle` is the lamp's own life, and `scanning` sweeps a room like
# a camera (CLAUDE.md rule 8; Nine's `GESTURES_REFUSED`, #3504 13:27).
REFUSED_GESTURES = ("idle", "scanning")
# A body lease for one look or move, never renewed: released at `done`, or
# left to lapse if `done` doesn't come (Nine, #3504 13:27).
ACTION_LEASE_MS = 10_000
# The longest `lamp_wait_for` waits, so one call can't hold an agent's turn.
MOST_WAIT_S = 120
# What `lamp://events/recent` keeps.
RECENT_EVENTS = 50
STATES = ("presence", "light", "mood", "lamp")
# The grant reading a state or hearing an event needs, as `crates/control`
# has it (protocol.rs, `StateKind::needs` and `EventKind::needs`): what isn't
# here needs none.
STATE_NEEDS = {"presence": "look"}
EVENT_NEEDS = {
    "person.arrived": "look",
    "person.left": "look",
    "person.stood_up": "look",
    "person.facing": "look",
    "phrase.heard": "hear",
    "name.heard": "hear",
    "sound.heard": "sound",
}
# Which `limits` kind each op counts against (docs/control.md, "Limits").
LIMIT_KIND = {
    "gesture": "motion",
    "move": "motion",
    "look": "motion",
    "light": "light",
    "say": "say",
    "express": "express",
}


def gestures() -> list[str]:
    return [name for name in schema.gestures() if name not in REFUSED_GESTURES]


def refused(error: LampError) -> ToolError:
    """The lamp's refusal as the tool's error, its code and words kept."""
    text = f"{error.code}: {error.why}"
    if error.retry_ms is not None:
        text += f" (try again in {error.retry_ms} ms)"
    return ToolError(text)


class Limits:
    """The lamp's `limits` from hello, kept by this server too, so an agent
    in a loop is slowed here before the lamp has to refuse it. A kind is
    `[{"most": N, "within_ms": T}, ...]`; any other shape is left to the lamp.
    """

    def __init__(self, limits: dict, clock=time.monotonic):
        self._clock = clock
        self._windows: dict[str, list[tuple[int, float]]] = {}
        for kind, windows in limits.items():
            if isinstance(windows, list):
                kept = [
                    (window["most"], window["within_ms"] / 1000)
                    for window in windows
                    if isinstance(window, dict)
                    and isinstance(window.get("most"), int)
                    and isinstance(window.get("within_ms"), (int, float))
                ]
                if kept:
                    self._windows[kind] = kept
        self._sent: dict[str, collections.deque] = collections.defaultdict(collections.deque)

    def take(self, op: str) -> None:
        """Count `op`, or refuse it as the lamp would, with when to retry."""
        kind = LIMIT_KIND.get(op)
        windows = self._windows.get(kind or "")
        if not windows:
            return
        now = self._clock()
        sent = self._sent[kind]
        longest = max(within for _, within in windows)
        while sent and now - sent[0] > longest:
            sent.popleft()
        for most, within in windows:
            inside = [at for at in sent if now - at <= within]
            if len(inside) >= most:
                retry_ms = int((within - (now - inside[0])) * 1000) + 1
                raise ToolError(f"rate: at most {most} {kind} in {within:g} s (try again in {retry_ms} ms)")
        sent.append(now)


def build(lamp: Lamp, *, client: str | None = None, clock=time.monotonic) -> FastMCP:
    """The MCP server for one connected `lamp`. Tools the lamp's grants
    don't allow are not listed at all."""
    grants = set(lamp.grants)
    limits = Limits(lamp.limits, clock)
    why = f"MCP: {client or socket.gethostname()}"[:64]
    recent: collections.deque = collections.deque(maxlen=RECENT_EVENTS)
    leases: set[str] = set()
    server = FastMCP(
        "lamp",
        instructions=(
            f"A lampOS lamp, {lamp.info.get('name') or lamp.info.get('id', 'paired')}, on the person's desk. "
            "It moves, lights and speaks for you within its own safety limits, and takes itself back the "
            "moment someone talks to it or touches it: then an action is refused or cut short, never forced. "
            "It never gives camera pictures, audio, transcripts or who someone is."
        ),
    )

    def allowed(*needs: str) -> bool:
        return all(need in grants for need in needs)

    # Only what the grants let this computer read or hear is offered, and
    # asked for (blue's P2 on #3802).
    states = [state for state in STATES if allowed(*filter(None, [STATE_NEEDS.get(state)]))]
    events_heard = [
        event for event in schema.events() if allowed(*filter(None, [EVENT_NEEDS.get(event)]))
    ]

    async def act(op: str, **fields: Any) -> dict:
        limits.take(op)
        try:
            return await lamp.request(op, **{k: v for k, v in fields.items() if v is not None})
        except LampError as error:
            raise refused(error) from error

    async def with_body_lease(op: str, **fields: Any) -> str:
        """`op` under a body lease of its own, released at `done` or left to
        lapse; a lease another holds, or one the lamp suspends, is the
        tool's error, never retried."""
        limits.take(op)
        try:
            taken = await lamp.request("lease.take", parts=["body"], for_ms=ACTION_LEASE_MS, why=why)
        except LampError as error:
            raise refused(error) from error
        lease = taken["lease"]
        leases.add(lease)
        ended: asyncio.Future = asyncio.get_running_loop().create_future()

        def follow(event: dict) -> None:
            if event.get("ev") == "lease" and event.get("lease") == lease and event.get("state") in ("lost", "suspended"):
                if not ended.done():
                    ended.set_result(("lease", event.get("state"), event.get("why")))
            elif event.get("ev") == "done" and event.get("re") == sent.get("re") and not ended.done():
                ended.set_result(("done", event.get("how"), event.get("why")))

        sent: dict = {}
        lamp.on("lease", follow)
        lamp.on("done", follow)
        try:
            try:
                sent.update(await lamp.request(op, **fields))
            except LampError as error:
                raise refused(error) from error
            try:
                what, how, why_ended = await asyncio.wait_for(asyncio.shield(ended), ACTION_LEASE_MS / 1000)
            except asyncio.TimeoutError:
                # No `done` yet (the server sends it once actions reach the
                # parts): the lease lapses by itself; it is never held open.
                return f"{op} taken; its lease lapses within {ACTION_LEASE_MS // 1000} s"
            if what == "lease":
                raise ToolError(f"lease_{how}: {why_ended or 'the lamp took the body back'}")
            return f"{op} {how}" + (f": {why_ended}" if why_ended else "")
        finally:
            lamp.off("lease", follow)
            lamp.off("done", follow)
            # Let go at `done`, and of a suspended lease, so it never comes
            # back by itself; a lost one is gone already; with neither, it
            # lapses (and `lamp_stop` lets go of it sooner).
            if ended.done():
                what, how, _ = ended.result()
                if what == "done" or how == "suspended":
                    leases.discard(lease)
                    try:
                        await lamp.request("lease.release", lease=lease)
                    except LampError:
                        pass
                elif how == "lost":
                    leases.discard(lease)

    @server.tool()
    async def lamp_state(which: list[Literal["presence", "light", "mood", "lamp"]] | None = None) -> dict:
        """The lamp's state now: who is there (a count and how near, never who), its light, its mood,
        and whether it is talking, paused, private or asleep."""
        ungranted = [state for state in which or [] if state not in states]
        if ungranted:
            raise ToolError(f"no_grant: this computer may read {', '.join(states)}")
        try:
            reply = await lamp.get(*(which or states))
        except LampError as error:
            raise refused(error) from error
        return {k: v for k, v in reply.items() if k not in ("re", "ok")}

    @server.tool()
    async def lamp_wait_for(
        events: list[str], phrases: list[str] | None = None, timeout_s: float = 60
    ) -> dict:
        """Wait for the first of `events` (person.arrived, person.left, touch, button, phrase.heard, …),
        at most `timeout_s` seconds (120 at most). For phrase.heard give the `phrases` to listen for:
        the lamp says only which one was heard, never what else was said."""
        unknown = [event for event in events if event not in schema.events()]
        if unknown or not events:
            raise ToolError(f"bad_request: events are some of {', '.join(events_heard)}")
        ungranted = [event for event in events if event not in events_heard]
        if ungranted:
            raise ToolError(f"no_grant: this computer may wait for {', '.join(events_heard)}")
        first: asyncio.Future = asyncio.get_running_loop().create_future()

        def heard(event: dict) -> None:
            recent.append(event)
            if not first.done():
                first.set_result(event)

        # Subscribed here, not by `on()`'s own background subscribe, so a
        # refusal (`no_grant`) is the tool's error; `off()` unsubscribes.
        try:
            await lamp.subscribe(events, phrases)
        except LampError as error:
            raise refused(error) from error
        for event in events:
            lamp.on(event, heard)
        try:
            return await asyncio.wait_for(first, min(max(timeout_s, 0), MOST_WAIT_S))
        except asyncio.TimeoutError:
            return {"timed_out": True}
        finally:
            for event in events:
                lamp.off(event, heard)

    if allowed("body", "light"):

        @server.tool()
        async def lamp_express(feeling: str, strength: float = 0.5) -> str:
            """Show a feeling in the body and the light (glad, lively, low, …), `strength` 0 to 1."""
            await act("express", feeling=feeling, strength=strength)
            return f"expressing {feeling}"

    if allowed("body"):
        Gesture = Literal[tuple(gestures())]  # type: ignore[valid-type]

        @server.tool()
        async def lamp_gesture(name: Gesture) -> str:  # type: ignore[valid-type]
            """One of the lamp's own gestures: nod, headshake, greeting, laugh, shy, a dance, …"""
            await act("gesture", name=name)
            return f"{name} taken"

        @server.tool()
        async def lamp_look(
            at: Literal["face", "name", "ahead"] | None = None, yaw: float | None = None, pitch: float | None = None
        ) -> str:
            """Turn the head: at the face in view, toward who said its name, ahead, or to `yaw`/`pitch`
            in degrees (right and up positive), within its reach."""
            if at is None:
                target: Any = {"yaw": yaw or 0.0, "pitch": pitch or 0.0}
            else:
                if at == "face" and "look" not in grants:
                    raise ToolError("no_grant: looking at a face needs the look grant")
                target = at
            return await with_body_lease("look", at=target)

        @server.tool()
        async def lamp_move(step_ms: int, frames: list[list[int]]) -> str:
            """A move of your own: `frames` of 5 joint offsets in counts (pitch, roll, yaw, shoulder,
            elbow), the first and last all 0, one every `step_ms` (20 to 1000). The body checks it."""
            return await with_body_lease("move", step_ms=step_ms, frames=frames)

    if allowed("light"):

        @server.tool()
        async def lamp_light(
            colour: Literal[tuple(schema.colours())],  # type: ignore[valid-type]
            level: int = 3,
            for_ms: int = 10_000,
            pattern: Literal[tuple(schema.patterns())] | None = None,  # type: ignore[valid-type]
        ) -> str:
            """Light the ring for at most 30 s: one of its colours, `level` 1 to 5, steady or a pattern.
            The person's own light comes back after; at night the lamp dims it."""
            await act("light", colour=colour, level=level, for_ms=min(for_ms, 30_000), pattern=pattern)
            return f"{colour} light for {min(for_ms, 30_000)} ms"

    if allowed("speak"):

        @server.tool()
        async def lamp_say(text: str) -> str:
            """Say a short line in the lamp's own voice (300 characters at most). It isn't said while the
            lamp is in quiet hours, private, or talking with someone."""
            if len(text) > 300:
                raise ToolError("bad_request: at most 300 characters")
            await act("say", text=text)
            return "said"

    @server.tool()
    async def lamp_stop() -> str:
        """Stop everything this computer asked of the lamp, now, and let go of the body."""
        await act("stop")
        for lease in list(leases):
            leases.discard(lease)
            try:
                await lamp.request("lease.release", lease=lease)
            except LampError:
                pass
        return "stopped"

    for state in states:

        def reader(state: str = state):
            async def read() -> dict:
                try:
                    reply = await lamp.get(state)
                except LampError as error:
                    raise refused(error) from error
                return {k: v for k, v in reply.items() if k not in ("re", "ok")}

            return read

        server.resource(f"lamp://state/{state}", name=state, mime_type="application/json")(reader())

    @server.resource("lamp://events/recent", mime_type="application/json")
    def recent_events() -> list[dict]:
        """The last events `lamp_wait_for` heard, newest last: never transcripts."""
        return list(recent)

    @server.resource("lamp://lease", mime_type="application/json")
    def held_leases() -> list[str]:
        """The leases this server holds now."""
        return sorted(leases)

    return server


async def serve(lamp_id: str | None, client: str | None) -> None:
    async with await Lamp.connect(lamp_id) as lamp:
        await build(lamp, client=client).run_stdio_async()


def main(argv: list[str] | None = None) -> None:
    parser = argparse.ArgumentParser(prog="lamp-mcp", description="One paired lampOS lamp as MCP tools.")
    parser.add_argument("--lamp", help="the lamp's id, when this computer is paired with more than one")
    parser.add_argument("--client", help="how this computer is named in the lamp's leases and journal")
    args = parser.parse_args(argv)
    try:
        asyncio.run(serve(args.lamp, args.client))
    except LampError as error:
        # Said on stderr: stdout is the MCP stream.
        raise SystemExit(f"lamp-mcp: {error}") from None
