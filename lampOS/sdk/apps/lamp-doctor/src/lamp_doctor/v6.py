"""Score v6 (#7452), the parts of it that numbers allow, on its own 0 to 10 scale.

Ported from `score_v6.py` of the #7452 report (the owner's scale, 26 Nov): the failure weights, the soft ones, the
count and depth caps, `speed_pts`, the Voice part (`lamp_quality`), the parts' weights, `MIN_Q` and the bands.
Nothing here reads a word. A question is a dict of numbers and a few fixed words:

    ms        heard to first sound, or None
    answered  True if the lamp gave an answer (so its speed counts)
    problem   one of "stuck", "no_answer", "misheard", "error", or None

What v6 needs that numbers do not give is in `INPUTS` below, each with where Lamp Doctor gets it.
"""

from __future__ import annotations

import collections
import math
from typing import Any

MIN_Q = 20
DEPTH_MIN = 3
VERY_SLOW_MS = 8000
RATE_K = 9
GOOD_MS, FAST_MS, OK_MS = 2000, 3000, 5000
# kind -> weight (v6 FAIL). Order: how bad it feels to the user.
FAIL = collections.OrderedDict([
    ("stuck", 1.5), ("lost_answer", 1.5), ("task_never_run", 1.5), ("error", 1.0), ("skill_make_failed", 1.0),
    ("skill_refused", 1.0), ("memory_not_saved", 1.0), ("no_answer", 1.0), ("tts_refusal", 1.0), ("issue", 1.0),
    ("misheard", 0.5), ("very_slow", 0.5),
])
SOFT = {"misheard", "very_slow"}  # half weight, and not counted toward the count caps
COUNT_CAPS = ((10, 5.4), (3, 7.4), (1, 8.5))
DEPTH_CAP = {"T1": 5.9, "T2": 5.9, "T3": 8.5, "T4": 10.0}
PART_W = {"Voice": 0.40, "Skills": 0.25, "Memory": 0.20, "SDK": 0.15}
VOICE_QUESTION_FAILS = ("no_answer", "stuck", "misheard", "very_slow")  # what v6 charges to Voice
NOT_SPEED = ("no_answer", "stuck", "error")  # a question with one of these has no answer to time

# Each v6 input and where Lamp Doctor gets it: ("now", ...) numbers of the turn record or the journal;
# ("later", ...) needs the words, only through `model.ask` on the lamp; ("no", ...) not available to the app yet.
INPUTS = (
    ("question's first-answer time `ms`", "now", "turns: heard_to_first_sound_ms"),
    ("failure: stuck", "now", "turns: outcome held or stuck"),
    ("failure: had to repeat (misheard)", "now", "turns: outcome asked_again, or the flags heard_wrongly, name_missed"),
    ("failure: very slow (over 8 s)", "now", "turns: heard_to_first_sound_ms over 8000"),
    ("failure: no answer", "now", "turns: answered but no first sound; today the viewer's rows only have answered requests"),
    ("failure: error", "no", "a lamp error is not in the turn record; a tool timeout is (calls[].timed_out), to confirm it counts"),
    ("lamp signals: skill build failed or refused, task lost or never run, memory not saved, answer lost, speech failed, issue filed", "no",
     "journal lines; not in `turns`. Needs a numbers-only field from Ten, or the `logs` switch"),
    ("question's capabilities (caps) for Skills, Memory, SDK, tier, area", "later",
     "tool names are in turns (calls[].name); the tool-name to capability table of the #7452 generator is not in the four files"),
    ("question's tier by the words (text_tier) and area (text_area)", "later", "the words, via model.ask on the lamp; or the caps above"),
    ("conversation level 2+ (follow-ups), generic description", "later", "the words, via model.ask on the lamp"),
    ("proof of success for Skills, Memory, SDK", "no", "the lamp's own markers (tools: calls=, skill-run, SDK door); not in `turns`"),
    ("window day or night, MIN_Q 20", "now", "turns: at (unix minute); counts"),
    ("sdk.installed, sdk.paired", "no", "off on the public page (proof method), and not an app's to read"),
)


def speed_pts(ms: float) -> float:
    """One answered question's speed, 0 to 1: up to 2 s full, 3 s 0.7, 5 s 0.4, 8 s 0.2, slower 0."""
    return 1.0 if ms <= GOOD_MS else 0.7 if ms <= FAST_MS else 0.4 if ms <= OK_MS else 0.2 if ms <= VERY_SLOW_MS else 0.0


def band(score: float | None) -> str:
    return "grey" if score is None else "green" if score >= 8 else "amber" if score >= 6 else "red"


def failure(q: dict[str, Any]) -> str | None:
    if q.get("problem") in ("stuck", "no_answer", "misheard", "error"):
        return q["problem"]
    ms = q.get("ms")
    if q.get("answered") and isinstance(ms, (int, float)) and ms > VERY_SLOW_MS:
        return "very_slow"
    return None


def weight(qs: list[dict[str, Any]], signals: dict[str, float] | None = None) -> float:
    w = sum(FAIL[f] for f in map(failure, qs) if f)
    return w + sum(FAIL[k] * v for k, v in (signals or {}).items() if k in FAIL and v > 0)


def reliability(qs: list[dict[str, Any]], signals: dict[str, float] | None = None) -> float:
    """10 x e^(-9 x weighted failures / questions): 2 % failing 8.4, 5 % 6.4, 10 % 4.1, 20 % 1.7."""
    return 10 * math.exp(-RATE_K * weight(qs, signals) / len(qs)) if qs else 0.0


def count_cap(qs: list[dict[str, Any]]) -> float | None:
    count = round(sum(1 for f in map(failure, qs) if f and f not in SOFT))
    return next((cap for at, cap in COUNT_CAPS if count >= at), None)


def speed(qs: list[dict[str, Any]]) -> tuple[float, int]:
    """10 x the mean speed of the answered questions that have a time, and how many there are."""
    ms = [q["ms"] for q in qs if q.get("answered") and isinstance(q.get("ms"), (int, float)) and failure(q) not in NOT_SPEED]
    return (10 * sum(speed_pts(m) for m in ms) / len(ms) if ms else 0.0), len(ms)


def voice(qs: list[dict[str, Any]], signals: dict[str, float] | None = None) -> float | None:
    """The Voice part (40 % of the lamp score): 0.6 reliability + 0.4 speed over every question; under 3 questions none.
    With no answer times at all it is reliability alone, as v6 does."""
    if len(qs) < DEPTH_MIN:
        return None
    rel = reliability(qs, signals)
    sp, n = speed(qs)
    return round(0.6 * rel + 0.4 * sp, 1) if n else round(rel, 1)


def depth_capped(score: float, deepest: str | None) -> float:
    return min(score, DEPTH_CAP.get(deepest, 5.9))
