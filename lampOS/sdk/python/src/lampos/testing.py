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
import base64
import datetime
import hashlib
import io
import ipaddress
import json
import math
import os
import random
import ssl
import tempfile
import time
import wave
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
from .moves import move_refusal

# The fake's camera picture: the smallest whole JPEG (start and end markers).
FAKE_JPEG = bytes([0xFF, 0xD8, 0xFF, 0xD9])

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
# The ring plays one pulse a minute for one asker, and shows a second steady,
# without its pulse (lamp_msgs' PULSE_GAP_US): the door has answered by then.
PULSE_GAP_S = 60.0

# Hello's `limits`, as lamp-control says them with its rates off (session.rs
# `limits`): no rate but its two guards, refused actions and phrase changes.
LIMITS = {
    "rates": "off",
    "refused": [{"most": 20, "within_ms": 1_000}],
    "phrases": [{"most": 6, "within_ms": 60_000}],
    "hold_max_ms": 30_000,
    "lease_max_ms": 600_000,
    "say_max_bytes": 900,
    "sounds": {
        "rate_hz": 24_000,
        "clip_max_ms": 30_000,
        "clip_max_bytes": 1_444_140,
        "part_max_chars": 49_152,
        "parts_max": 64,
        "clips_max": 64,
        "store_max_bytes": 20_000_000,
    },
    "ring": {"pixels": 32},
    "quiet_hours": "off",
}

def _clip(name: str, data: bytes) -> dict:
    """A clip as sound.upload and sound.list say it."""
    frames = _frames_of(data) or 0
    return {"name": name, "bytes": len(data), "ms": frames * 1_000 // 24_000}


def _frames_of(data: bytes) -> int | None:
    try:
        with wave.open(io.BytesIO(data)) as read:
            if read.getnchannels() != 1 or read.getsampwidth() != 2 or read.getframerate() != 24_000:
                return None
            return read.getnframes()
    except (wave.Error, EOFError):
        return None


def _clip_refusal(data: bytes) -> str | None:
    """Why lamp-control would refuse this clip, its format and length only."""
    try:
        with wave.open(io.BytesIO(data)):
            pass
    except (wave.Error, EOFError):
        return "not a WAV file"
    frames = _frames_of(data)
    if frames is None:
        return "not 16-bit mono PCM at 24000 Hz"
    if not 1 <= frames <= 24_000 * 30:
        return "not 1 to 720000 frames (30 s at most)"
    return None


# How a synthetic hand moves (`hand_track`): across the picture and back
# (`wave`), round in a circle, left to right once, or held still.
HAND_PATHS = ("wave", "circle", "across", "still")
# How lamp-control turns `humans/hands/tracked` into `hand` events (Nine's
# watch.rs): told when first seen, moved or grown by `HAND_STEP`, or opened
# or closed, at most every `HAND_EVERY_US` with the newest held back until
# then; `seen: false` once, `HAND_GONE_US` after the last message. A message
# under `HAND_SURE`, or a share outside 0 to 1, is no hand. Its check runs
# every `HAND_CHECK_US`.
HAND_STEP = 0.05
HAND_EVERY_US = 250_000
HAND_GONE_US = 1_000_000
HAND_SURE = 0.5
HAND_CHECK_US = 250_000
HAND_FIELDS = ("x", "y", "size", "open", "confidence")


def hand_track(
    path: str = "wave", *, seconds: float = 3.0, hz: float = 15.0, open: bool = True, seed: int = 0, t0: int = 0
) -> list[dict]:
    """A synthetic hand track in `humans/hands/tracked`'s shape (Ten's): `x`
    and `y` the palm centre as shares of the picture from its left and top,
    `size` the palm box's width over the picture's, `open`, `confidence`, and
    `t` the lamp's monotonic clock in microseconds, from `t0`. A seeded jitter
    of a hundredth keeps it from being a perfect line, and the same seed gives
    the same track."""
    if path not in HAND_PATHS:
        raise ValueError(f"path: one of {', '.join(HAND_PATHS)}")
    jitter = random.Random(seed)
    frames = max(1, round(seconds * hz))
    track = []
    for n in range(frames):
        share = n / max(frames - 1, 1)
        angle = 2 * math.pi * share
        x, y = {
            "wave": (0.5 + 0.3 * math.sin(angle), 0.45),
            "circle": (0.5 + 0.25 * math.cos(angle), 0.5 + 0.25 * math.sin(angle)),
            "across": (0.15 + 0.7 * share, 0.5),
            "still": (0.5, 0.5),
        }[path]
        track.append(
            {
                "x": round(min(max(x + jitter.uniform(-0.01, 0.01), 0.0), 1.0), 4),
                "y": round(min(max(y + jitter.uniform(-0.01, 0.01), 0.0), 1.0), 4),
                "size": round(0.12 + jitter.uniform(-0.01, 0.01), 4),
                "open": open,
                "confidence": round(0.9 + jitter.uniform(-0.05, 0.05), 3),
                "t": t0 + round(n * 1_000_000 / hz),
            }
        )
    return track


def _two(value: float) -> float:
    # Rust's `round`: halves away from zero (the shares are never negative).
    return math.floor(value * 100 + 0.5) / 100


def hand_events(track: list[dict]) -> list[tuple[int, dict]]:
    """The `hand` events lamp-control says for a `humans/hands/tracked`
    track, as its watch.rs does, each with the topic time (µs) it is said
    at: `seen` with where the hand is, rounded to 2 places, never the
    tracker's clock; `seen: false` once it's gone. Each message is taken at
    its `t`, and the 250 ms check runs from the first."""
    events: list[tuple[int, dict]] = []
    newest: tuple[dict, int] | None = None
    told: dict | None = None
    told_us: int | None = None

    def news(now: int) -> None:
        nonlocal newest, told, told_us
        if newest is None:
            return
        hand, heard = newest
        if now - heard >= HAND_GONE_US:
            newest = None
            if told is None:
                return
            told, told_us = None, now
            events.append((now, {"seen": False}))
            return
        fresh = told is None or hand["open"] != told["open"] or any(
            abs(hand[k] - told[k]) >= HAND_STEP for k in ("x", "y", "size")
        )
        if not fresh or (told_us is not None and now - told_us < HAND_EVERY_US):
            return
        told, told_us = hand, now
        events.append((now, {"seen": True, **{k: hand[k] if k == "open" else _two(hand[k]) for k in HAND_FIELDS}}))

    check = track[0]["t"] + HAND_CHECK_US if track else 0
    for hand in track:
        while check < hand["t"]:
            news(check)
            check += HAND_CHECK_US
        if hand["confidence"] < HAND_SURE or not all(0 <= hand[k] <= 1 for k in ("x", "y", "size", "confidence")):
            continue
        newest = (hand, hand["t"])
        news(hand["t"])
    while newest is not None:
        news(check)
        check += HAND_CHECK_US
    return events


def _whole(value: Any) -> bool:
    return isinstance(value, int) and not isinstance(value, bool)


def _frames(frames: Any) -> bool:
    """Whether a move's frames are what lamp-control parses (`Vec<[i32; 5]>`),
    so its check can read them."""
    return isinstance(frames, list) and all(
        isinstance(frame, list) and len(frame) == 5 and all(_whole(count) for count in frame) for frame in frames
    )


def _now() -> str:
    # As lamp-control stamps it (server.rs `rfc3339_utc`): UTC, to the second, ending `Z`.
    return datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


# A made-up air reading and computer, as lamp-control's `get` answers them
# (watch.rs `air_reading` and `device_reading`): the fake lamp's own.
AIR = {"pm1_0": 2.1, "pm2_5": 5.3, "pm4_0": 6.8, "pm10": 7.9, "co2_ppm": 620, "humidity": 45.0, "temperature": 27.4,
       "feel": "fresh", "known": True}
DEVICE = {"cpu_load": 0.25, "memory_used": 0.4, "disk_free": 0.6, "uptime_s": 3600,
          "temperature_c": {"cpu_thermal": 55.0}, "known": True}

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
        follows_hand: bool = True,
    ):
        self.code = code
        # Ops refused `refused` with this why, as a lamp that doesn't route
        # them yet does (LAMP_CONTROL_REFUSES); none by default.
        self.refuses = dict(refuses or {})
        # False answers a look at `hand` as a lamp-control from before #4183
        # does: taken, with no `follow`, and nothing follows it.
        self.follows_hand = follows_hand
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
        # Each computer's clips, by name (`sound.upload`), and its upload in
        # flight: as lamp-control's sounds.rs, without its loudness check.
        self.clips: dict[str, dict[str, bytes]] = defaultdict(dict)
        self._uploads: dict[str, dict] = {}
        # The newest light a computer asked for, and until when (the fake's
        # `clock`): what the ring shows while it lasts (`light_state`).
        self.shown: tuple[dict, float] | None = None
        # Each pulse the ring would play, and each it would drop as a second
        # within a minute from the same computer: (client, the light request).
        self.pulses_played: list[tuple[str, dict]] = []
        self.pulses_dropped: list[tuple[str, dict]] = []
        self._pulsed: dict[str, float] = {}
        self.clock = time.monotonic
        # Pushed skills by name, and the one push in flight on the lamp.
        self.pushed: dict[str, dict] = {}
        self._pushing: dict | None = None
        self.subscribed: dict[str, set[str]] = defaultdict(set)
        self.state: dict[str, Any] = {
            "presence": {"count": 0, "near": 0.0, "facing": False},
            # null until the lamp has heard them, as lamp-control's Watch answers.
            "light": None,
            "mood": None,
            "lamp": {"talking": False, "paused": False, "private": False, "asleep": False},
            # Made-up readings, as lamp-control answers them; `set_air` changes
            # the air and tells `air.changed`, `{"known": False}` for none.
            "air": dict(AIR),
            "device": dict(DEVICE),
            "volume": {"level": 50, "muted": False},
            # A lamp that may stand, as lamp-control's `get` `body` answers it.
            "body": {"known": True, "joints": {"yaw": {"counts": 2015.0, "deg": 0.0, "min_deg": -3.5, "max_deg": 3.5, "min_counts": 1975.0, "max_counts": 2055.0, "home_counts": 2015.0, "folded_counts": None, "stand_counts": None}, "shoulder": {"counts": 2050.0, "deg": 0.0, "min_deg": -7.0, "max_deg": 59.8, "min_counts": 1970.0, "max_counts": 2730.0, "home_counts": 2050.0, "folded_counts": 2050.0, "stand_counts": 2650.0}, "elbow": {"counts": 2047.0, "deg": 0.0, "min_deg": -7.0, "max_deg": 55.6, "min_counts": 1967.0, "max_counts": 2680.0, "home_counts": 2047.0, "folded_counts": 2047.0, "stand_counts": 2600.0}, "roll": {"counts": 2044.0, "deg": 0.0, "min_deg": -2.6, "max_deg": 2.6, "min_counts": 2014.0, "max_counts": 2074.0, "home_counts": 2044.0, "folded_counts": None, "stand_counts": None}, "pitch": {"counts": 2000.0, "deg": 0.0, "min_deg": -1.3, "max_deg": 4.0, "min_counts": 1955.0, "max_counts": 2015.0, "home_counts": 2000.0, "folded_counts": None, "stand_counts": None}}, "stands": True, "speed_deg_s": 4.4, "acceleration_deg_s2": 35.2, "load_limit": 0.6, "heat_stop_c": 62, "counts_per_turn": 4096.0},
        }
        # Where an unmute brings it back to (lamp-actions' `before_mute`).
        self._before_mute = 50
        self._refusals: dict[str, list[tuple[str, str, int | None]]] = defaultdict(list)
        # Each connection whose head follows the hand (`look` at `hand`) and
        # its lease, until its stop, another look, or the lease ends; and the
        # last hand said.
        self.following: dict[int, tuple[ServerConnection, str]] = {}
        self.hand: dict | None = None
        # Each computer's own turn, by the id of the turn.start that opened
        # it, until its `done`; and each computer's mute, held by the
        # connection and request id of its listen.set false, until it says
        # true, its last connection closes or `hand_on_head` lifts it.
        self.turns: dict[str, int] = {}
        self.mutes: dict[str, tuple[ServerConnection, int]] = {}
        self._client_of: dict[int, str] = {}
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

    async def set_air(self, **reading: Any) -> None:
        """The air sensor reads `reading` (`pm2_5=40.0, co2_ppm=1300, feel="stuffy"`, ...): `get` `air` answers it,
        and `air.changed` is told, as lamp-control tells a change past a step. With nothing, no sensor:
        `{"known": False}`."""
        self.state["air"] = {**reading, "known": True} if reading else {"known": False}
        if reading:
            await self.emit("air.changed", **reading)

    async def set_volume(self, level: int) -> None:
        """The person changes the volume (by voice or in the app), or a
        `volume` op does: `get("volume")` follows, and subscribers hear
        `volume.changed` when it changed, as lamp-control tells it from
        `speaker/volume`."""
        level = max(0, min(100, round(level)))
        now = {"level": level, "muted": level == 0}
        before, self.state["volume"] = self.state["volume"], now
        if before != now:
            await self.emit("volume.changed", **now)

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

    async def emit_hand_track(self, track: list[dict], *, realtime: bool = False) -> None:
        """Say a `humans/hands/tracked` track (`hand_track`) as lamp-control
        would (`hand_events`): `hand` events by change, at most four a second,
        and `seen: false` once the hand is gone. `realtime` paces them by the
        track's clock; without it they go as fast as they can. A lost hand
        doesn't end a following: the head waits for it, as the lamp's does."""
        started = asyncio.get_running_loop().time()
        first = track[0]["t"] if track else 0
        for at, event in hand_events(track):
            if realtime:
                await asyncio.sleep(max(0.0, started + (at - first) / 1e6 - asyncio.get_running_loop().time()))
            self.hand = event if event["seen"] else None
            await self.emit("hand", **event)

    def stop_following(self, ws: ServerConnection | None = None) -> None:
        """End the following of `ws` (or of every connection). lamp-control
        says nothing for it: the head just stops following."""
        for key, (held_ws, _lease) in list(self.following.items()):
            if ws is None or held_ws is ws:
                del self.following[key]

    def _unfollow(self, lease: str) -> None:
        for key, (_ws, held) in list(self.following.items()):
            if held == lease:
                del self.following[key]

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
        if state == "lost":
            self._unfollow(lease)

    async def lose_lease(self, lease: str, why: str = "hand") -> None:
        """The lamp takes a lease back, as a hand on the head would."""
        await self.lease_event(lease, "lost", why)

    async def done(self, re: int, how: str = "finished", why: str | None = None) -> None:
        """End the action whose request id was `re`, to the client that asked."""
        # A turn's outcome ends it, as the voice's does.
        self.turns = {client: started for client, started in self.turns.items() if started != re}
        await self._send_done(re, {"how": how} if why is None else {"how": how, "why": why})

    async def hand_on_head(self) -> None:
        """A hand on the head lifts every computer's mute, as the voice does:
        each is told with a `done` `cut_short` on the listen.set that held it."""
        mutes, self.mutes = self.mutes, {}
        for ws, re in mutes.values():
            await ws.send(json.dumps({"ev": "done", "re": re, "how": "cut_short", "why": "a hand on the head"}))

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
                # As lamp-control's protocol.rs, once a move has parsed (an id,
                # and frames of five whole counts) and before grants or leases:
                # one it won't take, or the body would drop, is refused in its
                # words (#4217). Anything else is the schema's to refuse.
                if (
                    request.get("op") == "move"
                    and id_ is not None
                    and _whole(request.get("step_ms"))
                    and _frames(request.get("frames"))
                ):
                    refusal = move_refusal(request["step_ms"], request["frames"])
                    if refusal is not None:
                        await self._refuse(ws, id_, "bad_request", refusal)
                        continue
                if self._validator is not None:
                    errors = list(self._validator.iter_errors(request))
                    if errors:
                        await self._refuse(ws, id_, "bad_request", errors[0].message)
                        continue
                if request.get("op") == "move" and not _whole(request.get("step_ms")):
                    await self._refuse(ws, id_, "bad_request", "step_ms: a whole number of ms")
                    continue
                if request.get("op") == "move" and not _frames(request.get("frames")):
                    await self._refuse(ws, id_, "bad_request", "frames: each frame is five whole counts")
                    continue
                op = request.get("op")
                if op == "pair":
                    await self._pair(ws, request, nonce)
                    continue
                if op == "hello":
                    client = await self._hello(ws, request, nonce)
                    if client is not None:
                        self._client_of[id(ws)] = client
                    continue
                if client is None:
                    await self._refuse(ws, id_, "no_hello", "hello first")
                    continue
                await self._op(ws, client, request)
        finally:
            self._connections.discard(ws)
            self.subscribed.pop(id(ws), None)
            self.following.pop(id(ws), None)
            # A computer gone lifts its mute: its last connection closed.
            gone = self._client_of.pop(id(ws), None)
            if gone is not None and gone not in self._client_of.values():
                self.mutes.pop(gone, None)
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
        await self._ok(ws, request["id"], lamp=self.lamp, grants=paired["grants"], limits=LIMITS)
        return request["client"]

    # The grant each event and state needs, as lamp-control's own (#3736).
    EVENT_GRANT = {
        "person.arrived": "look", "person.left": "look", "person.stood_up": "look", "person.facing": "look",
        "hand": "look",
        "phrase.heard": "hear", "name.heard": "hear", "sound.heard": "sound",
        "air.changed": "air",
        "volume.changed": "speak",
    }
    STATE_GRANT = {"presence": "look", "air": "air", "device": "device", "volume": "speak"}

    def _needs(self, op: str, request: dict) -> list[str]:
        if op == "subscribe":
            return sorted({self.EVENT_GRANT[e] for e in request.get("events", []) if e in self.EVENT_GRANT})
        if op == "get":
            return sorted({self.STATE_GRANT[s] for s in request.get("state", []) if s in self.STATE_GRANT})
        if op == "lease.take":
            # A lease's parts; a `sound.upload`'s or `skill.push`'s `parts` is a count.
            return [{"body": "body", "light": "light", "voice": "speak"}[p] for p in request.get("parts", [])]
        body = {"gesture", "move", "play", "joints", "look"}
        if op in body:
            return ["body", "look"] if op == "look" and request.get("at") in ("face", "hand") else ["body"]
        return {
            "express": ["body", "light"],
            "light": ["light"],
            "say": ["speak"],
            "sound": ["speak"],
            "camera.snapshot": ["camera"],
            "camera.stream": ["camera"],
            "sound.upload": ["speak"],
            "sound.list": ["speak"],
            "sound.remove": ["speak"],
            "skill.push": ["skills"],
            "skill.list": ["skills"],
            "skill.remove": ["skills"],
            # A turn speaks and hears what is said to the lamp.
            "turn.start": ["speak", "hear"],
            "turn.end": ["speak"],
            "listen.set": ["hear"],
            "volume": ["speak"],
        }.get(op, [])

    async def _upload(self, ws, client: str, request: dict) -> None:
        """One `sound.upload` part, as lamp-control's sounds.rs gathers it:
        in order from 0, the same clip, then checked and kept whole."""
        id_ = request["id"]
        key = (request["name"], request["sha256"].lower(), request["size"], request["parts"])
        if request["part"] == 0:
            self._uploads[client] = {"key": key, "next": 0, "bytes": b""}
        flight = self._uploads.get(client)
        if flight is None or flight["key"] != key or flight["next"] != request["part"]:
            self._uploads.pop(client, None)
            await self._refuse(ws, id_, "bad_request", "part: out of order; send the upload again from 0")
            return
        flight["bytes"] += base64.b64decode(request["data"])
        flight["next"] += 1
        if flight["next"] < request["parts"]:
            await self._ok(ws, id_, got=flight["next"])
            return
        data = self._uploads.pop(client)["bytes"]
        if len(data) != request["size"] or hashlib.sha256(data).hexdigest() != key[1]:
            await self._refuse(ws, id_, "bad_request", "sha256: not the clip's")
            return
        why = _clip_refusal(data)
        if why is not None:
            await self._refuse(ws, id_, "bad_request", f"clip: {why}")
            return
        kept = {n: b for n, b in self.clips[client].items() if n != request["name"]}
        if len(kept) >= LIMITS["sounds"]["clips_max"]:
            await self._refuse(ws, id_, "refused", "this computer keeps 64 clips already: remove one first")
            return
        if sum(map(len, kept.values())) + len(data) > LIMITS["sounds"]["store_max_bytes"]:
            await self._refuse(ws, id_, "refused", "this computer's clips would take over 20000000 bytes: remove one first")
            return
        self.clips[client][request["name"]] = data
        await self._ok(ws, id_, sound=_clip(request["name"], data))

    async def _push(self, ws, client: str, id_: int, request: dict) -> None:
        """A part of a push, as lamp-control and the helper take it: in order
        from 0, one push at a time, the whole checked against its SHA-256 and
        its manifest, then kept as this computer's."""
        import base64
        import hashlib
        import io
        import tarfile

        going = self._pushing
        if request["part"] == 0:
            if going and going["client"] != client:
                await self._refuse(ws, id_, "busy", "another computer's push is in flight")
                return
            going = self._pushing = {"client": client, "request": request, "data": b""}
        elif not going or going["client"] != client or request["part"] != going["part"] + 1:
            await self._refuse(ws, id_, "bad_request", "parts out of order")
            return
        going["part"] = request["part"]
        going["data"] += base64.b64decode(request["data"])
        if request["part"] < request["parts"] - 1:
            await self._ok(ws, id_, got=request["part"] + 1)
            return
        self._pushing = None
        data = going["data"]
        if len(data) != request["size"] or hashlib.sha256(data).hexdigest() != request["sha256"].lower():
            await self._refuse(ws, id_, "bad_request", "the package's SHA-256 doesn't match")
            return
        with tarfile.open(fileobj=io.BytesIO(data)) as tar:
            names = tar.getnames()
            manifest = json.loads(tar.extractfile("lamp-skill.json").read())
        if names[0] != "lamp-skill.json" or manifest["name"] != request["name"]:
            await self._refuse(ws, id_, "refused", "not a package of that skill")
            return
        kept = self.pushed.get(request["name"])
        if kept and kept["client"] != client:
            await self._refuse(ws, id_, "refused", "another computer's skill has that name")
            return
        skill = {
            "name": manifest["name"],
            "version": manifest["version"],
            "sha256": request["sha256"].lower(),
            "on": True,
            "held": None,
        }
        # Listed as the helper lists it (lamp-skills `list_pushed_by`): no
        # `client`, which only this fake keeps; `author` is a signed skill's,
        # never a pushed test folder's.
        at = datetime.datetime.now(datetime.timezone.utc).replace(microsecond=0)
        self.pushed[request["name"]] = {
            **skill,
            "pushed_at": at.strftime("%Y-%m-%dT%H:%M:%SZ"),
            "pushed_at_unix_ms": int(at.timestamp() * 1000),
            "author": None,
            "client": client,
        }
        await self._ok(ws, id_, skill={**skill, "asks": [], "shown": []})

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
            self._unfollow(request["lease"])
        elif op == "stop":
            # The holder's stop ends its following, as any of its actions.
            self.stop_following(ws)
            await self._ok(ws, id_)
        elif op in ("gesture", "move"):
            # As lamp-control routes them: the body takes them, and no
            # `done` follows.
            await self._ok(ws, id_, to="body")
        elif op == "light" and ("pixels" in request or "rgb" in request):
            # A computer's own colours (the ring, pixel by pixel), as
            # lamp-control's parts.rs: to the ring, each pixel at the brightness.
            given = request.get("pixels") or [request["rgb"]] * 32
            if len(given) != 32:
                self.requests.remove(request)
                await self._refuse(ws, id_, "bad_request", "pixels: 32 of them, one per pixel of the ring")
                return
            self.shown_pixels = given
            await self._ok(ws, id_, to="ring", pixels=len(given))
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
            # Another look ends a following. At `hand`, the body follows the
            # hand, waiting while it's gone; `follows_hand=False` is a
            # lamp-control from before #4183, which takes the look without
            # saying it follows.
            self.stop_following(ws)
            if request["at"] == "hand" and not self.follows_hand:
                await self._ok(ws, id_)
                return
            if request["at"] == "hand":
                lease = next(l for l, h in self.leases.items() if h["ws"] is ws and "body" in h["parts"])
                self.following[id(ws)] = (ws, lease)
                await self._ok(ws, id_, to="body", follow="hand")
                return
            await self._ok(ws, id_, to="body")
            if self.auto_done:
                at = request["at"] if isinstance(request["at"], dict) else {"yaw": 0.0, "pitch": 0.0}
                await self._send_done(id_, {"how": "finished", "yaw": at["yaw"], "pitch": at["pitch"], "clamped": False})
        elif op == "volume":
            # As lamp-control through lamp-actions' app calls: a level, the
            # voice's 20-point step within its floor of 10 and ceiling of 100,
            # or a mute that an unmute takes back. The answer is the level it
            # left, and `volume.changed` follows when it changed.
            level = self.state["volume"]["level"]
            if request.get("to") is not None:
                level_next = request["to"]
            elif request.get("step") == "up":
                level_next = min(100, level + 20)
            elif request.get("step") == "down":
                level_next = 0 if level == 0 else max(10, level - 20)
            elif request.get("mute") is True:
                if level > 0:
                    self._before_mute = level
                level_next = 0
            else:
                level_next = level if level > 0 else self._before_mute
            await self.set_volume(level_next)
            await self._ok(ws, id_, **self.state["volume"])
        elif op == "skill.push":
            await self._push(ws, client, id_, request)
        elif op == "skill.list":
            await self._ok(ws, id_, skills=[{k: v for k, v in s.items() if k != "client"} for s in self.pushed.values() if s["client"] == client])
        elif op == "skill.remove":
            kept = self.pushed.get(request["name"])
            if not kept or kept["client"] != client:
                await self._refuse(ws, id_, "refused", "not one of this computer's pushed skills")
                return
            del self.pushed[request["name"]]
            await self._ok(ws, id_)
        elif op == "sound.upload":
            await self._upload(ws, client, request)
        elif op == "sound.list":
            await self._ok(ws, id_, sounds=[_clip(n, b) for n, b in sorted(self.clips[client].items())])
        elif op == "sound.remove":
            if self.clips[client].pop(request["name"], None) is None:
                await self._refuse(ws, id_, "refused", f"no clip named {request['name']} on this lamp for this computer")
                return
            await self._ok(ws, id_, removed=request["name"])
        elif op in ("say", "sound"):
            # As lamp-control: the voice takes it (a sound is its hum or one
            # of the computer's clips), and its `done` follows.
            if op == "sound" and request["name"] != "hum" and request["name"] not in self.clips[client]:
                self.requests.remove(request)
                await self._refuse(
                    ws, id_, "refused", f"no clip named {request['name']} on this lamp for this computer: sound.upload it first"
                )
                return
            await self._ok(ws, id_, to="voice")
            if self.auto_done:
                await self.done(id_)
        elif op == "camera.snapshot":
            # A picture with no camera behind it: the smallest whole JPEG.
            await self._ok(ws, id_, jpeg=base64.b64encode(FAKE_JPEG).decode(), bytes=len(FAKE_JPEG), age_ms=0)
        elif op == "camera.stream":
            if request["on"]:
                fps = request.get("fps", 2)
                await self._ok(ws, id_, stream="on", fps=fps, fps_now=None)
                frame = {"ev": "camera.frame", "jpeg": base64.b64encode(FAKE_JPEG).decode(), "bytes": len(FAKE_JPEG), "age_ms": 0, "fps_now": None}
                await ws.send(json.dumps(frame))
            else:
                await self._ok(ws, id_, stream="off")
        elif op == "turn.start":
            # As lamp-control: handed to the voice, kept as this computer's
            # turn until its `done`; finished at once with auto_done.
            self.turns[client] = id_
            await self._ok(ws, id_, to="voice")
            if self.auto_done:
                await self.done(id_)
        elif op == "turn.end":
            started = self.turns.pop(client, None)
            if started is None:
                self.requests.remove(request)
                await self._refuse(ws, id_, "busy", "not this computer's turn")
                return
            await self._ok(ws, id_, turn="ended", started=started)
            await self.done(started, "dropped", "dropped: ended by the computer")
        elif op == "listen.set":
            # As lamp-control: false holds this computer's mute (again,
            # nothing new); true lifts it, or says nothing was held.
            if not request["on"]:
                self.mutes.setdefault(client, (ws, id_))
                await self._ok(ws, id_, mute="held")
            else:
                await self._ok(ws, id_, mute="lifted" if self.mutes.pop(client, None) else "nothing_held")
        else:
            await self._ok(ws, id_)
            if op == "play" and self.auto_done:
                await self.done(id_)
