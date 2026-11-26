"""The day's score from turn records: numbers only, no text, no model.

A record is what the `turns` grant gives (docs/apps.md, #8726): times,
outcome, counts, the route and the build, never words. Only the keys read
below are read; anything else in a record is ignored.
"""

from __future__ import annotations

import datetime as dt
import math
from collections import Counter
from typing import Any

from . import v6

WITHIN_S = 5000  # the "answered within 5 s" metric; v6 calls only over 8 s a failure (v6.VERY_SLOW_MS)
FAILURES = ("no_answer", "error", "heard_wrongly", "slow_first_sound", "call_held", "answer_cut_off", "tool_timeout", "silent_turn")

# A placeholder for #7452's depth scale until Backend's table is in: the
# deepest tool a turn used sets the day's tier. A tool not listed is T1.
DEPTH = {
    "set_light": "T1", "get_time": "T1", "set_volume": "T1", "gesture": "T1",
    "reminders": "T2", "weather": "T2", "web_search": "T2", "tell_story": "T2", "remember": "T2",
    "play_music": "T3", "mail": "T3", "calendar": "T3", "timer": "T3",
    "start_agent_task": "T4", "confirm_agent_task": "T4", "make_skill": "T4", "use_skill": "T4",
}


def _percentile(values: list[int], p: float) -> int | None:
    if not values:
        return None
    ordered = sorted(values)
    return ordered[max(0, math.ceil(p * len(ordered)) - 1)]


def _median(values: list[int]) -> int | None:
    if not values:
        return None
    ordered = sorted(values)
    mid = len(ordered) // 2
    return ordered[mid] if len(ordered) % 2 else round((ordered[mid - 1] + ordered[mid]) / 2)


def _commonest(records: list[dict[str, Any]], key: str) -> str:
    counts = Counter(str(r.get(key)) for r in records if r.get(key) is not None)
    return counts.most_common(1)[0][0] if counts else "unknown"


def score_day(records: list[dict[str, Any]]) -> dict[str, Any]:
    n = len(records)
    if n == 0:
        return {"turns": 0, "score": None, "tier": None, "failures": {}}
    sounds = [r["heard_to_first_sound_ms"] for r in records if r.get("heard_to_first_sound_ms") is not None]
    quick = sum(1 for s in sounds if s <= WITHIN_S)
    calls = [c for r in records for c in r.get("calls", [])]
    timed_out_turns = sum(1 for r in records if any(c.get("timed_out") for c in r.get("calls", [])))
    outcome = Counter(r.get("outcome") for r in records)
    failures = Counter({
        "no_answer": sum(1 for r in records if r.get("outcome") == "unanswered" and r.get("problem") == "no_answer"),
        "error": sum(1 for r in records if r.get("outcome") == "unanswered" and r.get("problem") == "error"),
        "heard_wrongly": sum(1 for r in records if r.get("heard_wrongly") or r.get("name_missed")),
        "slow_first_sound": sum(1 for s in sounds if s > v6.VERY_SLOW_MS),
        "call_held": outcome["held"],
        "answer_cut_off": sum(1 for r in records if r.get("cut_ins", 0) > 0),
        "tool_timeout": timed_out_turns,
        "silent_turn": sum(1 for r in records if r.get("outcome") == "stuck" and not r.get("words_heard")),
    })
    within = quick / n
    questions = [_question(r) for r in records]
    value = v6.voice(questions)
    depth = max((DEPTH.get(c.get("name"), "T1") for c in calls), default="T1")
    return {
        "turns": n,
        "answered_within_5s": round(100 * within, 1),
        "heard_to_first_sound_ms_median": _median(sounds) or 0,
        "heard_to_first_sound_ms_p90": _percentile(sounds, 0.9) or 0,
        "asked_again": outcome["asked_again"],
        "held": outcome["held"],
        "cut_ins": sum(r.get("cut_ins", 0) for r in records),
        "tool_timeouts": sum(1 for c in calls if c.get("timed_out")),
        "stuck": outcome["stuck"],
        "tier": depth,
        "score": value,  # v6's Voice part, 0 to 10 (bands 8 and 6); the other parts are not scored yet
        "band": v6.band(value),
        "few": n < v6.MIN_Q,
        "failures": {name: count for name, count in failures.items() if count},
        "v6_failures": _v6_failures(records, questions),
        "cases": _cases(records),
        "route": _commonest(records, "route"),
        "build": _commonest(records, "build"),
    }


CASES_MAX = 50
# The report's bounds (Ten's `lamp_msgs::doctor`, 26 Nov): a number past one is not sent as itself.
MOST_COUNT, MOST_REQUEST, MOST_WAIT_MS, MOST_SOUND_MS = 100_000, 100_000, 120_000, 600_000
KINDS = ("stt", "llm", "tts", "tool", "app", "memory", "task")
CASE_OUTCOMES = ("error", "timeout", "refused")
FAULTS = ("local", "transport", "connect_timeout", "response_timeout", "authentication", "quota_exceeded",
          "rate_limited", "server_unavailable", "server_rejected", "protocol")


def _v6_failures(records: list[dict[str, Any]], questions: list[dict[str, Any]]) -> dict[str, int]:
    """The day's failures by v6's kinds (the report's categories): the questions' own, and a lost answer."""
    counts = Counter(k for k in map(v6.failure, questions) if k)
    lost = sum(1 for r in records if r.get("outcome") == "lost_answer")
    if lost:
        counts["lost_answer"] += lost
    return dict(counts)


def _cases(records: list[dict[str, Any]]) -> dict[str, Any]:
    """Each failed call of each turn (Ten's `failed[]`): kind, outcome, http, fault, waited_ms, request, when.
    Only listed words and numbers are kept; at most 50, the rest only counted."""
    found = []
    dropped_out = 0
    for r in records:
        minutes = r.get("at_min", r.get("at"))
        for call in r.get("failed", []):
            kind, outcome = call.get("kind"), call.get("outcome")
            if kind not in KINDS or outcome not in CASE_OUTCOMES:
                continue
            case: dict[str, Any] = {
                "at": dt.datetime.fromtimestamp(minutes * 60).strftime("%H:%M") if isinstance(minutes, (int, float)) else "00:00",
                "kind": kind, "outcome": outcome}
            if isinstance(call.get("http"), int) and 100 <= call["http"] <= 599:
                case["http"] = call["http"]
            if call.get("fault") in FAULTS:
                case["fault"] = call["fault"]
            waited = call.get("waited_ms")
            case["waited_ms"] = min(int(waited), MOST_WAIT_MS) if isinstance(waited, (int, float)) and waited >= 0 else 0
            request = call.get("request")
            case["request"] = int(request) if isinstance(request, (int, float)) and 0 <= request <= MOST_REQUEST else -1
            if case["request"] < 0:  # a request number past what the voice counts to: not sent, only counted as dropped
                dropped_out += 1
                continue
            found.append(case)
    return {"list": found[:CASES_MAX], "dropped": max(0, len(found) - CASES_MAX) + dropped_out}


def _question(record: dict[str, Any]) -> dict[str, Any]:
    """A turn record as v6 reads a question: its first-answer time, whether it was answered, its problem."""
    outcome = record.get("outcome")
    problem = (
        record.get("problem") if outcome == "unanswered" and record.get("problem") in ("no_answer", "error")
        else "stuck" if outcome in ("held", "stuck")
        else "misheard" if outcome == "asked_again" or record.get("heard_wrongly") or record.get("name_missed")
        else "error" if outcome == "error"
        else "no_answer" if outcome == "no_answer"
        else None
    )
    sound = record.get("heard_to_first_sound_ms")
    return {"ms": sound, "answered": outcome in ("answered", "asked_again"), "problem": problem}
