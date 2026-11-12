"""The control schema has no repeated key in any object: JSON parsers keep the last one silently, so a repeat
(two changes that each added the same property) is harmless today and wrong (#6408 and #6448 both added hello's
`token`). The three copies are checked, the docs one when this is run from a checkout."""

from __future__ import annotations

import json
from pathlib import Path

import pytest

import lampos

COPIES = {
    "sdk/python": Path(lampos.__file__).parent / "control.schema.json",
    "docs": Path(__file__).resolve().parents[3] / "docs" / "control.schema.json",
    "sdk/js": Path(__file__).resolve().parents[2] / "js" / "src" / "control.schema.json",
}


def _no_repeats(pairs):
    seen: dict = {}
    for key, value in pairs:
        if key in seen:
            raise ValueError(f"key {key!r} appears twice in one object")
        seen[key] = value
    return seen


@pytest.mark.parametrize("where", sorted(COPIES))
def test_a_schema_copy_has_no_repeated_key(where):
    path = COPIES[where]
    if not path.exists():
        pytest.skip(f"{where} copy is not here (the SDK is installed without the checkout)")
    json.loads(path.read_text(), object_pairs_hook=_no_repeats)


def test_the_checker_catches_a_repeat():
    with pytest.raises(ValueError, match="appears twice"):
        json.loads('{"a": {"token": 1, "sig": 2, "token": 1}}', object_pairs_hook=_no_repeats)
