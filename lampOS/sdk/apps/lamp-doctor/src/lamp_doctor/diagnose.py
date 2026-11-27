"""The diagnosis: ask the lamp's model which fixed failure categories it sees.

The answer is free text, so it is never passed on. `parse_categories` keeps
the listed category names and drops everything else; the report holds only
those. The lamp checks the way out again (`log.send` refuses a string outside
the list), so a heard sentence or a name has two doors shut.
"""

from __future__ import annotations

import json
import re
from typing import Any

from . import v6

FAILURES = tuple(v6.FAIL)  # the 12 kinds of v6, the only labels a report may carry

MAX_INPUT_CHARS = 2000
_WORD = re.compile(r"\b(" + "|".join(FAILURES) + r")\b")


def parse_categories(answer: object) -> list[str]:
    if not isinstance(answer, str) or not answer.strip():
        return []
    try:
        data = json.loads(answer)
    except ValueError:
        data = None
    names = data if isinstance(data, list) else _WORD.findall(answer)
    kept: list[str] = []
    for name in names:
        if isinstance(name, str) and name in FAILURES and name not in kept:
            kept.append(name)
    return kept


def _prompt() -> str:
    return (
        "You look at one day of a voice lamp's conversation metrics (and, if given, what was said). "
        "Say which of these failure categories you see, as a JSON list of names and nothing else: "
        + ", ".join(FAILURES)
        + ". Do not quote anything that was said."
    )


async def diagnose(ops: Any, day: dict[str, Any], heard: list[dict[str, Any]] | None = None) -> list[str]:
    numbers = {k: v for k, v in day.items() if isinstance(v, (int, float, str, dict)) and k not in ("failures", "v6_failures", "cases")}
    text = json.dumps({"metrics": numbers, "heard": [h.get("heard") for h in heard or []]})[:MAX_INPUT_CHARS]
    try:
        answer = await ops.model_ask(_prompt(), input=text)
    except Exception:  # the cap, the provider, a refusal: the report goes without a diagnosis
        return []
    return parse_categories(answer.get("text") if isinstance(answer, dict) else None)
