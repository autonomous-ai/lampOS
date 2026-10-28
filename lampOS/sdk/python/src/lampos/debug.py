"""LAMPOS_LOG=debug: every frame to and from the lamp, one line each on stderr.

Off by default. What would let someone pair or say hello as this computer, or
read the lamp's pin, never reaches the log: `proof`, `sig`, `key`, `nonce`,
`pin` and `fp` are replaced, and so is a pair request's `code`, the owner's
one-time code (a reply's `code` is a refusal's, and is kept); a say's `text` is given only as its length and the
start of its SHA-256, and a pushed skill part's `data` only as its length.
"""

from __future__ import annotations

import hashlib
import json
import os
import sys
from typing import Any

# Fields replaced wherever they appear in a frame.
SECRET = frozenset({"proof", "sig", "key", "nonce", "pin", "fp"})


def on() -> bool:
    """Read at each frame, so setting LAMPOS_LOG in a running program takes effect."""
    return os.environ.get("LAMPOS_LOG", "").strip().lower() == "debug"


def redact(value: Any, key: str | None = None) -> Any:
    """A copy of `value` safe to print."""
    if key in SECRET and value is not None:
        return "<redacted>"
    if key == "text" and isinstance(value, str):
        digest = hashlib.sha256(value.encode()).hexdigest()[:8]
        return f"<{len(value)} chars, sha256 {digest}>"
    if key == "data" and isinstance(value, str):
        return f"<{len(value)} chars>"
    if isinstance(value, dict):
        pairing = value.get("op") == "pair"
        return {k: "<redacted>" if pairing and k == "code" else redact(v, k) for k, v in value.items()}
    if isinstance(value, list):
        return [redact(v) for v in value]
    return value


def frame(direction: str, value: Any) -> None:
    """Print one frame, `->` sent or `<-` received, when LAMPOS_LOG=debug."""
    if on():
        print(f"lampos {direction} {json.dumps(redact(value), separators=(',', ':'), ensure_ascii=False)}", file=sys.stderr, flush=True)
