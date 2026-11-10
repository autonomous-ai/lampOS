"""The day-0 guide (`lampOS/sdk/GUIDE.md`) keeps up with the protocol.

Every op, event, state, grant and error code the schema names must appear in
the guide, written as code (`name`), so a pull request that adds one to
`docs/control.schema.json` fails here until the guide says what it is
(lampOS/AGENTS.md: a change to an SDK op, event, grant, refusal, limit or
status updates the guide in the same pull request). And every row of the
guide's status table that says a thing works carries the date it was last
verified, so "works today" never goes stale unseen.
"""

from __future__ import annotations

import re
from pathlib import Path

import pytest

from lampos import schema

GUIDE = Path(__file__).resolve().parents[2] / "GUIDE.md"

# The statuses the guide's "Plan and status" table uses: "lamp run pending" is
# in the customer build's code, not yet shown on a lamp.
STATUSES = ("works today", "lamp run pending", "next build", "in development", "planned")
# A date as the guide writes "Last verified": 2026-11-09, or 9 Nov 2026.
DATE = re.compile(r"\b(20\d\d-\d\d-\d\d|\d{1,2} (Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec) 20\d\d)\b")


def guide() -> str:
    assert GUIDE.is_file(), f"{GUIDE} is missing: the SDK's day-0 guide"
    return GUIDE.read_text(encoding="utf-8")


@pytest.mark.parametrize(
    "kind, names",
    [
        ("op", schema.ops()),
        ("event", schema.events()),
        ("state", schema.states()),
        ("grant", schema.grants()),
        ("error code", schema.error_codes()),
    ],
)
def test_the_guide_names_everything_the_schema_names(kind, names):
    text = guide()
    missing = sorted(name for name in names if f"`{name}`" not in text)
    assert not missing, (
        f"sdk/GUIDE.md doesn't name these {kind}s from docs/control.schema.json: {', '.join(missing)}. "
        "Say what each does, its grant, limits, refusals and status (lampOS/AGENTS.md)."
    )


def test_every_status_row_that_says_it_works_says_when_it_was_verified():
    rows = [line for line in guide().splitlines() if line.startswith("|") and "works today" in line.lower()]
    assert rows, "sdk/GUIDE.md has no status row that says `works today`"
    undated = [row for row in rows if not DATE.search(row)]
    assert not undated, "status rows without a last-verified date:\n" + "\n".join(undated)


def test_every_status_row_uses_one_of_the_guides_statuses():
    text = guide()
    start = text.find("## Plan and status")
    assert start >= 0, "sdk/GUIDE.md has no '## Plan and status' section"
    section = text[start:].split("\n## ", 1)[0]
    lines = section.splitlines()
    # A table's header is the row just before its |---| line; the section may hold more than one table.
    separator = lambda line: line.startswith("|") and set(line) <= set("|-: ")
    body = [
        line
        for at, line in enumerate(lines)
        if line.startswith("|") and not separator(line) and not (at + 1 < len(lines) and separator(lines[at + 1]))
    ]
    assert body, "the status table is empty"
    unknown = [row for row in body if not any(status in row.lower() for status in STATUSES)]
    assert not unknown, f"status rows without one of {STATUSES}:\n" + "\n".join(unknown)
