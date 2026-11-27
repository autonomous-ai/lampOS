"""The report `log.send` takes: the fixed schema agreed with Ten and Backend (#8726, 26 Nov 16:06), built from numbers.

Nothing in it comes from what a person said: the numbers are the score's, and the only words are the ones on a
closed list (a failure category, a case's kind, outcome and fault, the route, the band). A flat body. The app sends no
`build` and no `day`: lamp-control sets both before it reads the report (an app's own are overwritten).
"""

from __future__ import annotations

from typing import Any

from . import v6
from .score import MOST_COUNT, MOST_SOUND_MS

DIAGNOSIS_VERSION = 1
CATEGORIES = tuple(v6.FAIL)  # the 12 kinds of v6, once each
ROUTES = ("first", "second", "unknown")
FIELDS = (
    "turns", "answered_within_5s_pct", "heard_to_first_sound_ms_median", "heard_to_first_sound_ms_p90", "asked_again",
    "held", "cut_ins", "tool_timeouts", "stuck", "tier", "score", "band", "failures", "cases", "cases_dropped", "route",
    "diagnosis_version",
)


def build_report(day: dict[str, Any], diagnosis: list[str]) -> dict[str, Any] | None:
    """The day's report, or None when v6 gives no Voice score yet (under 3 questions): nothing is sent then."""
    score = day.get("score")
    if score is None:
        return None
    failures = dict(day.get("v6_failures", {}))
    for name in diagnosis:
        # A category the model names and the records did not count goes in as 1: flagged, not counted.
        if name in CATEGORIES:
            failures.setdefault(name, 1)
    cases = day.get("cases", {"list": [], "dropped": 0})
    def count(n: int) -> int:
        return min(int(n), MOST_COUNT)

    median, p90 = min(day["heard_to_first_sound_ms_median"], MOST_SOUND_MS), min(day["heard_to_first_sound_ms_p90"], MOST_SOUND_MS)
    return {
        "turns": count(day["turns"]),
        "answered_within_5s_pct": round(day["answered_within_5s"]),
        "heard_to_first_sound_ms_median": median,
        "heard_to_first_sound_ms_p90": p90,
        "asked_again": count(day["asked_again"]),
        "held": count(day["held"]),
        "cut_ins": count(day["cut_ins"]),
        "tool_timeouts": count(day["tool_timeouts"]),
        "stuck": count(day["stuck"]),
        "tier": int(str(day["tier"]).lstrip("T")),
        "score": round(score, 1),
        "band": v6.band(round(score, 1)),
        "failures": [{"category": name, "count": count(failures[name])} for name in CATEGORIES if failures.get(name, 0) > 0],
        "cases": cases["list"],
        "cases_dropped": min(cases["dropped"], day["turns"]),  # at most the turns (Ten's bound)
        "route": day.get("route") if day.get("route") in ROUTES else "unknown",
        "diagnosis_version": DIAGNOSIS_VERSION,
    }
