"""A fake lamp for tests: the v1 protocol on localhost, no lamp needed.

    async with FakeLamp(code="12345678") as fake:
        pairing = await Lamp.pair("12345678", fake.host, pin=fake.pin, port=fake.port, keyring=ring)
        async with await Lamp.connect(keyring=ring) as lamp:
            await lamp.gesture("nod")
        assert fake.asked("gesture") == [{"name": "nod"}]

It speaks docs/control.schema.json: a welcome with a fresh nonce, pairing
checked against its code, hello checked against the paired key, and every
request validated against the schema when `jsonschema` is installed. It does
what a test tells it (`emit`, `refuse_next`, `lose_lease`), and keeps every
request it took. It moves nothing, so it never stands in for a lamp check.
"""

from __future__ import annotations

import asyncio
import datetime
import ipaddress
import json
import math
import os
import ssl
import tempfile
import time
from collections import defaultdict
from typing import Any

from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey
from cryptography.hazmat.primitives.asymmetric.ec import SECP256R1, generate_private_key
from cryptography.x509.oid import NameOID
from websockets.asyncio.server import ServerConnection, serve

from . import schema
from .keys import HELLO, b64, pin_of, proof, unb64

try:
    import jsonschema
except ImportError:  # validation is a test nicety, not a need
    jsonschema = None


def _certificate() -> tuple[bytes, bytes]:
    key = generate_private_key(SECP256R1())
    name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "fake-lamp")])
    now = datetime.datetime.now(datetime.timezone.utc)
    cert = (
        x509.CertificateBuilder()
        .subject_name(name)
        .issuer_name(name)
        .public_key(key.public_key())
        .serial_number(x509.random_serial_number())
        .not_valid_before(now - datetime.timedelta(minutes=1))
        .not_valid_after(now + datetime.timedelta(days=1))
        .add_extension(x509.SubjectAlternativeName([x509.IPAddress(ipaddress.ip_address("127.0.0.1"))]), critical=False)
        .sign(key, hashes.SHA256())
    )
    pem_key = key.private_bytes(
        serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8, serialization.NoEncryption()
    )
    return cert.public_bytes(serialization.Encoding.PEM), pem_key


# `look`'s targets lamp-control doesn't route yet, in parts.rs's words.
LOOK_WAITS = {
    "face": "not in this release: look at a face or a name",
    "name": "not in this release: look at a face or a name",
}

# What lamp-control refuses today, in its own words (crates/control/src/
# parts.rs, `waits_for`): pass it as `refuses=` for a fake that answers as a
# lamp does now. `lamp fake` does.
LAMP_CONTROL_REFUSES = {
    # No product unit passes --lease-socket yet (docs/control.md).
    "look": "the body's lease socket is not on here",
    "joints": "not routed yet: waits for a goal stream on the body's lease socket",
    "play": "not routed yet: waits for client packages",
    "sound": "not routed yet: waits for client packages",
}


# `express`'s feelings and their moves, as lamp-control's feelings.rs.
FEELING_MOVES = {"glad": "glad-lift", "lively": "perk", "low": "low-droop"}


# `low` goes no smaller than three quarters (feelings.rs `least_step`).
FEELING_LEAST_STEP = {"glad": 1, "lively": 1, "low": 3}


def _feeling_move(request: dict) -> str | None:
    """The move lamp-control sends for this strength: the skill's own from
    0.5 up, below it a quarter step of it, by its own name; none under 0.0625.
    Halves round up, as Rust's and JS's do (jarvis's P3 on #4020)."""
    step = math.floor(min(max(request.get("strength", 0.5) / 0.5, 0.0), 1.0) * 4 + 0.5)
    if step == 0:
        return None
    step = max(step, FEELING_LEAST_STEP[request["feeling"]])
    name = FEELING_MOVES[request["feeling"]]
    return name if step == 4 else f"{name}-{step * 25}"


# A light's pattern, as lamp-control checks it before the ring (lamp_msgs'
# LightPattern::check, with the breath's low at 1): the least level each is
# seen at, and the words it refuses with.
PATTERN_LEAST_LEVEL = {
    "breathe": (2, "a breath's low is from 1 to under the light's level"),
    "candle": (3, "a candle flickers at level 3 and up"),
    "pulse": (2, "a pulse is seen at level 2 and up"),
}
# The ring plays one pulse a minute for one asker, and drops a second without a
# word to the computer (lamp_msgs' PULSE_GAP_US): the door has answered by then.
PULSE_GAP_S = 60.0


def _now() -> str:
    return datetime.datetime.now().astimezone().isoformat(timespec="seconds")


class FakeLamp:
    """A lamp that speaks the protocol on 127.0.0.1 and records what it's asked."""

    LEASE_OPS = {"look", "move", "play", "joints"}

    def __init__(
        self,
        *,
        code: str = "12345678",
        grants: tuple[str, ...] = ("body", "light", "speak"),
        lamp_id: str = "lamp-fake",
        name: str = "Fake lamp",
        auto_done: bool = True,
        refuses: dict[str, str] | None = None,
    ):
        self.code = code
        # Ops refused `refused` with this why, as a lamp that doesn't route
        # them yet does (LAMP_CONTROL_REFUSES); none by default.
        self.refuses = dict(refuses or {})
        # gesture, move and play end with `done` at once; False leaves it to
        # the test (`done`).
        self.auto_done = auto_done
        self._asker: dict[int, ServerConnection] = {}
        self.grants = list(grants)
        self.lamp = {"id": lamp_id, "name": name, "v": schema.VERSION}
        self.host = "127.0.0.1"
        self.port = 0
        # The certificate's pin (full SHA-256), as the owner's app shows it,
        # and mDNS's 16-hex hint of it.
        self.pin = ""
        self.fp = ""
        self.clients: dict[str, dict] = {}
        self.requests: list[dict] = []
        # Every frame that arrived, taken or refused.
        self.received: list[dict] = []
        self.leases: dict[str, dict] = {}
        # The newest light a computer asked for, and until when (the fake's
        # `clock`): what the ring shows while it lasts (`light_state`).
        self.shown: tuple[dict, float] | None = None
        # Each pulse the ring would play, and each it would drop as a second
        # within a minute from the same computer: (client, the light request).
        self.pulses_played: list[tuple[str, dict]] = []
        self.pulses_dropped: list[tuple[str, dict]] = []
        self._pulsed: dict[str, float] = {}
        self.clock = time.monotonic
        self.subscribed: dict[str, set[str]] = defaultdict(set)
        self.state: dict[str, Any] = {
            "presence": {"count": 0, "near": 0.0, "facing": False},
            # null until the lamp has heard them, as lamp-control's Watch answers.
            "light": None,
            "mood": None,
            "lamp": {"talking": False, "paused": False, "private": False, "asleep": False},
        }
        self._refusals: dict[str, list[tuple[str, str, int | None]]] = defaultdict(list)
        self._connections: set[ServerConnection] = set()
        self._next_lease = 1
        self._server = None
        self._validator = (
            jsonschema.Draft202012Validator({**schema.schema(), "oneOf": [{"$ref": "#/$defs/request"}]})
            if jsonschema
            else None
        )

    async def __aenter__(self) -> "FakeLamp":
        cert, key = _certificate()
        self.pin = pin_of(x509.load_pem_x509_certificate(cert).public_bytes(serialization.Encoding.DER))
        self.fp = self.pin[:16]
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        with tempfile.TemporaryDirectory() as folder:
            cert_path, key_path = os.path.join(folder, "c.pem"), os.path.join(folder, "k.pem")
            with open(cert_path, "wb") as file:
                file.write(cert)
            with open(key_path, "wb") as file:
                file.write(key)
            context.load_cert_chain(cert_path, key_path)
        self._server = await serve(self._serve, self.host, 0, ssl=context)
        self.port = self._server.sockets[0].getsockname()[1]
        return self

    async def __aexit__(self, *_exc) -> None:
        self._server.close()
        await self._server.wait_closed()

    # What a test asks of it.

    def asked(self, op: str) -> list[dict]:
        """The fields of every `op` request taken, oldest first."""
        return [
            {k: v for k, v in request.items() if k not in ("id", "op")}
            for request in self.requests
            if request["op"] == op
        ]

    def refuse_next(self, op: str, code: str, why: str = "refused by the test", retry_ms: int | None = None) -> None:
        """The next `op` is answered with this error."""
        self._refusals[op].append((code, why, retry_ms))

    async def emit(self, event: str, **fields: Any) -> None:
        """Say `event` to every connection that subscribed to it."""
        frame = json.dumps({"ev": event, "at": _now(), **fields})
        for ws in list(self._connections):
            if event in self.subscribed[id(ws)]:
                await ws.send(frame)

    def _show(self, request: dict) -> None:
        """The ring shows a computer's light while it lasts."""
        light = {"on": True, "colour": request["colour"], "level": request["level"]}
        self.shown = (light, self.clock() + request["for_ms"] / 1000)

    def light_state(self) -> dict:
        """`get`'s light, as lamp-control answers it (Nine's light/drawn): the
        person's own (`state["light"]`: on, colour, level) and `shows`, what the
        ring draws now. That's the newest light a computer asked for while it
        lasts, else the person's own. With neither, no `shows`. No pattern.
        null with nothing to say, as lamp-control answers before it has heard
        light/state or light/drawn. The person's own stands in for light/drawn
        here, as the ring publishes both when it draws the person's light."""
        own = dict(self.state.get("light") or {})
        if self.shown is not None and self.shown[1] > self.clock():
            own["shows"] = dict(self.shown[0])
        elif any(key in own for key in ("on", "colour", "level")):
            own["shows"] = {key: own[key] for key in ("on", "colour", "level") if key in own}
        return own or None

    async def lease_event(self, lease: str, state: str, why: str | None = None) -> None:
        """Tell a lease's holder its lease is `held`, `suspended` or `lost`
        (and, lost, end it here), as the lamp would."""
        holder = self.leases.get(lease)
        if holder is None:
            return
        if state == "lost":
            self.leases.pop(lease)
        event: dict[str, Any] = {"ev": "lease", "lease": lease, "state": state}
        if why is not None:
            event["why"] = why
        await holder["ws"].send(json.dumps(event))

    async def lose_lease(self, lease: str, why: str = "hand") -> None:
        """The lamp takes a lease back, as a hand on the head would."""
        await self.lease_event(lease, "lost", why)

    async def done(self, re: int, how: str = "finished", why: str | None = None) -> None:
        """End the action whose request id was `re`, to the client that asked."""
        await self._send_done(re, {"how": how} if why is None else {"how": how, "why": why})

    async def _send_done(self, re: int, fields: dict[str, Any]) -> None:
        ws = self._asker.get(re)
        if ws is not None:
            await ws.send(json.dumps({"ev": "done", "re": re, **fields}))

    # The protocol.

    async def _serve(self, ws: ServerConnection) -> None:
        nonce = os.urandom(32)
        client: str | None = None
        self._connections.add(ws)
        try:
            await ws.send(json.dumps({"ev": "welcome", "at": _now(), "nonce": b64(nonce), "lamp": self.lamp}))
            async for text in ws:
                try:
                    request = json.loads(text)
                except json.JSONDecodeError:
                    await self._refuse(ws, None, "bad_request", "not JSON")
                    continue
                self.received.append(request)
                id_ = request.get("id") if isinstance(request.get("id"), int) else None
                if self._validator is not None:
                    errors = list(self._validator.iter_errors(request))
                    if errors:
                        await self._refuse(ws, id_, "bad_request", errors[0].message)
                        continue
                op = request.get("op")
                if op == "pair":
                    await self._pair(ws, request, nonce)
                    continue
                if op == "hello":
                    client = await self._hello(ws, request, nonce)
                    continue
                if client is None:
                    await self._refuse(ws, id_, "no_hello", "hello first")
                    continue
                await self._op(ws, client, request)
        finally:
            self._connections.discard(ws)
            self.subscribed.pop(id(ws), None)
            for lease in [l for l, held in self.leases.items() if held["ws"] is ws]:
                self.leases.pop(lease)

    async def _refuse(self, ws, re, code: str, why: str, retry_ms: int | None = None) -> None:
        error: dict[str, Any] = {"code": code, "why": why}
        if retry_ms is not None:
            error["retry_ms"] = retry_ms
        await ws.send(json.dumps({"re": re, "ok": False, "error": error}))

    async def _ok(self, ws, re, **fields: Any) -> None:
        await ws.send(json.dumps({"re": re, "ok": True, **fields}))

    async def _pair(self, ws, request: dict, nonce: bytes) -> None:
        key = unb64(request["key"])
        if unb64(request["proof"]) != proof(self.code, bytes.fromhex(self.pin), key, nonce):
            await self._refuse(ws, request["id"], "unpaired", "the code does not match")
            return
        client = f"c_{len(self.clients) + 1:04x}"
        self.clients[client] = {"key": key, "name": request["name"], "grants": list(self.grants)}
        await self._ok(ws, request["id"], client=client, grants=self.grants)

    async def _hello(self, ws, request: dict, nonce: bytes) -> str | None:
        paired = self.clients.get(request["client"])
        try:
            if paired is None:
                raise ValueError
            Ed25519PublicKey.from_public_bytes(paired["key"]).verify(unb64(request["sig"]), HELLO + nonce)
        except Exception:
            await self._refuse(ws, request["id"], "unpaired", "not paired, or a bad signature")
            await ws.close()
            return None
        await self._ok(ws, request["id"], lamp=self.lamp, grants=paired["grants"], limits={"joints_hz": 40})
        return request["client"]

    # The grant each event and state needs, as lamp-control's own (#3736).
    EVENT_GRANT = {
        "person.arrived": "look", "person.left": "look", "person.stood_up": "look", "person.facing": "look",
        "hand": "look",
        "phrase.heard": "hear", "name.heard": "hear", "sound.heard": "sound",
    }

    def _needs(self, op: str, request: dict) -> list[str]:
        if op == "subscribe":
            return sorted({self.EVENT_GRANT[e] for e in request.get("events", []) if e in self.EVENT_GRANT})
        if op == "get":
            return ["look"] if "presence" in request.get("state", []) else []
        body = {"gesture", "move", "play", "joints", "look"}
        if op in body:
            return ["body", "look"] if op == "look" and request.get("at") in ("face", "hand") else ["body"]
        return {
            "express": ["body", "light"],
            "light": ["light"],
            "say": ["speak"],
            "sound": ["speak"],
            "lease.take": [{"body": "body", "light": "light", "voice": "speak"}[p] for p in request.get("parts", [])],
        }.get(op, [])

    async def _op(self, ws, client: str, request: dict) -> None:
        op, id_ = request["op"], request["id"]
        grants = self.clients[client]["grants"]
        missing = [g for g in self._needs(op, request) if g not in grants]
        if missing:
            await self._refuse(ws, id_, "no_grant", f"needs {missing[0]}")
            return
        if self._refusals[op]:
            code, why, retry_ms = self._refusals[op].pop(0)
            await self._refuse(ws, id_, code, why, retry_ms)
            return
        if op in self.LEASE_OPS:
            held = [l for l, h in self.leases.items() if h["ws"] is ws and "body" in h["parts"]]
            if not held or (op == "joints" and request["lease"] not in held):
                await self._refuse(ws, id_, "no_lease", "take a body lease first")
                return
        if op in self.refuses:
            await self._refuse(ws, id_, "refused", self.refuses[op])
            return
        self.requests.append(request)
        self._asker[id_] = ws
        if op == "subscribe":
            self.subscribed[id(ws)].update(request["events"])
            await self._ok(ws, id_)
        elif op == "unsubscribe":
            self.subscribed[id(ws)].difference_update(request["events"])
            await self._ok(ws, id_)
        elif op == "get":
            # As lamp-control's session.rs answers `get`: the states under `state`, each by the name asked (`lamp`, not
            # `lamp.changed`'s `lamp_state`) (harness-reviewer's lamp run on 75c9, 28 Oct).
            await self._ok(ws, id_, state={k: self.light_state() if k == "light" else self.state.get(k) for k in request["state"]})
        elif op == "lease.take":
            if any(set(request["parts"]) & set(h["parts"]) for h in self.leases.values()):
                await self._refuse(ws, id_, "busy", "another client holds it")
                return
            lease = f"l_{self._next_lease}"
            self._next_lease += 1
            self.leases[lease] = {"ws": ws, "parts": request["parts"]}
            await self._ok(ws, id_, lease=lease, until=_now())
        elif op == "lease.renew":
            if request["lease"] not in self.leases:
                await self._refuse(ws, id_, "lease_lost", "that lease has ended")
                return
            await self._ok(ws, id_, lease=request["lease"], until=_now())
        elif op == "lease.release":
            self.leases.pop(request["lease"], None)
            await self._ok(ws, id_)
        elif op in ("gesture", "move"):
            # As lamp-control routes them: the body takes them, and no
            # `done` follows.
            await self._ok(ws, id_, to="body")
        elif op == "light":
            # As lamp-control's parts.rs: the pattern checked first, then to the
            # ring, which keeps its own one-pulse-a-minute (not said back).
            least, why = PATTERN_LEAST_LEVEL.get(request.get("pattern"), (1, ""))
            if request["level"] < least:
                self.requests.remove(request)
                await self._refuse(ws, id_, "refused", why)
                return
            if request.get("pattern") == "pulse":
                now = self.clock()
                if now - self._pulsed.get(client, -PULSE_GAP_S) < PULSE_GAP_S:
                    # The ring shows it without its pulse (ring_light.rs's
                    # start_moving starts it "as already played"): its colour
                    # and level steady, the old light replaced.
                    self.pulses_dropped.append((client, request))
                else:
                    self._pulsed[client] = now
                    self.pulses_played.append((client, request))
            self._show(request)
            await self._ok(ws, id_, to="ring")
        elif op == "express":
            # As lamp-control's feelings.rs: the feelings skill's three, its
            # move to the body (smaller below strength 0.5) and its light; no
            # `done` follows. The fake always shows the light.
            if request["feeling"] not in FEELING_MOVES:
                await self._refuse(ws, id_, "bad_request", "feeling: one of glad, lively, low")
                return
            await self._ok(ws, id_, to="body", move=_feeling_move(request), light=True)
        elif op == "look":
            # As lamp-control with --lease-socket (parts.rs `look`): `face`
            # and `name` aren't routed; `hand` is followed until a stop,
            # another look or the lease ends, so no `done` follows it; the
            # body takes the rest, and its `done` says where the head got to.
            if isinstance(request["at"], str) and request["at"] in LOOK_WAITS:
                await self._refuse(ws, id_, "refused", LOOK_WAITS[request["at"]])
                return
            if request["at"] == "hand":
                await self._ok(ws, id_, to="body", follow="hand")
                return
            await self._ok(ws, id_, to="body")
            if self.auto_done:
                at = request["at"] if isinstance(request["at"], dict) else {"yaw": 0.0, "pitch": 0.0}
                await self._send_done(id_, {"how": "finished", "yaw": at["yaw"], "pitch": at["pitch"], "clamped": False})
        elif op == "say":
            # As lamp-control: the voice takes it, and its `done` follows.
            await self._ok(ws, id_, to="voice")
            if self.auto_done:
                await self.done(id_)
        else:
            await self._ok(ws, id_)
            if op == "play" and self.auto_done:
                await self.done(id_)
