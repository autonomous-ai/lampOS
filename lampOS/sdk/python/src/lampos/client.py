"""The lamp, from a developer's computer: pair once, then connect, listen and act.

    async with await Lamp.connect() as lamp:
        lamp.on("person.arrived", lambda event: lamp.gesture("greeting"))
        await lamp.wait_closed()

Every message is docs/control.schema.json's. The client checks the names it
sends against the schema before sending, and turns a refusal into a typed
error (`lampos.errors`).
"""

from __future__ import annotations

import asyncio
import inspect
import json
import socket
import ssl
import unicodedata
from collections import defaultdict
from typing import Any, Callable

from websockets.asyncio.client import ClientConnection, connect as ws_connect
from websockets.exceptions import ConnectionClosed

from . import debug, errors, schema
from .errors import BY_CODE, LampError, NotPaired, WrongLamp, from_reply
from .keys import Keyring, Pairing, b64, is_pin, pin_of, proof, unb64

Handler = Callable[[dict], Any]

# A ping this often keeps a connection that only listens open: the lamp
# closes one quiet for 2 minutes (lamp-control's QUIET_MAX, #3778).
KEEPALIVE_S = 20.0
# The actions lamp-control answers `{"to":"body"}` with no `done` after
# (docs/control.md): a wait for them ends at that answer. `say` answers
# `{"to":"voice"}` and its `done` does follow.
ENDS_AT_REPLY = frozenset({"gesture", "move"})
# A phrase's floor in letters and digits, lamp-control's PHRASE_MIN_LETTERS.
# The SDK's check is a pre-check: it may let through what the lamp refuses,
# never the other way.
PHRASE_MIN_LETTERS = 8


def _short_phrase(phrases: list[str] | None) -> None:
    """Refuse a phrase under the lamp's floor before it leaves the computer,
    in the lamp's words; the lamp checks it again."""
    for phrase in phrases or ():
        # The lamp counts Unicode Alphabetic or Numeric, which takes in marks
        # such as Devanagari's vowel signs; the stdlib has no Alphabetic, so
        # count letters, marks and numbers, a superset (Nine's and
        # harness-reviewer's P2 on #3914).
        if sum(unicodedata.category(c)[0] in "LMN" for c in phrase) < PHRASE_MIN_LETTERS:
            shown = json.dumps(phrase, ensure_ascii=False)
            raise BY_CODE["bad_request"](f"phrases: {shown} is too short: {PHRASE_MIN_LETTERS} letters at least")


def _tls() -> ssl.SSLContext:
    # The lamp's certificate is its own, self-signed: it is checked against
    # its pin (the full SHA-256 the owner's app shows), not a CA, before any
    # frame is sent.
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
    context.check_hostname = False
    context.verify_mode = ssl.CERT_NONE
    return context


async def _open(host: str, port: int, pin: str) -> ClientConnection:
    if not is_pin(pin):
        raise WrongLamp("no pin to check the lamp by: the 64 hex the owner's app shows with the code")
    ws = await ws_connect(
        f"wss://{host}:{port}/v{schema.VERSION}",
        ssl=_tls(),
        max_size=1 << 20,
        ping_interval=KEEPALIVE_S,
    )
    seen = pin_of(ws.transport.get_extra_info("ssl_object").getpeercert(binary_form=True))
    if seen != pin:
        await ws.close()
        raise WrongLamp(f"the certificate at {host} is not the lamp's: {seen[:16]}..., not {pin[:16]}...")
    return ws


async def _welcome(ws: ClientConnection) -> dict:
    first = json.loads(await asyncio.wait_for(ws.recv(), timeout=5))
    debug.frame("<-", first)
    if first.get("ev") != "welcome" or "nonce" not in first:
        raise LampError("the lamp's first frame was not a welcome", code="bad_welcome")
    try:
        first["nonce_bytes"] = unb64(first["nonce"], 32)
    except ValueError as error:
        raise LampError(f"the welcome's nonce is not 32 bytes of base64: {error}", code="bad_welcome") from error
    return first


def _check(op: str, fields: dict) -> None:
    """The names the schema enumerates, checked before sending."""
    if op not in schema.ops():
        raise LampError(f"no op {op!r} in v{schema.VERSION}", code="bad_request")
    allowed = {
        "name": schema.gestures() if op == "gesture" else None,
        "colour": schema.colours(),
        "pattern": schema.patterns(),
    }
    for field, names in allowed.items():
        value = fields.get(field)
        if names is not None and value is not None and value not in names:
            raise LampError(f"{field} {value!r}: one of {', '.join(names)}", code="bad_request")
    for value in fields.get("events", ()):
        if value not in schema.events():
            raise LampError(f"no event {value!r}", code="bad_request")
    for value in fields.get("parts", ()):
        if value not in schema.parts():
            raise LampError(f"no part {value!r}", code="bad_request")


class Subscription:
    """What `Lamp.on` returns: an awaitable, not a future. Awaited, it raises
    the lamp's refusal; one nobody awaits is raised by `Lamp.wait_closed`
    instead, never lost."""

    def __init__(self, task: asyncio.Future, lamp: "Lamp | None" = None):
        self.task = task
        self.awaited = False
        self._lamp = lamp

    def __await__(self):
        self.awaited = True
        # A refusal that came before this await was kept for `wait_closed`;
        # it is the caller's now, and only it leaves the list.
        lamp = self._lamp
        if lamp is not None and self.task.done() and not self.task.cancelled():
            error = self.task.exception()
            if error is not None:
                lamp._refusals = [kept for kept in lamp._refusals if kept is not error]
        return self.task.__await__()

    def done(self) -> bool:
        return self.task.done()


class Lease:
    """A lease on some of the lamp's parts, renewed for its holder until it is
    released or lost. `state` follows the lamp's lease events."""

    def __init__(self, lamp: "Lamp", parts: list[str], for_ms: int, why: str | None):
        self.lamp = lamp
        self.parts = parts
        self.for_ms = for_ms
        self.why = why
        self.id: str | None = None
        self.state = "none"
        self.lost_why: str | None = None
        self._renewing: asyncio.Task | None = None

    async def take(self) -> "Lease":
        fields: dict[str, Any] = {"parts": self.parts, "for_ms": self.for_ms}
        if self.why:
            fields["why"] = self.why
        reply = await self.lamp.request("lease.take", **fields)
        self.id = reply["lease"]
        self.state = "held"
        self.lamp._leases[self.id] = self
        self._renewing = asyncio.create_task(self._renew())
        return self

    async def _renew(self) -> None:
        # Renewed at half its time, so one late renewal does not lose it; a
        # lamp that says no (10 min a take, or lost) ends the renewing.
        while self.state in ("held", "suspended"):
            await asyncio.sleep(self.for_ms / 2000)
            if self.state not in ("held", "suspended"):
                return
            try:
                await self.lamp.request("lease.renew", lease=self.id, for_ms=self.for_ms)
            except LampError as error:
                self.state, self.lost_why = "lost", error.why
                return

    def _event(self, event: dict) -> None:
        self.state = event["state"]
        if self.state == "lost":
            self.lost_why = event.get("why")

    @property
    def held(self) -> bool:
        return self.state == "held"

    async def release(self) -> None:
        if self._renewing:
            self._renewing.cancel()
        if self.id and self.state in ("held", "suspended"):
            self.state = "released"
            try:
                await self.lamp.request("lease.release", lease=self.id)
            except LampError:
                pass

    async def __aenter__(self) -> "Lease":
        return await self.take()

    async def __aexit__(self, *_exc) -> None:
        await self.release()


class Following:
    """The head following a hand (`look` at `"hand"`), under a body lease of
    its own. lamp-body follows on the lamp, waiting while the hand is gone,
    until `stop()`, the client's `stop`, another look, or the lamp taking
    the body back (a hand on the head, a conversation). lamp-control
    answers `{"to": "body", "follow": "hand"}` and says no `done`, so
    `ended` is told here: `{"how": "finished", "why": "stopped"}`, or
    `{"how": "cut_short", "why": "stop" | "another look" | the lease's why}`.
    The client's `stop` lets go of its lease too. After another look the
    lease stays held, since that look runs under it, and the lamp's own life
    stands back until `stop()` or the block ends; a new `follow_hand()` lets
    go of it first. Where the hand is comes as `hand` events
    (`lamp.on("hand", ...)`): `seen` with `x`, `y`, `size`, `open`,
    `confidence`, and `seen: false` once it's gone. Needs the `body` and
    `look` grants."""

    def __init__(self, lamp: "Lamp", for_ms: int, why: str | None):
        self.lamp = lamp
        self.lease = Lease(lamp, ["body"], for_ms, why)
        self.ended: asyncio.Future = asyncio.get_running_loop().create_future()

    async def start(self) -> "Following":
        # One following a connection: the one before it ends, and its lease
        # is let go, or this one's take would be refused `busy`.
        before = self.lamp._following
        if before is not None:
            before.end("cut_short", "another look")
            await before._end()
        await self.lease.take()
        self.lamp.on("lease", self._lease_event)
        try:
            reply = await self.lamp.request("look", at="hand")
        except BaseException:
            await self._end()
            raise
        if reply.get("follow") != "hand":
            # A lamp that took the look but doesn't say it follows (a
            # lamp-control from before #4183): let go rather than claim it does.
            await self._end()
            raise errors.BY_CODE["refused"]("this lamp doesn't follow a hand yet")
        # Another look or a stop from this client ends it (`Lamp.look`,
        # `Lamp.stop`).
        self.lamp._following = self
        return self

    def end(self, how: str, why: str | None) -> None:
        if not self.ended.done():
            self.ended.set_result({"how": how, "why": why})

    def _lease_event(self, event: dict) -> None:
        if event.get("lease") == self.lease.id and event.get("state") == "lost":
            self.end("cut_short", event.get("why"))

    @property
    def following(self) -> bool:
        return not self.ended.done() and self.lease.held

    async def wait(self) -> dict:
        """Until it ends: how, and why."""
        return await asyncio.shield(self.ended)

    async def stop(self) -> dict:
        """Stop following: the lease is let go, and the head goes back to the
        lamp's own pose. Says how it ended (`finished`, "stopped", if it was
        still following)."""
        self.end("finished", "stopped")
        await self._end()
        return self.ended.result()

    async def _end(self) -> None:
        # The lamp keeps it while its lease is held, so a new following can
        # let go of it.
        if self.lamp._following is self:
            self.lamp._following = None
        self.lamp.off("lease", self._lease_event)
        await self.lease.release()

    async def __aenter__(self) -> "Following":
        return await self.start()

    async def __aexit__(self, *_exc) -> None:
        await self.stop()


class Lamp:
    """A connection to one paired lamp."""

    def __init__(self, ws: ClientConnection, welcome: dict, hello: dict):
        self._ws = ws
        self.welcome = welcome
        self.info: dict = hello.get("lamp") or welcome.get("lamp", {})
        self.grants: list[str] = hello.get("grants", [])
        self.limits: dict = hello.get("limits", {})
        self._next_id = 1
        self._waiting: dict[int, asyncio.Future] = {}
        self._done: dict[int, asyncio.Future] = {}
        self._handlers: dict[str, list[Handler]] = defaultdict(list)
        self._phrases: list[str] = []
        self._leases: dict[str, Lease] = {}
        # The hand this connection's head follows, if any (`follow_hand`).
        self._following: Following | None = None
        self._tasks: set[asyncio.Task] = set()
        # The first refusal of a subscription nobody awaited: `wait_closed`
        # raises it, so a listener the lamp refused never waits in silence
        # (harness-reviewer's P2 on #3760).
        # Refusals of subscriptions nobody awaited, oldest first, for
        # `wait_closed`; `_refused` is set when one is added.
        self._refusals: list[BaseException] = []
        self._refused = asyncio.Event()
        self._reader = asyncio.create_task(self._read())

    # Pairing and connecting.

    @staticmethod
    async def pair(
        code: str,
        host: str,
        *,
        pin: str,
        port: int = schema.PORT,
        name: str | None = None,
        keyring: Keyring | None = None,
    ) -> Pairing:
        """Pair this computer with the lamp at `host`, with the owner's
        one-time code and the pin the app shows with it (the certificate's
        full SHA-256, 64 hex). The certificate is checked against the pin
        before any frame is sent (only the TLS handshake and the WebSocket's
        upgrade request go first), so the code's proof never reaches another
        host; there is no pairing without a pin."""
        keyring = keyring or Keyring()
        code = code.replace(" ", "")
        pin = pin.replace(" ", "").replace(":", "").lower()
        if not (len(code) == 8 and code.isdigit()):
            raise LampError("the code is the 8 digits the app shows", code="bad_request")
        ws = await _open(host, port, pin)
        try:
            welcome = await _welcome(ws)
            key = keyring.public()
            ask = {
                "id": 1,
                "op": "pair",
                "name": (name or socket.gethostname())[:64],
                "key": b64(key),
                "proof": b64(proof(code, bytes.fromhex(pin), key, welcome["nonce_bytes"])),
            }
            debug.frame("->", ask)
            await ws.send(json.dumps(ask))
            reply = json.loads(await asyncio.wait_for(ws.recv(), timeout=10))
            debug.frame("<-", reply)
        finally:
            await ws.close()
        if not reply.get("ok"):
            raise from_reply(reply.get("error", {}))
        lamp = welcome["lamp"]
        pairing = Pairing(
            lamp=lamp["id"],
            name=lamp.get("name", ""),
            client=reply["client"],
            grants=reply.get("grants", []),
            fp=pin,
            host=host,
            port=port,
        )
        keyring.remember(pairing)
        return pairing

    @classmethod
    async def connect(
        cls,
        lamp: str | None = None,
        *,
        host: str | None = None,
        keyring: Keyring | None = None,
    ) -> "Lamp":
        """Connect to a paired lamp (by its id, or the only one paired) and
        say hello."""
        keyring = keyring or Keyring()
        pairing = keyring.pairing(lamp)
        if pairing is None:
            raise NotPaired("no pairing here for that lamp: run `lamp pair` first")
        ws = await _open(host or pairing.host, pairing.port, pairing.fp)
        try:
            welcome = await _welcome(ws)
            sig = keyring.hello(welcome["nonce_bytes"])
            hello = {"id": 0, "op": "hello", "client": pairing.client, "sig": b64(sig)}
            debug.frame("->", hello)
            await ws.send(json.dumps(hello))
            reply = json.loads(await asyncio.wait_for(ws.recv(), timeout=10))
            debug.frame("<-", reply)
        except BaseException:
            await ws.close()
            raise
        if not reply.get("ok"):
            await ws.close()
            raise from_reply(reply.get("error", {}))
        return cls(ws, welcome, reply)

    async def __aenter__(self) -> "Lamp":
        return self

    async def __aexit__(self, *_exc) -> None:
        await self.close()

    async def close(self) -> None:
        for lease in list(self._leases.values()):
            await lease.release()
        await self._ws.close()
        self._reader.cancel()

    async def wait_closed(self) -> None:
        """Until the lamp closes the connection, or raises the refusal of a
        subscription `on` asked for and nobody awaited (a missing grant,
        say)."""
        closed = asyncio.ensure_future(self._ws.wait_closed())
        try:
            # Checked again after every wake: an await elsewhere may have
            # taken the refusal that woke this, so the list says, not the wake.
            while not self._refusals:
                self._refused.clear()
                refused = asyncio.ensure_future(self._refused.wait())
                try:
                    done, _ = await asyncio.wait({closed, refused}, return_when=asyncio.FIRST_COMPLETED)
                finally:
                    refused.cancel()
                if closed in done and not self._refusals:
                    return
            raise self._refusals.pop(0)
        finally:
            closed.cancel()

    # Requests.

    async def request(self, op: str, **fields: Any) -> dict:
        """Send one request and await its reply; a refusal raises its error."""
        _check(op, fields)
        id_ = self._next_id
        self._next_id += 1
        future = asyncio.get_running_loop().create_future()
        self._waiting[id_] = future
        try:
            sent = {"id": id_, "op": op, **fields}
            debug.frame("->", sent)
            await self._ws.send(json.dumps(sent))
        except ConnectionClosed as error:
            # The lamp has gone: the same typed `closed` as a request the
            # close cut short, not the socket library's own error. The reader
            # may have failed the future already; it is read here, so asyncio
            # has nothing left to report about it.
            self._waiting.pop(id_, None)
            if future.done():
                future.exception()
            else:
                future.cancel()
            raise LampError("the connection to the lamp closed", code="closed") from error
        reply = await future
        if not reply.get("ok"):
            raise from_reply(reply.get("error", {}))
        return reply

    async def _act(self, op: str, wait: bool, **fields: Any) -> dict:
        """An action: its reply when taken, or with `wait` its `done` event;
        for the actions no `done` follows yet (`ENDS_AT_REPLY`), the reply.
        `request` takes the next id before it first yields, so this is it."""
        id_ = self._next_id
        # Handed to the body, which says nothing back, `gesture` and `move`
        # get no `done` (docs/control.md): their wait ends at the answer, so
        # none is kept for them.
        done = asyncio.get_running_loop().create_future() if wait and op not in ENDS_AT_REPLY else None
        if done is not None:
            self._done[id_] = done
        try:
            reply = await self.request(op, **{k: v for k, v in fields.items() if v is not None})
        except BaseException:
            if done is not None:
                self._done.pop(id_, None)
            raise
        return reply if done is None else await done

    # The reader.

    async def _read(self) -> None:
        try:
            await self._frames()
        except ConnectionClosed:
            # The lamp went away without a close (a restart, its Wi-Fi):
            # `wait_closed` returns, and the waiting requests fail below.
            pass

    async def _frames(self) -> None:
        try:
            async for text in self._ws:
                frame = json.loads(text)
                debug.frame("<-", frame)
                if "re" in frame and "ok" in frame:
                    future = self._waiting.pop(frame["re"], None) if frame["re"] is not None else None
                    if future and not future.done():
                        future.set_result(frame)
                elif "ev" in frame:
                    self._event(frame)
        finally:
            for future in [*self._waiting.values(), *self._done.values()]:
                if not future.done():
                    future.set_exception(LampError("the connection to the lamp closed", code="closed"))

    def _event(self, event: dict) -> None:
        name = event["ev"]
        if name == "lease":
            lease = self._leases.get(event.get("lease", ""))
            if lease:
                lease._event(event)
        elif name == "done":
            future = self._done.pop(event.get("re"), None)
            if future and not future.done():
                future.set_result(event)
        for handler in self._handlers.get(name, []):
            result = handler(event)
            if inspect.isawaitable(result):
                task = asyncio.ensure_future(result)
                self._tasks.add(task)
                task.add_done_callback(self._tasks.discard)

    # Events and state.

    def on(self, event: str, handler: Handler, *, phrases: list[str] | None = None) -> "Subscription":
        """Call `handler(event)` for each `event` the lamp says. A handler may
        be a coroutine. Subscribes at once, and returns that subscription:
        `await lamp.on(...)` raises the lamp's refusal (`NoGrantError` without
        the event's grant). Not awaited, a refusal is raised by
        `wait_closed` instead, never lost."""
        if event not in schema.events() and event not in ("lease", "done"):
            raise LampError(f"no event {event!r}", code="bad_request")
        _short_phrase(phrases)
        self._handlers[event].append(handler)
        if phrases:
            self._phrases = sorted(set(self._phrases) | set(phrases))
        if event not in schema.events():
            done = asyncio.get_running_loop().create_future()
            done.set_result({})
            return Subscription(done)
        task = asyncio.ensure_future(self.subscribe([event]))
        self._tasks.add(task)
        task.add_done_callback(self._tasks.discard)
        subscription = Subscription(task, self)
        task.add_done_callback(lambda task: self._subscribed(subscription, event, handler))
        return subscription

    def _subscribed(self, subscription: "Subscription", event: str, handler: Handler) -> None:
        task = subscription.task
        if task.cancelled():
            return
        error = task.exception()
        if error is None:
            return
        # A refused subscription takes its handler back off, so a later `on()`
        # for the event, once granted, doesn't call both.
        handlers = self._handlers.get(event, [])
        if handler in handlers:
            handlers.remove(handler)
        if not handlers:
            self._handlers.pop(event, None)
        # Only a refusal nobody awaited goes to `wait_closed`; a closed
        # connection is `wait_closed` returning, not a refusal.
        if subscription.awaited or getattr(error, "code", None) == "closed":
            return
        self._refusals.append(error)
        self._refused.set()

    def off(self, event: str, handler: Handler) -> None:
        """Stop calling `handler` for `event`. With no handler left for an
        event the lamp says, the client unsubscribes from it (in the
        background)."""
        handlers = self._handlers.get(event, [])
        if handler in handlers:
            handlers.remove(handler)
        if not handlers:
            self._handlers.pop(event, None)
            if event in schema.events():
                task = asyncio.ensure_future(self.request("unsubscribe", events=[event]))
                self._tasks.add(task)
                task.add_done_callback(self._tasks.discard)

    async def subscribe(self, events: list[str], phrases: list[str] | None = None) -> dict:
        _short_phrase(phrases)
        fields: dict[str, Any] = {"events": events}
        wanted = phrases or (self._phrases if "phrase.heard" in events else None)
        if wanted:
            fields["phrases"] = wanted
        return await self.request("subscribe", **fields)

    async def unsubscribe(self, events: list[str]) -> dict:
        for event in events:
            self._handlers.pop(event, None)
        return await self.request("unsubscribe", events=events)

    async def get(self, *states: str) -> dict:
        """The states asked for (`presence`, `light`, `mood`, `lamp`), by
        name: the reply's `state`."""
        return (await self.request("get", state=list(states))).get("state", {})

    # Leases.

    def lease(self, *parts: str, for_ms: int = 30_000, why: str | None = None) -> Lease:
        """`async with lamp.lease("body"):` holds the body, renewed until the
        block ends or the lamp takes it back."""
        return Lease(self, list(parts), for_ms, why)

    # Actions.

    async def express(self, feeling: str, strength: float | None = None) -> dict:
        return await self._act("express", False, feeling=feeling, strength=strength)

    async def gesture(self, name: str, *, wait: bool = False) -> dict:
        return await self._act("gesture", wait, name=name)

    async def look(self, at: str | None = None, *, yaw: float | None = None, pitch: float | None = None) -> dict:
        target: Any = at if at is not None else {"yaw": yaw or 0.0, "pitch": pitch or 0.0}
        reply = await self._act("look", False, at=target)
        # Taken, another look ends a following (lamp-control's own rule).
        if self._following is not None:
            self._following.end("cut_short", "another look")
        return reply

    def follow_hand(self, *, for_ms: int = 10_000, why: str | None = None) -> Following:
        """`async with lamp.follow_hand() as following:` turns the head after
        the hand the lamp sees, under a body lease of its own, until the block
        ends, another look, or the lamp takes the body back. Or
        `following = await lamp.follow_hand().start()`, then
        `await following.stop()`. Listen with `lamp.on("hand", ...)` for
        where the hand is."""
        return Following(self, for_ms, why)

    async def move(self, frames: list[list[int]], step_ms: int, *, wait: bool = False) -> dict:
        return await self._act("move", wait, step_ms=step_ms, frames=frames)

    async def play(self, name: str, *, wait: bool = False) -> dict:
        return await self._act("play", wait, name=name)

    async def joints(self, lease: Lease, within_ms: int, **targets: float) -> dict:
        return await self._act("joints", False, lease=lease.id, targets=targets, within_ms=within_ms)

    async def light(self, colour: str, level: int, for_ms: int, pattern: str | None = None) -> dict:
        return await self._act("light", False, colour=colour, level=level, pattern=pattern, for_ms=for_ms)

    async def say(self, text: str) -> dict:
        return await self._act("say", False, text=text)

    async def volume(self, *, to: int | None = None, step: str | None = None, mute: bool | None = None) -> dict:
        """The speaker's volume, the person's own: one of `to` (0-100), `step`
        ("up" or "down"), or `mute` (True or False). Returns the level it
        left, `{"level": ..., "muted": ...}`."""
        given = {k: v for k, v in {"to": to, "step": step, "mute": mute}.items() if v is not None}
        # Exactly one, as the lamp holds it (control.schema.json's `volume`).
        if len(given) != 1:
            raise LampError("volume: one of to, step or mute", code="bad_request")
        reply = await self._act("volume", False, **given)
        return {"level": reply["level"], "muted": reply["muted"]}

    async def sound(self, name: str) -> dict:
        return await self._act("sound", False, name=name)

    async def stop(self) -> dict:
        reply = await self._act("stop", False)
        if self._following is not None:
            # A stop ends what this client asked for; its lease goes too.
            following = self._following
            following.end("cut_short", "stop")
            await following._end()
        return reply

