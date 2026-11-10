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
import base64
import hashlib
import inspect
import json
import logging
import os
import re
import socket
import ssl
import sys
import unicodedata
import warnings
from collections import defaultdict
from pathlib import Path
from typing import Any, Callable

from websockets.asyncio.client import ClientConnection, connect as ws_connect
from websockets.exceptions import ConnectionClosed, WebSocketException

from . import debug, errors, schema
from .errors import BY_CODE, LampError, NotPaired, WrongLamp, from_reply
from .keys import Keyring, Pairing, b64, is_pin, pin_of, proof, unb64
from .moves import POSES_STEP_MS, poses_to_frames

Handler = Callable[[dict], Any]

# A ping this often keeps a connection that only listens open: the lamp
# closes one quiet for 2 minutes (lamp-control's QUIET_MAX, #3778).
KEEPALIVE_S = 20.0
# The actions lamp-control answers `{"to":"body"}` with no `done` after
# (docs/control.md): a wait for them ends at that answer. `say` and
# `turn.start` answer `{"to":"voice"}` and their `done` does follow.
# Ended at their answer by a lamp-control that doesn't say `motion_done` in
# hello's limits (from before #4209): it hands them to the body and says
# nothing back.
ENDS_AT_REPLY = frozenset({"gesture", "move", "express"})
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


def _hex(colour) -> str:
    """`"#rrggbb"` from `"#rrggbb"` or `(r, g, b)`, each 0-255."""
    if isinstance(colour, str):
        return colour
    red, green, blue = colour
    if not all(isinstance(c, int) and 0 <= c <= 255 for c in (red, green, blue)):
        raise LampError(f"a colour's channels are 0 to 255: {colour!r}", code="bad_request")
    return f"#{red:02x}{green:02x}{blue:02x}"


# The longest say's text in UTF-8 bytes: the voice's own cap, which
# lamp-control holds too (protocol.rs SAY_MAX_BYTES, lamp-voice's
# MOST_WORDS_BYTES), checked here at the same bytes, never stricter.
SAY_MAX_BYTES = 900


def _check(op: str, fields: dict) -> None:
    """The names the schema enumerates, checked before sending."""
    if op not in schema.ops():
        raise LampError(f"no op {op!r} in v{schema.VERSION}", code="bad_request")
    text = fields.get("text")
    if op == "say" and isinstance(text, str):
        # Any control character (Unicode Cc, as Rust's char::is_control), a
        # newline or tab too: the door and the voice refuse them.
        if any(unicodedata.category(c) == "Cc" for c in text):
            raise LampError("text: words, no control characters", code="bad_request")
        if len(text.encode("utf-8")) > SAY_MAX_BYTES:
            raise LampError(f"text: at most {SAY_MAX_BYTES} bytes as the voice takes it", code="bad_request")
    # A gesture's name is held to its shape only, as the door holds it: any
    # body clip, `idle` and `scanning` too (never stricter than the lamp).
    name = fields.get("name")
    if op == "gesture" and name is not None and not (isinstance(name, str) and re.fullmatch(schema.gesture_pattern(), name)):
        raise LampError(
            f"name {name!r}: a lowercase letter, then lowercase letters, digits, _ or -, at most 64", code="bad_request"
        )
    allowed = {
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
    # A lease's parts; a `sound.upload`'s or `skill.push`'s `parts` is a count.
    for value in fields.get("parts", ()) if op == "lease.take" else ():
        if value not in schema.parts():
            raise LampError(f"no part {value!r}", code="bad_request")


def binary_frame(message: bytes) -> dict:
    """A binary frame, a fast stream's `camera.frame` or a microphone
    stream's `microphone.audio`: its header, with the JPEG or the samples as
    `data` (bytes)."""
    length = int.from_bytes(message[:4], "big")
    frame = json.loads(message[4 : 4 + length])
    frame["data"] = message[4 + length :]
    return frame


# Events that come with a request, never subscribed to: a camera stream's,
# and a microphone stream's.
CAMERA_EVENTS = ("camera.frame", "camera.ended")
MICROPHONE_EVENTS = ("microphone.audio", "microphone.ended")

# `keep_connected`'s pauses before connecting again, in seconds: the first
# after a closed connection, then longer while the lamp can't be reached (a
# restart takes a few seconds; an update about a minute).
RECONNECT_PAUSES = (1.0, 2.0, 5.0, 10.0)

_log = logging.getLogger("lampos")


def _said(task: "asyncio.Future", name: str) -> None:
    """Says an async handler's exception: nobody else awaits it."""
    if not task.cancelled() and task.exception() is not None:
        _log.error("your handler for %s raised", name, exc_info=task.exception())


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


class Notes:
    """This computer's notes on the lamp: `await lamp.notes.set("mood",
    {"calm": True})`, `get`, `delete`, `list` (see `Lamp.notes_set`)."""

    def __init__(self, lamp: "Lamp") -> None:
        self._lamp = lamp

    async def set(self, key: str, value: Any) -> dict:
        return await self._lamp.notes_set(key, value)

    async def get(self, key: str, default: Any = None) -> Any:
        return await self._lamp.notes_get(key, default)

    async def delete(self, key: str) -> bool:
        return await self._lamp.notes_delete(key)

    async def list(self, prefix: str = "") -> dict:
        return await self._lamp.notes_list(prefix)


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

    @classmethod
    async def keep_connected(
        cls,
        setup: Callable[["Lamp"], Any],
        lamp: str | None = None,
        *,
        host: str | None = None,
        keyring: Keyring | None = None,
        pauses: tuple[float, ...] = RECONNECT_PAUSES,
    ) -> None:
        """Run a long-running program across the lamp's restarts (every update
        restarts it, and the connection closes). Connects, runs
        `await setup(lamp)` (subscribe, ask for leases, as after `connect`),
        and waits until the connection ends; then connects again and runs
        `setup` again on the new connection, after a pause that grows along
        `pauses` while the lamp can't be reached and starts over once it can.

        Never returns by itself: cancel it to stop. Raises at once what
        connecting again can't mend: no pairing (`NotPaired`), another lamp
        at the address (`WrongLamp`), a refusal of the hello (`unpaired`,
        revoked), and anything `setup` raises or a subscription's refusal
        (`no_grant`) that nobody awaited."""
        failures = 0
        while True:
            unreachable: BaseException | None = None
            try:
                connection = await cls.connect(lamp, host=host, keyring=keyring)
            except LampError as error:
                # `closed`: the lamp went while saying hello, as a restart does.
                if error.code != "closed":
                    raise
                unreachable = error
            except (OSError, asyncio.TimeoutError, WebSocketException, EOFError) as error:
                unreachable = error
            if unreachable is not None:
                wait = pauses[min(failures, len(pauses) - 1)]
                if debug.on():
                    print(f"lampos .. not reached ({type(unreachable).__name__}), again in {wait:g} s", file=sys.stderr, flush=True)
                failures += 1
                await asyncio.sleep(wait)
                continue
            failures = 0
            try:
                result = setup(connection)
                if inspect.isawaitable(result):
                    await result
                await connection.wait_closed()
            except LampError as error:
                # The lamp went in the middle of setup: connect again.
                if error.code != "closed":
                    raise
            finally:
                # Quiet when the lamp has gone already: its leases went with it.
                await connection.close()
            await asyncio.sleep(pauses[0])

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

    async def request(self, op: str, *, on_id: Callable[[int], None] | None = None, **fields: Any) -> dict:
        """Send one request and await its reply; a refusal raises its error.
        `on_id(id)` is told the request's id before it is sent, so a caller can
        match a `done` (or any event naming it) that comes before the reply is
        read."""
        _check(op, fields)
        id_ = self._next_id
        self._next_id += 1
        if on_id is not None:
            on_id(id_)
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

    async def _act(self, op: str, wait: bool, *, taken: Callable[[], None] | None = None, **fields: Any) -> dict:
        """An action: its reply when taken, or with `wait` its `done` event;
        for the actions no `done` follows yet (`ENDS_AT_REPLY`), the reply.
        `taken` runs once the lamp has taken it, before any wait for its
        `done`, and never for a refusal: what the lamp ends on taking it ends
        here then, even if the wait is cancelled (Reviewer-two's P2 on #5837).
        `request` takes the next id before it first yields, so this is it."""
        id_ = self._next_id
        # A lamp that says `motion_done` ends `gesture`, `move` and an
        # express's move with a `done` once the body says what became of
        # them; on one that doesn't, their wait ends at the answer, so none
        # is kept for them.
        at_reply = op in ENDS_AT_REPLY and not self.limits.get("motion_done")
        done = asyncio.get_running_loop().create_future() if wait and not at_reply else None
        if done is not None:
            self._done[id_] = done
        try:
            reply = await self.request(op, **{k: v for k, v in fields.items() if v is not None})
        except BaseException:
            if done is not None:
                self._done.pop(id_, None)
            raise
        if taken is not None:
            taken()
        # An express too slight to move (`"move": null`) sends the body
        # nothing, so no `done` follows: its wait ends at the answer.
        if done is not None and op == "express" and reply.get("move") is None:
            self._done.pop(id_, None)
            return reply
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
                if isinstance(text, bytes):
                    # A fast camera frame or a microphone block: the
                    # header's length, the header, then the JPEG or the
                    # samples, given as `data` (bytes). Never logged.
                    self._event(binary_frame(text))
                    continue
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
            # The lamp ends a computer's leases when its last connection
            # closes and tells nobody: none of ours is held any more.
            for lease in self._leases.values():
                if lease.state in ("held", "suspended"):
                    lease.state, lease.lost_why = "lost", "the connection closed"

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
            # A handler that raises must neither end the connection (this runs in the reader) nor vanish:
            # it is said, with its traceback, on the `lampos` logger (stderr by default).
            try:
                result = handler(event)
            except Exception:
                _log.exception("your handler for %s raised", name)
                continue
            if inspect.isawaitable(result):
                task = asyncio.ensure_future(result)
                self._tasks.add(task)
                task.add_done_callback(self._tasks.discard)
                task.add_done_callback(lambda done, name=name: _said(done, name))

    # Events and state.

    def on(self, event: str, handler: Handler, *, phrases: list[str] | None = None) -> "Subscription":
        """Call `handler(event)` for each `event` the lamp says. A handler may
        be a coroutine. Subscribes at once, and returns that subscription:
        `await lamp.on(...)` raises the lamp's refusal (`NoGrantError` without
        the event's grant). Not awaited, a refusal is raised by
        `wait_closed` instead, never lost."""
        if event not in schema.events() and event not in ("lease", "done", *CAMERA_EVENTS, *MICROPHONE_EVENTS):
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
        """The states asked for (`presence`, `light`, `mood`, `lamp`, `body`),
        by name: the reply's `state`."""
        return (await self.request("get", state=list(states))).get("state", {})

    async def air(self) -> dict:
        """The air sensor's newest reading (the `air` grant): `pm1_0`,
        `pm2_5`, `pm4_0`, `pm10`, `co2_ppm`, `humidity`, `temperature`,
        `feel`, and `known`; only `{"known": False}` with no reading. Each
        change past a step comes as `lamp.on("air.changed", ...)`."""
        return (await self.get("air"))["air"]

    async def device(self) -> dict:
        """The lamp's own computer (the `device` grant): `cpu_load`,
        `memory_used`, `disk_free`, `uptime_s`, `temperature_c` by sensor,
        and `known`."""
        return (await self.get("device"))["device"]

    async def body_info(self) -> dict:
        """What the body may do, joint by joint, and where each joint is now:
        `get("body")`'s `body` (`yaw`, `pitch`, `roll`, `shoulder`, `elbow`,
        each with `deg` from its home, `min_deg`, `max_deg` and its counts;
        the speed, load and heat limits), or `known: False` when the body
        has said nothing in the last 15 s. The head's and base's bounds are
        a folded lamp's: a standing lamp's head may sit outside them, so
        don't clamp a target to them; the body holds every look to its
        own."""
        return (await self.get("body")).get("body", {})

    # Leases.

    def lease(self, *parts: str, for_ms: int = 30_000, why: str | None = None) -> Lease:
        """`async with lamp.lease("body"):` holds the body, renewed until the
        block ends or the lamp takes it back."""
        return Lease(self, list(parts), for_ms, why)

    # Actions.

    async def express(self, feeling: str, strength: float | None = None, *, wait: bool = False) -> dict:
        return await self._act("express", wait, feeling=feeling, strength=strength)

    async def gesture(self, name: str, *, wait: bool = False) -> dict:
        return await self._act("gesture", wait, name=name)

    async def look(
        self, at: str | None = None, *, yaw: float | None = None, pitch: float | None = None, wait: bool = False
    ) -> dict:
        """Turn the head (`at`, or `yaw`/`pitch` in degrees). Its answer when
        the door takes it; with `wait`, the body's `done` instead: `finished`
        with where the head got to, or `dropped` with why (`the body is not
        held` while a conversation suspends your lease). A look at `hand` has
        no `done` (`follow_hand`)."""
        if wait and at == "hand":
            raise BY_CODE["bad_request"]("a look at hand is followed until it ends, with no done: use follow_hand")
        target: Any = at if at is not None else {"yaw": yaw or 0.0, "pitch": pitch or 0.0}
        # Taken, another look ends a following (lamp-control's own rule), at
        # once, not after its `done`.
        return await self._act("look", wait, taken=self._another_look, at=target)

    def _another_look(self) -> None:
        if self._following is not None:
            self._following.end("cut_short", "another look")

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

    async def move_poses(self, poses: list[dict], *, hold_ms: int = 400, wait: bool = False) -> dict:
        """A move through `poses`, each the degrees of the joints it turns
        from the standing pose (`{"pitch": 10, "yaw": -8}`), eased there and
        back (`moves.poses_to_frames`). LampError `bad_request`, before sending, for poses
        a move can't make."""
        frames = poses_to_frames(poses, step_ms=POSES_STEP_MS, hold_ms=hold_ms)
        return await self.move(frames, POSES_STEP_MS, wait=wait)

    async def play(self, name: str, *, wait: bool = False) -> dict:
        return await self._act("play", wait, name=name)

    async def joints(self, lease: Lease, within_ms: int, *, wait: bool = False, **targets: float) -> dict:
        """Aim the joints named, in degrees, under `lease`. Its answer when the
        door takes it; with `wait`, the body's `done`, as `look`'s."""
        # lamp-control sends yaw and pitch as the holder's look, so taking it
        # ends a following too (parts.rs `joints`).
        return await self._act("joints", wait, taken=self._another_look, lease=lease.id, targets=targets, within_ms=within_ms)

    async def light(self, colour: str, level: int, for_ms: int, pattern: str | None = None) -> dict:
        return await self._act("light", False, colour=colour, level=level, pattern=pattern, for_ms=for_ms)

    @property
    def ring_pixels(self) -> int | None:
        """How many pixels the ring has, from hello's limits (none from a lamp
        before pixel lights)."""
        return (self.limits.get("ring") or {}).get("pixels")

    async def ring(
        self,
        pixels: list,
        *,
        for_ms: int,
        brightness: int | None = None,
        fade_ms: int | None = None,
    ) -> dict:
        """Light the ring pixel by pixel for `for_ms` (30 s at most): one colour
        per pixel, from pixel 0 around the ring, each `"#rrggbb"` or `(r, g, b)`
        from 0 to 255, exactly `ring_pixels` of them, at `brightness` 0-100
        (100 by default). Under the person's own light and the conversation,
        as `light` is."""
        if self.ring_pixels is not None and len(pixels) != self.ring_pixels:
            raise LampError(f"pixels: {self.ring_pixels} of them, one per pixel of the ring", code="bad_request")
        return await self._act(
            "light", False, pixels=[_hex(pixel) for pixel in pixels], brightness=brightness, fade_ms=fade_ms, for_ms=for_ms
        )

    async def light_rgb(
        self, rgb, *, for_ms: int, brightness: int | None = None, fade_ms: int | None = None
    ) -> dict:
        """Light the whole ring in one colour of your own, `"#rrggbb"` or
        `(r, g, b)`, for `for_ms`, at `brightness` 0-100."""
        return await self._act("light", False, rgb=_hex(rgb), brightness=brightness, fade_ms=fade_ms, for_ms=for_ms)

    async def say(self, text: str, *, rephrase: bool = False, wait: bool = False) -> dict:
        """Say `text` in the lamp's voice, exactly as written, once the person
        isn't talking. `rephrase=True` lets the lamp's model say it in its own
        words instead, as before. `wait=True` returns its `done` once it has
        been said (`finished`), or `dropped`/`cut_short` with the voice's why."""
        return await self._act("say", wait, text=text, rephrase=True if rephrase else None)

    # The camera (the `camera` grant): the eyes' own newest picture, never
    # while the lamp is private.

    async def camera_snapshot(self) -> bytes:
        """The camera's newest picture, a JPEG about 320 across."""
        reply = await self.request("camera.snapshot")
        return base64.b64decode(reply["jpeg"])

    async def camera_stream(self, on: bool = True, fps: int | None = None, size: str | None = None) -> dict:
        """Pictures as `camera.frame` events (`lamp.on("camera.frame", ...)`), at
        most `fps` a second (1 to 30), until `camera_stream(False)`, `stop`, or
        a `camera.ended` event. Where the eyes serve every frame (the answer's
        `fast: true`), each comes as the camera made it, `size` `640x480` (the
        default) or `1280x720`, with the JPEG as `data` (bytes); otherwise the
        eyes' newest frame, about 1 to 2 a second, with `jpeg` in base64. Each
        frame and the answer carry `fps_now`."""
        fields: dict[str, Any] = {"on": on}
        if fps is not None:
            fields["fps"] = fps
        if size is not None:
            fields["size"] = size
        return await self.request("camera.stream", **fields)

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

    # The microphone (the `microphone` grant): the ears' own sound, never
    # while the lamp is private, only on the lamp's own network.

    async def microphone_stream(self, on: bool = True) -> dict:
        """The room's sound as `microphone.audio` events
        (`lamp.on("microphone.audio", ...)`), one each 20 ms block: mono,
        16 kHz, signed 16-bit little-endian samples as `data` (bytes, 640 a
        block), with `sequence` and `lost` (blocks lost since the last one
        sent). Until `microphone_stream(False)`, `stop`, or a
        `microphone.ended` event: the lamp turned private, someone else came
        into view, the room went unseen, the ring didn't show its mark, or no sound came for 2 s
        (the lamp doesn't share its microphone, `LAMP_BUS_AUDIO`)."""
        return await self.request("microphone.stream", on=on)

    async def sound(self, name: str) -> dict:
        """Play `hum`, the lamp's own, or one of this computer's clips by
        name (`upload_sound`), once the person isn't talking."""
        return await self._act("sound", False, name=name)

    async def upload_sound(self, name: str, wav: bytes | str | os.PathLike) -> dict:
        """Keep a clip on the lamp as `name`, over one of the same name, to
        play with `sound(name)`: a WAV of 16-bit mono PCM at 24 kHz, at most
        30 s, within the voice's loudness cap. `wav` is its bytes or a path.
        Sent in parts, as the lamp takes them; the lamp checks it whole and
        refuses it with why. Returns the clip as kept: name, bytes, ms."""
        data = wav if isinstance(wav, bytes) else Path(wav).read_bytes()
        sha256 = hashlib.sha256(data).hexdigest()
        # 36 KiB a part: 48 Ki characters of base64, a part's most.
        step = 36 * 1024
        pieces = [data[at : at + step] for at in range(0, len(data), step)] or [b""]
        reply: dict = {}
        for part, piece in enumerate(pieces):
            reply = await self.request(
                "sound.upload",
                name=name,
                sha256=sha256,
                size=len(data),
                part=part,
                parts=len(pieces),
                data=base64.b64encode(piece).decode(),
            )
        return reply.get("sound", {})

    async def sounds(self) -> list[dict]:
        """This computer's clips on the lamp: each its name, bytes and ms."""
        return (await self.request("sound.list")).get("sounds", [])

    async def remove_sound(self, name: str) -> dict:
        """Delete one of this computer's clips from the lamp."""
        return await self.request("sound.remove", name=name)

    # Notes (the `notes` grant, docs/control.md "Notes"): this computer's own
    # small key/value memory on the lamp. It outlives the program and the
    # lamp's restart, belongs to this computer alone, and goes when the owner
    # lets it go.

    @property
    def notes(self) -> "Notes":
        """`lamp.notes.get/set/delete/list`: this computer's own notes."""
        return Notes(self)

    async def notes_set(self, key: str, value: Any) -> dict:
        """Keep `value` (any JSON, at most 16384 bytes serialized) under
        `key` (up to 64 of letters, digits and `. _ : -`): `{"key", "bytes"}`.
        A computer keeps 256 keys and 256 KiB at most (`limits["notes"]`)."""
        reply = await self.request("notes.set", key=key, value=value)
        return {"key": reply["key"], "bytes": reply["bytes"]}

    async def notes_get(self, key: str, default: Any = None) -> Any:
        """The value under `key`, or `default` when none is kept (a stored
        `None` is returned as `None`)."""
        reply = await self.request("notes.get", key=key)
        return reply["value"] if reply.get("found") else default

    async def notes_delete(self, key: str) -> bool:
        """Drop `key`; whether it was kept."""
        return bool((await self.request("notes.delete", key=key)).get("deleted"))

    async def notes_list(self, prefix: str = "") -> dict:
        """This computer's keys that start with `prefix`, sorted:
        `keys` (each `{"key", "bytes"}`), `count`, `total`, `bytes`,
        `keys_max` and `bytes_max`."""
        reply = await self.request("notes.list", prefix=prefix) if prefix else await self.request("notes.list")
        return {k: v for k, v in reply.items() if k not in ("re", "ok")}

    # Turns and listening (docs/control.md): the person wins over each.

    async def start_turn(self, prompt: str | None = None, *, wait: bool = False) -> dict:
        """Open a conversation turn, as a tap on the head does: the person's
        next line is answered as spoken to the lamp. With `prompt`, the lamp
        says it first, as a `say`. The reply is `{"to": "voice"}`; with
        `wait`, its `done`: `finished` once the turn's answer has sounded,
        else `dropped` with the voice's why. Needs speak and hear."""
        return await self._act("turn.start", wait, prompt=prompt)

    async def end_turn(self) -> dict:
        """End the turn this computer started, never another's: the reply is
        `{"turn": "ended", "started": <the turn.start's id>}`. With no turn
        of this computer's, `BusyError`."""
        return await self._act("turn.end", False)

    async def set_listening(self, on: bool) -> dict:
        """`False` holds this computer's mute (`{"mute": "held"}`) until
        `set_listening(True)` (`{"mute": "lifted"}`, or `"nothing_held"`),
        this computer goes, or a hand on the head lifts it: then a `done`
        `cut_short`, "a hand on the head", names the request that held it
        (`lamp.on("done", ...)`). A `stop` doesn't lift it. Needs hear."""
        return await self._act("listen.set", False, on=on)

    async def stop(self) -> dict:
        reply = await self._act("stop", False)
        if self._following is not None:
            # A stop ends what this client asked for; its lease goes too.
            following = self._following
            following.end("cut_short", "stop")
            await following._end()
        return reply

    # Skills of this computer's own, put on the lamp (the `skills` grant;
    # docs/control.md, "Skills pushed from a computer").

    async def push_skill(self, folder) -> dict:
        """Check the skill folder as `lamp skill check` does, pack it, and send
        it in parts; returns the lamp's `skill` (what it asks, whether it's
        on or held for the owner's yes). A check error, or a grant this
        pairing lacks, refuses it here, before any byte is sent."""
        from .push import asks_beyond, pack
        from .skillcheck import check

        report = check(folder, shape_required=False)
        if not report.ok:
            raise LampError(f"{report.folder.name}: {report.errors[0]}", code="bad_request")
        for warning in report.warnings:
            # Said, not swallowed: a push without `lampos[check]` goes on, and
            # the developer hears that the lamp checks the shape instead.
            warnings.warn(f"{report.folder.name}: {warning}", stacklevel=2)
        if "skills" not in self.grants:
            raise BY_CODE["no_grant"]("not granted: skills")
        beyond = asks_beyond(folder, self.grants)
        if beyond:
            code, why = beyond
            raise BY_CODE[code](why)
        package = pack(folder)
        parts = package.parts()
        reply: dict = {}
        for part, data in enumerate(parts):
            reply = await self.request(
                "skill.push",
                name=package.name,
                sha256=package.sha256,
                size=len(package.data),
                part=part,
                parts=len(parts),
                data=data,
            )
        return reply.get("skill", {})

    async def skills(self) -> list[dict]:
        """This computer's own pushed skills."""
        return (await self.request("skill.list")).get("skills", [])

    async def remove_skill(self, name: str) -> dict:
        """Take one of this computer's own pushed skills off the lamp."""
        return await self.request("skill.remove", name=name)

