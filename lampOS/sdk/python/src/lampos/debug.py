"""LAMPOS_LOG=debug: every frame to and from the lamp, one line each on stderr.

Off by default. What would let someone pair or say hello as this computer, or
read the lamp's pin, never reaches the log: `proof`, `sig`, `key`, `nonce`,
`pin` and `fp` are replaced, and so is a pair request's `code`, the owner's
one-time code (a reply's `code` is a refusal's, and is kept). A say's `text` is
given only as its length and a tag keyed for this run (an HMAC), and a pushed
skill part's `data` only as its length.
"""

from __future__ import annotations

import hashlib
import hmac
import json
import os
import sys
from typing import Any

# Fields replaced wherever they appear in a frame.
SECRET = frozenset({"proof", "sig", "key", "nonce", "pin", "fp"})
# Fields given only as a summary: a said text, a pushed skill part's bytes.
SUMMARISED = frozenset({"text", "data"})
# Every other field the protocol has, printed as it is. A test fails on any
# field of control.schema.json in none of these, so a new one is decided here,
# not found later in a log; and at run time a field in none of them prints as
# `<not shown>`, so one from a newer lamp is hidden until decided (fails closed). Shown as they are, so on a terminal or in the logs
# an MCP client keeps of stderr: a registered phrase and a `phrase.heard`
# phrase, events' fields, and a refusal's `why`.
SHOWN = frozenset({
    # A dev lamp's `--rates off`, said in hello's limits.
    "rates",
    "asleep", "at", "clamped", "class", "client", "code", "colour", "confidence", "count", "elbow", "error", "ev", "events",
    "express", "facing", "from", "feeling", "for_ms", "frames", "got", "grants", "hold_max_ms", "how",
    "id", "joints", "kind", "lamp", "lamp_state", "lease", "level", "light", "limits", "manage",
    "mood", "most", "motion", "name", "near", "ok", "on", "op", "open", "parts", "pattern", "paused", "phrase",
    "phrases", "pitch", "presence", "press", "private", "push", "quiet_hours", "re", "refused",
    "retry_ms", "roll", "say", "say_max_chars", "score", "seen", "shoulder", "shows", "side", "size", "skill", "skills",
    "state", "step_ms", "strength", "take_max_ms", "talking", "targets", "to", "until", "v", "where",
    "why", "within_ms", "x", "y", "yaw"
})
# Fields a lamp sends that the schema leaves open (`limits`, an `express`
# answer, `mood`, `light`, a pushed skill's parts, Nine's `shows` and `hand`,
# and a `look` at `hand`'s `follow`).
SHOWN_BEYOND_SCHEMA = frozenset({
    "to", "move", "on", "energy", "warmth", "awake", "shows", "seen", "x", "y", "size", "open",
    "confidence", "sha256", "part", "follow",
})
# A said text's tag is keyed with this process's own random key: the same text
# gives the same tag within a run, and nobody holding the log can test a guess.
_TAG_KEY = os.urandom(16)


def on() -> bool:
    """Read at each frame, so setting LAMPOS_LOG in a running program takes effect."""
    return os.environ.get("LAMPOS_LOG", "").strip().lower() == "debug"


def _decided(key: str) -> bool:
    return key in SECRET or key in SUMMARISED or key in SHOWN or key in SHOWN_BEYOND_SCHEMA


def redact(value: Any, key: str | None = None) -> Any:
    """A copy of `value` safe to print."""
    if key in SECRET and value is not None:
        return "<redacted>"
    if key == "text" and isinstance(value, str):
        tag = hmac.new(_TAG_KEY, value.encode(), hashlib.sha256).hexdigest()[:8]
        return f"<{len(value)} chars, tag {tag}>"
    if key == "data" and isinstance(value, str):
        return f"<{len(value)} chars>"
    if isinstance(value, dict):
        pairing = value.get("op") == "pair"
        return {
            k: "<redacted>" if pairing and k == "code" else redact(v, k) if _decided(k) else "<not shown>"
            for k, v in value.items()
        }
    if isinstance(value, list):
        return [redact(v) for v in value]
    return value


def frame(direction: str, value: Any) -> None:
    """Print one frame, `->` sent or `<-` received, when LAMPOS_LOG=debug."""
    if on():
        print(f"lampos {direction} {json.dumps(redact(value), separators=(',', ':'), ensure_ascii=False)}", file=sys.stderr, flush=True)
