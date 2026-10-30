"""The lamp's kinematic model (docs/lamp.model.json, written from the rigged
CAD by the JS SDK's tools/lamp-model.js): its joints under the protocol's
names, the chain from the base out, each joint's origin, axis and reach, and
what is measured and what is still the CAD's.

The package carries its own copy; a test keeps it equal to the repository's.
"""

from __future__ import annotations

import copy
import json
from functools import lru_cache
from importlib import resources
from typing import Any


@lru_cache(maxsize=1)
def _model() -> dict[str, Any]:
    return json.loads(resources.files("lampos").joinpath("lamp.model.json").read_text("utf-8"))


def model() -> dict[str, Any]:
    """The model, a fresh copy each call, so a caller's change stays its own."""
    return copy.deepcopy(_model())


@lru_cache(maxsize=1)
def _hardware() -> dict[str, Any]:
    return json.loads(resources.files("lampos").joinpath("lamp.hardware.json").read_text("utf-8"))


def hardware() -> dict[str, Any]:
    """The lamp's parts as data (docs/lamp.hardware.json), each with where its
    facts come from; a fresh copy each call."""
    return copy.deepcopy(_hardware())
