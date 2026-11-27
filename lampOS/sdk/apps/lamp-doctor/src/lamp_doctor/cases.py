"""Failed and timed-out external calls, as CASES, from the lamp's journal. Never the words.

Each rule matches the exact format of one journal line the lamp writes when a call fails or times out
(`FAILURE-LINES.md` lists them by file and line). A case holds: when, the request number, which call, the outcome
(error, timeout or refused), a reason from a fixed vocabulary or an HTTP status, and the ms waited. Free text
that is not in the fixed vocabulary becomes "other"; a quoted line (the lamp's own words) is never read.

Input: `journalctl -o json` lines (one JSON object each, `MESSAGE` and `__REALTIME_TIMESTAMP` in microseconds), or
plain text lines. Nothing here opens a connection.
"""

from __future__ import annotations

import datetime as dt
import json
import re
from dataclasses import dataclass
from typing import Any, Callable, Iterable

ERROR, TIMEOUT, REFUSED = "error", "timeout", "refused"

CALL_CHAT, CALL_STT, CALL_TTS = "LLM chat", "speech-to-text", "text-to-speech"
CALL_SERVICE, CALL_LIVE, CALL_TASK = "service", "live provider", "Harness task"
CALL_MEMORY, CALL_APP = "memory", "app call"

_STEP = re.compile(r"^[a-z][a-z' -]{0,47}$")  # a step word of the code: lower-case words only
_KIND = re.compile(r"^[a-z_ ]{1,32}$")
_CODE = re.compile(r"^[a-z_]{1,32}$")


@dataclass(frozen=True)
class Case:
    at: dt.datetime | None
    request: int | None
    call: str
    outcome: str
    reason: str
    ms: int | None
    source: str  # the code's file:line that writes the line

    @property
    def problem(self) -> str | None:
        """The v6 failure this case is, if it ended the person's turn with nothing said: "no_answer" or "error".
        A fallback that still answered (relay, own voice, a line said in place) is a case only: None."""
        return TURN_ENDERS.get(self.source) if self.request is not None else None


# source line -> the v6 failure of a turn it ended (v6 q_failure: no_answer weight 1.0, error 1.0)
TURN_ENDERS = {
    "voice/src/second_provider.rs:20914": "no_answer",  # "had not begun ...; the turn ends unanswered"
    "voice/src/second_provider.rs:20971": "error",      # a chat round failed
    "voice/src/second_provider.rs:10663": "error",      # speech to text failed
    "voice/src/second_provider.rs:10626": "error",      # speech to text failed after the next line began
}


def classify(error: str) -> tuple[str, str]:
    """`net::Error`'s Display (second_provider/net.rs): a fixed vocabulary, never the service's own text."""
    text = error.strip()
    if text == "unusable service URL":
        return ERROR, "unusable service URL"
    if m := re.fullmatch(r"service answered HTTP (\d+) \(usage limit\)", text):
        return REFUSED, f"HTTP {m[1]} usage limit"
    if m := re.fullmatch(r"service answered HTTP (\d+) \(the key invalid\)", text):
        return REFUSED, f"HTTP {m[1]} key invalid"
    if m := re.fullmatch(r"service answered HTTP (\d+)", text):
        return (REFUSED if m[1] in ("401", "402", "403", "429") else ERROR), f"HTTP {m[1]}"
    if (m := re.fullmatch(r"(.+) timed out", text)) and _STEP.match(m[1]):
        return TIMEOUT, f"{m[1]} timed out"
    if (m := re.fullmatch(r"(.+) failed", text)) and _STEP.match(m[1]):
        return ERROR, f"{m[1]} failed"
    if (m := re.fullmatch(r"unexpected (.+)", text)) and _STEP.match(m[1]):
        return ERROR, f"unexpected {m[1]}"
    return ERROR, "other"


def _int(value: str | None) -> int | None:
    return int(value) if value is not None and value.isdigit() else None


def _word(value: str | None, pattern: re.Pattern[str]) -> str:
    return value if value is not None and pattern.match(value) else "other"


# (pattern, call, outcome or None = by the {error} classification, reason, source). Group names: request, ms, round,
# status, error, seconds, kind, code, stage. A reason is a template over the groups, or "{error}" for the classified one.
R = [
    # LLM chat (second route)
    (r"request (?P<request>\d+) chat round (?P<round>\d+) had not begun in (?P<ms>\d+) ms; the turn ends unanswered",
     CALL_CHAT, TIMEOUT, "chat round {round} had not begun", "voice/src/second_provider.rs:20914"),
    (r"request (?P<request>\d+) chat round (?P<round>\d+) had not begun in (?P<ms>\d+) ms; the onboarding line's own words are said",
     CALL_CHAT, TIMEOUT, "chat round {round} had not begun", "voice/src/second_provider.rs:20902"),
    (r"request (?P<request>\d+) look round had not begun in (?P<ms>\d+) ms; the lamp says it could not look",
     CALL_CHAT, TIMEOUT, "look round had not begun", "voice/src/second_provider.rs:20894"),
    (r"request (?P<request>\d+) chat round (?P<round>\d+) failed after (?P<ms>\d+) ms: (?P<error>.+)$",
     CALL_CHAT, None, "{error}", "voice/src/second_provider.rs:20971"),
    (r"request (?P<request>\d+) chat round (?P<round>\d+) failed; the onboarding line's own words are said",
     CALL_CHAT, ERROR, "chat round {round} failed", "voice/src/second_provider.rs:21024"),
    (r"request (?P<request>\d+) the chat refused the look's picture \((?P<status>\d+)\)",
     CALL_CHAT, REFUSED, "HTTP {status}, the look's picture", "voice/src/second_provider.rs:20956"),
    (r"request (?P<request>\d+) the chat refused the history's \d+ kept call message\(s\) \((?P<status>\d+)\)",
     CALL_CHAT, REFUSED, "HTTP {status}, the kept calls", "voice/src/second_provider.rs:20985"),
    (r"request (?P<request>\d+) held line's round failed: (?P<error>.+)$",
     CALL_CHAT, None, "{error}", "voice/src/second_provider.rs:15757"),
    (r"request (?P<request>\d+) said it has hit its usage limit \(429s for \d+ s",
     CALL_CHAT, REFUSED, "usage limit (429)", "voice/src/second_provider.rs:9995"),
    (r"DeepSeek: the person's own key was refused \(HTTP (?P<status>\d+)\)",
     CALL_CHAT, REFUSED, "HTTP {status}, the person's own key", "voice/src/second_provider/worker.rs:1035"),
    (r"the person's own model gave no answer; this request goes to the lamp's own",
     CALL_CHAT, TIMEOUT, "the person's own model gave no answer", "voice/src/second_provider/worker.rs:1039"),
    (r"the person's own model couldn't answer \(HTTP (?P<status>\d+)\)",
     CALL_CHAT, ERROR, "HTTP {status}, the person's own model", "voice/src/second_provider/worker.rs:1043"),
    # Speech to text
    (r"request (?P<request>\d+) speech to text failed: (?P<error>.+)$",
     CALL_STT, None, "{error}", "voice/src/second_provider.rs:10663"),
    (r"request (?P<request>\d+): its speech to text failed after the next line began",
     CALL_STT, ERROR, "failed after the next line began", "voice/src/second_provider.rs:10626"),
    # Text to speech
    (r"ElevenLabs refused the person's own key or voice \(HTTP (?P<status>\d+)\)",
     CALL_TTS, REFUSED, "HTTP {status}, the person's own key or voice", "voice/src/second_provider/net/tts.rs:233"),
    (r"the person's own ElevenLabs voice failed a sentence twice \((?P<error>.+)\); the lamp's own voice from now on",
     CALL_TTS, None, "{error}", "voice/src/second_provider/net/tts.rs:246"),
    (r"request (?P<request>\d+): the speech socket refused .+? \((?P<error>[^()]+)\): no socket is opened ahead",
     CALL_TTS, REFUSED, "{error}", "voice/src/second_provider/net/tts.rs:801"),
    (r"request (?P<request>\d+): the speech socket didn't open \((?P<error>.+)\); its pieces go over HTTP",
     CALL_TTS, None, "{error}", "voice/src/second_provider/net/tts.rs:894"),
    (r"request (?P<request>\d+): the speech socket failed before its first audio \((?P<error>.+)\); the piece goes over HTTP",
     CALL_TTS, None, "{error}", "voice/src/second_provider/net/tts.rs:1173"),
    (r"failed to start a sentence \((?P<error>[^()]+)\); asked once more",
     CALL_TTS, None, "{error}", "voice/src/second_provider/services.rs:1139"),
    (r"failed again \((?P<error>[^()]+)\); the lamp's own voice is asked for it",
     CALL_TTS, None, "{error}", "voice/src/second_provider/services.rs:1154"),
    (r"the lamp's own voice failed it too \((?P<error>[^()]+)\); the sentence could not be started",
     CALL_TTS, None, "{error}", "voice/src/second_provider/services.rs:1162"),
    (r"failed again \((?P<error>[^()]+)\); the sentence could not be started",
     CALL_TTS, None, "{error}", "voice/src/second_provider/services.rs:1170"),
    # Any provider's HTTP answer
    (r"a service answered HTTP (?P<status>\d+), naming no code",
     CALL_SERVICE, ERROR, "HTTP {status}", "voice/src/second_provider/net.rs:282"),
    (r"a service answered HTTP (?P<status>\d+) \((?P<code>[a-z_]+)\)",
     CALL_SERVICE, ERROR, "HTTP {status} {code}", "voice/src/second_provider/net.rs:285"),
    (r"the service's usage limit \(HTTP (?P<status>\d+), usage_limit\)",
     CALL_SERVICE, REFUSED, "HTTP {status} usage limit", "voice/src/second_provider/net.rs:295"),
    (r"lamp-voice: the provider refuses for now \(quota, rate limit or key\)",
     CALL_SERVICE, REFUSED, "quota, rate limit or key", "voice/src/coordinator.rs:3349"),
    (r"lamp-voice: the provider refused this lamp's key",
     CALL_SERVICE, REFUSED, "the lamp's key", "voice/src/coordinator.rs:3372"),
    (r"request (?P<request>\d+): a voice service refused this lamp's key \(\d+ in a row\)",
     CALL_SERVICE, REFUSED, "the lamp's key", "voice/src/coordinator/key_refusals.rs:38"),
    (r"request (?P<request>\d+): the model service refused the turn \((?P<kind>[^)]*)\)",
     CALL_SERVICE, REFUSED, "{kind}", "voice/src/coordinator/model_refusing.rs:125"),
    (r"no service host answers after a turn failed on the network: the link is down",
     CALL_SERVICE, ERROR, "the network link is down", "voice/src/second_provider/worker.rs:16520"),
    (r"a public service's name looked up through another resolver failed after (?P<ms>\d+) ms: (?P<error>.+)$",
     CALL_SERVICE, None, "{error}", "voice/src/second_provider/net/public.rs:539"),
    (r"lamp-voice history: a forget was refused for good \((?P<status>\d+),",
     CALL_SERVICE, REFUSED, "HTTP {status}, a forget", "voice/src/second_provider/net/history.rs:164"),
    # Live route
    (r"lamp-voice provider: request (?P<request>\d+) got no answer in (?P<ms>\d+) ms; closed as unanswered",
     CALL_LIVE, TIMEOUT, "no answer", "voice/src/provider_worker.rs:5137"),
    (r"lamp-voice provider: request (?P<request>\d+), said over the lamp's reply, got no answer in (?P<ms>\d+) ms",
     CALL_LIVE, TIMEOUT, "no answer, said over the lamp's reply", "voice/src/provider_worker.rs:5130"),
    (r"lamp-voice provider: reminder request (?P<request>\d+) got no answer in (?P<ms>\d+) ms",
     CALL_LIVE, TIMEOUT, "no answer, a reminder", "voice/src/provider_worker.rs:5149"),
    (r"lamp-voice provider: request (?P<request>\d+) was lost at (?P<stage>[A-Za-z]+) \((?P<error>[^()]*)\); its line is kept",
     CALL_LIVE, ERROR, "lost at {stage}", "voice/src/provider_worker.rs:3743"),
    (r"lamp-voice provider: connection lost \((?P<error>[^()]*)\); output already received is still delivered",
     CALL_LIVE, ERROR, "connection lost", "voice/src/provider_worker.rs:3724"),
    (r"lamp-voice provider: request (?P<request>\d+) settled by (?P<error>.+?) after generation completed",
     CALL_LIVE, ERROR, "settled after generation", "voice/src/provider_worker.rs:3730"),
    (r"lamp-voice provider: refused a server message",
     CALL_LIVE, REFUSED, "a server message", "voice/src/provider_worker.rs:5167"),
    # Harness and agent tasks
    (r"lamp-voice provider: request (?P<request>\d+): no answer to the status before preparing, (?P<ms>\d+) ms in",
     CALL_TASK, TIMEOUT, "no answer to the status", "voice/src/agent_tasks.rs:4901"),
    (r"lamp-voice provider: request (?P<request>\d+): no answer to the preparation, (?P<ms>\d+) ms in",
     CALL_TASK, TIMEOUT, "no answer to the preparation", "voice/src/agent_tasks.rs:5118"),
    (r"lamp-voice: tasks: .+ was refused: (?P<code>[a-z_]+)$",
     CALL_TASK, REFUSED, "{code}", "voice/src/agent_tasks.rs:6348"),
    (r"request (?P<request>\d+): the make has no answer (?P<seconds>\d+) s after its waiting line",
     CALL_TASK, TIMEOUT, "the make has no answer", "voice/src/second_provider.rs:18707"),
    (r"\[lamp-tasks\] skill task \d+: Harness didn't make the fixed write's run \(new_failed\) and didn't answer within (?P<seconds>\d+) s",
     CALL_TASK, TIMEOUT, "Harness did not answer", "tasks/src/skill_task.rs:4127"),
    (r"\[lamp-tasks\] skill task \d+: Harness didn't make the fixed write's run \(new_failed\)",
     CALL_TASK, ERROR, "Harness did not make the run", "tasks/src/skill_task.rs:4113"),
    (r"\[lamp-tasks\] skill task \d+: a run without Harness is unavailable; Harness is signed out",
     CALL_TASK, REFUSED, "Harness is signed out", "tasks/src/skill_task.rs:3172"),
    # Memory and app calls
    (r"lamp-voice provider: no Days memory worker; memory unavailable",
     CALL_MEMORY, ERROR, "memory unavailable", "voice/src/remember.rs:7121"),
    (r"lamp-voice provider: the memory refused a room line's keeper",
     CALL_MEMORY, REFUSED, "a room line's keeper", "voice/src/remember.rs:2900"),
    (r"lamp-control: app (?P<app>[a-z][a-z0-9-]*): a call's end not told to the voice: (?P<error>.+)$",
     CALL_APP, ERROR, "a call's end was not told", "control/src/app_calls.rs:173"),
]
RULES = [(re.compile(p), call, outcome, reason, source) for p, call, outcome, reason, source in R]
# Matched only by an earlier, more specific rule: the failed-again lines share a stem, so first match wins.


def parse_message(message: str, at: dt.datetime | None = None) -> Case | None:
    for pattern, call, outcome, reason, source in RULES:
        m = pattern.search(message)
        if not m:
            continue
        g = m.groupdict()
        error_outcome, error_reason = classify(g["error"]) if "error" in g and g["error"] is not None else (None, None)
        text = reason.format_map({
            "round": _int(g.get("round")) if g.get("round") else "",
            "status": _int(g.get("status")) if g.get("status") else "",
            "seconds": _int(g.get("seconds")) or "",
            "stage": _word(g.get("stage"), re.compile(r"^[A-Za-z]{1,24}$")),
            "kind": _word(g.get("kind"), _KIND),
            "code": _word(g.get("code"), _CODE),
            "error": error_reason or "",
        })
        ms = _int(g.get("ms"))
        if ms is None and g.get("seconds"):
            ms = int(g["seconds"]) * 1000
        return Case(at, _int(g.get("request")), call, outcome or error_outcome or ERROR, text, ms, source)
    return None


def _message_of(entry: Any) -> str | None:
    text = entry.get("MESSAGE") if isinstance(entry, dict) else None
    if isinstance(text, list):  # journald gives non-text messages as byte arrays
        return None
    return text if isinstance(text, str) else None


def read_cases(lines: Iterable[str]) -> list[Case]:
    """Cases from `journalctl -o json` lines (or plain lines), in time order. Everything else is ignored."""
    cases: list[Case] = []
    for raw in lines:
        raw = raw.strip()
        if not raw:
            continue
        at, message = None, raw
        if raw.startswith("{"):
            try:
                entry = json.loads(raw)
            except ValueError:
                continue
            message = _message_of(entry)
            if message is None:
                continue
            try:
                at = dt.datetime.fromtimestamp(int(entry["__REALTIME_TIMESTAMP"]) / 1_000_000)
            except (KeyError, ValueError, TypeError):
                at = None
        if "lamp-" not in message and "[lamp-" not in message:
            continue
        if (case := parse_message(message, at)) is not None:
            cases.append(case)
    return sorted(cases, key=lambda c: c.at or dt.datetime.min)


def case_line(case: Case) -> str:
    when = case.at.strftime("%H:%M:%S") if case.at else "--:--:--"
    request = f"request {case.request}" if case.request is not None else "no request"
    waited = f", {case.ms} ms" if case.ms is not None else ""
    return f"{when}  {request}  {case.call}  {case.outcome}  {case.reason}{waited}"


def unanswered_turns(cases: list[Case], answered: set[int]) -> list[dict]:
    """One turn record per request a case ended: not answered, no first sound, the v6 failure in `problem`.
    A request that has an answered row stays a case only. Two cases of one request count once, no_answer first."""
    found: dict[int, tuple[Case, str]] = {}
    for case in cases:
        problem = case.problem
        if problem is None or case.request in answered:
            continue
        if case.request not in found or problem == "no_answer":
            found[case.request] = (case, problem)
    return [{
        "turn": request, "at": int(case.at.timestamp() // 60) if case.at else 0, "route": "live", "build": "viewer",
        "outcome": "unanswered", "problem": problem, "heard_to_first_sound_ms": None, "cut_ins": 0, "calls": [],
        "words_heard": 0, "words_said": 0, "heard_wrongly": False, "name_missed": False, "held_then_repeated": False,
    } for request, (case, problem) in sorted(found.items())]
