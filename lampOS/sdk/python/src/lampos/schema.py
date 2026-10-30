"""The v1 protocol's names, read from its schema (docs/control.schema.json).

The package carries its own copy of the schema; a test keeps it equal to the
repository's. Everything the client checks before sending (ops, events,
gestures, colours, error codes) comes from here, so a change to the schema
changes the SDK without a second list to keep in step.
"""

from __future__ import annotations

import json
from functools import lru_cache
from importlib import resources
from typing import Any

VERSION = 1
PORT = 7731
SERVICE = "_lampos._tcp.local."


@lru_cache(maxsize=1)
def schema() -> dict[str, Any]:
    text = resources.files("lampos").joinpath("control.schema.json").read_text("utf-8")
    return json.loads(text)


def _enum(name: str) -> tuple[str, ...]:
    return tuple(schema()["$defs"][name]["enum"])


def ops() -> tuple[str, ...]:
    """Every request op, as the schema lists them."""
    defs = schema()["$defs"]
    return tuple(
        defs[ref["$ref"].rsplit("/", 1)[1]]["properties"]["op"]["const"]
        for ref in defs["request"]["oneOf"]
    )


def events() -> tuple[str, ...]:
    return _enum("event_name")


def states() -> tuple[str, ...]:
    return _enum("state_name")


def gestures() -> tuple[str, ...]:
    """The body clips offered to programs (the schema's `examples`). The door
    takes any gesture name of the right shape (`gesture_pattern`), `idle` and
    `scanning` too, and the body answers a name it doesn't have."""
    return tuple(schema()["$defs"]["gesture"]["examples"])


def gesture_pattern() -> str:
    """A gesture name's shape, as lamp-control's plain_name checks it."""
    return schema()["$defs"]["gesture"]["pattern"]


def colours() -> tuple[str, ...]:
    return _enum("colour")


def patterns() -> tuple[str, ...]:
    return _enum("pattern")


def grants() -> tuple[str, ...]:
    return _enum("grant")


def parts() -> tuple[str, ...]:
    return _enum("part")


def error_codes() -> tuple[str, ...]:
    return tuple(schema()["$defs"]["error"]["properties"]["code"]["enum"])


def request_def(op: str) -> dict[str, Any]:
    """The schema's definition of one op's request."""
    defs = schema()["$defs"]
    for ref in defs["request"]["oneOf"]:
        definition = defs[ref["$ref"].rsplit("/", 1)[1]]
        if definition["properties"]["op"]["const"] == op:
            return definition
    raise KeyError(op)
