"""A fake of Ten's ops for Lamp Doctor, until lamp-control builds them.

Everything the app assumes about `turns`, `model.ask` and `log.send` is here
and nowhere else, so a change of shape on #8726 is a change in this file.
"""

from __future__ import annotations

# The agreed report (Ten and Backend, #8726, 26 Nov 16:06): flat, closed lists, no text. lamp-control adds `build` and `day`.
FAILURES = ("stuck", "lost_answer", "task_never_run", "error", "skill_make_failed", "skill_refused", "memory_not_saved",
            "no_answer", "tts_refusal", "issue", "misheard", "very_slow")
REPORT_FIELDS = {
    "turns", "answered_within_5s_pct", "heard_to_first_sound_ms_median", "heard_to_first_sound_ms_p90", "asked_again",
    "held", "cut_ins", "tool_timeouts", "stuck", "tier", "score", "band", "failures", "cases", "cases_dropped", "route",
    "diagnosis_version",
}
KINDS = ("stt", "llm", "tts", "tool", "app", "memory", "task")
OUTCOMES = ("error", "timeout", "refused")
FAULTS = ("local", "transport", "connect_timeout", "response_timeout", "authentication", "quota_exceeded",
          "rate_limited", "server_unavailable", "server_rejected", "protocol")
CASE_KEYS = {"at", "kind", "outcome", "http", "fault", "waited_ms", "request"}


class BadReport(Exception):
    code = "bad_report"


class RateLimited(Exception):
    code = "rate_limited"


def turn(**over):
    """A turn record as the voice would hand it over: no text of any kind."""
    record = {
        "turn": 1, "at": 29_000_000, "route": "first", "build": 16220, "outcome": "answered",
        "heard_to_first_sound_ms": 2000, "cut_ins": 0, "calls": [], "failed": [], "words_heard": 5, "words_said": 12,
        "heard_wrongly": False, "name_missed": False, "held_then_repeated": False,
    }
    record.update(over)
    return record


def _int(value, low, high):
    return isinstance(value, int) and not isinstance(value, bool) and low <= value <= high


def strict_check(report: dict) -> None:
    """lamp-control's `log.send` check: the fixed schema or nothing."""
    extra = set(report) - REPORT_FIELDS
    missing = REPORT_FIELDS - set(report)
    if extra or missing:
        raise BadReport(f"fields: extra {sorted(extra)}, missing {sorted(missing)}")
    for name in ("turns", "heard_to_first_sound_ms_median", "heard_to_first_sound_ms_p90", "asked_again", "held", "cut_ins",
                 "tool_timeouts", "stuck", "cases_dropped", "diagnosis_version"):
        if not _int(report[name], 0, 600_000 if 'ms' in name else 100 if name == 'diagnosis_version' else 100_000):
            raise BadReport(f"{name}: a whole number")
    if not _int(report["answered_within_5s_pct"], 0, 100) or not _int(report["tier"], 1, 4):
        raise BadReport("percent 0 to 100, tier 1 to 4")
    if report["cases_dropped"] > report["turns"]:
        raise BadReport("cases_dropped is at most the turns")
    if report["heard_to_first_sound_ms_p90"] < report["heard_to_first_sound_ms_median"]:
        raise BadReport("p90 below its median")
    score = report["score"]
    if not isinstance(score, (int, float)) or isinstance(score, bool) or not 0 <= score <= 10 or round(score, 1) != score:
        raise BadReport("score: 0.0 to 10.0, one decimal")
    if report["band"] != ("green" if score >= 8 else "amber" if score >= 6 else "red"):
        raise BadReport("band does not match the score")
    if report["route"] not in ("first", "second", "unknown"):
        raise BadReport("route: first, second or unknown")
    names = []
    if not isinstance(report["failures"], list) or len(report["failures"]) > 12:
        raise BadReport("failures: a list of up to 12")
    for item in report["failures"]:
        if not isinstance(item, dict) or set(item) != {"category", "count"}:
            raise BadReport("failures: each is {category, count}")
        if item["category"] not in FAILURES or item["category"] in names:
            raise BadReport(f"failures: {item['category']!r} is not a listed category, or twice")
        names.append(item["category"])
        if not _int(item["count"], 1, 100_000):
            raise BadReport(f"failures.{item['category']}: not a count")
    if not isinstance(report["cases"], list) or len(report["cases"]) > 50:
        raise BadReport("cases: a list of up to 50")
    for case in report["cases"]:
        if not isinstance(case, dict) or not {"at", "kind", "outcome", "waited_ms", "request"} <= set(case) <= CASE_KEYS:
            raise BadReport("cases: each is {at, kind, outcome, http?, fault?, waited_ms, request}")
        at = case["at"]
        if not (isinstance(at, str) and len(at) == 5 and at[2] == ":" and at[:2].isdigit() and at[3:].isdigit()
                and int(at[:2]) < 24 and int(at[3:]) < 60):
            raise BadReport("cases.at: HH:MM")
        if case["kind"] not in KINDS or case["outcome"] not in OUTCOMES:
            raise BadReport("cases: kind or outcome not on its list")
        if "http" in case and not _int(case["http"], 100, 599):
            raise BadReport("cases.http: a status")
        if "fault" in case and case["fault"] not in FAULTS:
            raise BadReport("cases.fault: not on the list")
        if not _int(case["waited_ms"], 0, 120_000) or not _int(case["request"], 0, 100_000):
            raise BadReport("cases: numbers")


class FakeOps:
    """What an app that holds turns, conversation, model.ask and log.send can call."""

    def __init__(self, turns, answer="[]", cap=1, heard=()):
        self._turns, self._answer, self._cap, self._heard = list(turns), answer, cap, list(heard)
        self.asked, self.sent, self.refused = [], [], []

    async def turns(self):
        return list(self._turns)

    async def heard_today(self):
        """What the `conversation.turn` events carried since the app started (the real adapter collects them)."""
        return list(self._heard)

    async def model_ask(self, prompt, input=None):
        """`{prompt, input}` only; the lamp cuts the answer at 1,000 characters."""
        if len(self.asked) >= self._cap:
            raise RateLimited("the daily cap")
        self.asked.append({"prompt": prompt, "input": input})
        return {"text": self._answer[:1000]}

    async def log_send(self, report):
        try:
            strict_check(report)
        except BadReport:
            self.refused.append(report)
            raise
        self.sent.append(report)
        return {"accepted": True}
