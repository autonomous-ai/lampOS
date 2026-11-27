"""Failed and timed-out calls as cases: fixture lines are built from the formats in FAILURE-LINES.md, one per rule."""

import datetime as dt
import json
from pathlib import Path

import pytest

from lamp_doctor import cases
from lamp_doctor.cases import CALL_APP, CALL_CHAT, CALL_LIVE, CALL_MEMORY, CALL_SERVICE, CALL_STT, CALL_TASK, CALL_TTS
from lamp_doctor.cases import ERROR, REFUSED, TIMEOUT

# (journal message, request, call, outcome, reason, ms): each message follows the Rust format of its source line.
FIXTURES = [
    ("lamp-voice second: request 7 chat round 2 had not begun in 4500 ms; the turn ends unanswered", 7, CALL_CHAT, TIMEOUT, "chat round 2 had not begun", 4500),
    ("lamp-voice second: request 8 chat round 1 had not begun in 4500 ms; the onboarding line's own words are said", 8, CALL_CHAT, TIMEOUT, "chat round 1 had not begun", 4500),
    ("lamp-voice second: request 9 look round had not begun in 4500 ms; the lamp says it could not look", 9, CALL_CHAT, TIMEOUT, "look round had not begun", 4500),
    ("lamp-voice second: request 41 chat round 1 failed after 5120 ms: service answered HTTP 503", 41, CALL_CHAT, ERROR, "HTTP 503", 5120),
    ("lamp-voice second: request 42 chat round 2 failed after 8000 ms: chat stream timed out", 42, CALL_CHAT, TIMEOUT, "chat stream timed out", 8000),
    ("lamp-voice second: request 43 chat round 1 failed; the onboarding line's own words are said", 43, CALL_CHAT, ERROR, "chat round 1 failed", None),
    ("lamp-voice second: request 44 the chat refused the look's picture (400); asked again without it", 44, CALL_CHAT, REFUSED, "HTTP 400, the look's picture", None),
    ("lamp-voice second: request 45 the chat refused the history's 3 kept call message(s) (422); asked again without them (LAMP_HISTORY_KEEPS_CALLS)", 45, CALL_CHAT, REFUSED, "HTTP 422, the kept calls", None),
    ("lamp-voice second: request 46 held line's round failed: chat stream cut failed", 46, CALL_CHAT, ERROR, "chat stream cut failed", None),
    ("lamp-voice second: request 47 said it has hit its usage limit (429s for 3600 s, back at 7 AM)", 47, CALL_CHAT, REFUSED, "usage limit (429)", None),
    ("lamp-voice second: DeepSeek: the person's own key was refused (HTTP 401); this request goes through the relay (LAMP_OWN_KEYS)", None, CALL_CHAT, REFUSED, "HTTP 401, the person's own key", None),
    ("lamp-voice second: the person's own model gave no answer; this request goes to the lamp's own (LAMP_OWN_KEYS)", None, CALL_CHAT, TIMEOUT, "the person's own model gave no answer", None),
    ("lamp-voice second: the person's own model couldn't answer (HTTP 500); this request goes to the lamp's own (LAMP_OWN_KEYS)", None, CALL_CHAT, ERROR, "HTTP 500, the person's own model", None),
    ("lamp-voice second: request 50 speech to text failed: speech-to-text final transcript timed out", 50, CALL_STT, TIMEOUT, "speech-to-text final transcript timed out", None),
    ("lamp-voice second: request 51 speech to text failed: speech-to-text connect failed", 51, CALL_STT, ERROR, "speech-to-text connect failed", None),
    ("lamp-voice second: request 52: its speech to text failed after the next line began, and nothing of this line is said (LAMP_SECOND_NAME_CARRIES)", 52, CALL_STT, ERROR, "failed after the next line began", None),
    ("lamp-voice second: ElevenLabs refused the person's own key or voice (HTTP 401); the lamp's own voice from now on (LAMP_OWN_KEYS)", None, CALL_TTS, REFUSED, "HTTP 401, the person's own key or voice", None),
    ("lamp-voice second: the person's own ElevenLabs voice failed a sentence twice (speech socket timed out); the lamp's own voice from now on (LAMP_OWN_KEYS)", None, CALL_TTS, TIMEOUT, "speech socket timed out", None),
    ("lamp-voice second: request 60: the speech socket refused eleven_flash (service answered HTTP 403): no socket is opened ahead for it again until the voice starts again", 60, CALL_TTS, REFUSED, "HTTP 403", None),
    ("lamp-voice second: request 61: the speech socket didn't open (speech socket connect timed out); its pieces go over HTTP", 61, CALL_TTS, TIMEOUT, "speech socket connect timed out", None),
    ("lamp-voice second: request 62: the speech socket failed before its first audio (speech socket closed before its audio failed); the piece goes over HTTP", 62, CALL_TTS, ERROR, "speech socket closed before its audio failed", None),
    ("lamp-voice second: ElevenLabs failed to start a sentence (service answered HTTP 502); asked once more", None, CALL_TTS, ERROR, "HTTP 502", None),
    ("lamp-voice second: ElevenLabs failed again (speech socket failed); the lamp's own voice is asked for it (LAMP_OWN_KEYS)", None, CALL_TTS, ERROR, "speech socket failed", None),
    ("lamp-voice second: the lamp's own voice failed it too (speech socket timed out); the sentence could not be started", None, CALL_TTS, TIMEOUT, "speech socket timed out", None),
    ("lamp-voice second: ElevenLabs failed again (speech socket failed); the sentence could not be started", None, CALL_TTS, ERROR, "speech socket failed", None),
    ("lamp-voice second: a service answered HTTP 502, naming no code", None, CALL_SERVICE, ERROR, "HTTP 502", None),
    ("lamp-voice second: a service answered HTTP 400 (invalid_request)", None, CALL_SERVICE, ERROR, "HTTP 400 invalid_request", None),
    ("lamp-voice second: the service's usage limit (HTTP 429, usage_limit) resets at 1795700000 (in 3600 s)", None, CALL_SERVICE, REFUSED, "HTTP 429 usage limit", None),
    ("lamp-voice: the provider refuses for now (quota, rate limit or key); staying 60 s before leaving, less if the voice's settings change", None, CALL_SERVICE, REFUSED, "quota, rate limit or key", None),
    ("lamp-voice: the provider refused this lamp's key; leaving now to ask once more, and waiting 30 s if it refuses again", None, CALL_SERVICE, REFUSED, "the lamp's key", None),
    ("lamp-voice: request 70: a voice service refused this lamp's key (2 in a row)", 70, CALL_SERVICE, REFUSED, "the lamp's key", None),
    ("lamp-voice: request 71: the model service refused the turn (quota); turns are held and the lamp says so", 71, CALL_SERVICE, REFUSED, "quota", None),
    ("lamp-voice second: no service host answers after a turn failed on the network: the link is down, checked every 10 s", None, CALL_SERVICE, ERROR, "the network link is down", None),
    ("lamp-voice second: a public service's name looked up through another resolver failed after 3000 ms: speech socket connect timed out", None, CALL_SERVICE, TIMEOUT, "speech socket connect timed out", 3000),
    ("lamp-voice history: a forget was refused for good (403, not allowed); let go", None, CALL_SERVICE, REFUSED, "HTTP 403, a forget", None),
    ("lamp-voice provider: request 80 got no answer in 20000 ms; closed as unanswered, connection kept", 80, CALL_LIVE, TIMEOUT, "no answer", 20000),
    ("lamp-voice provider: request 81, said over the lamp's reply, got no answer in 20000 ms; closed, connection kept, and not counted toward a busy room", 81, CALL_LIVE, TIMEOUT, "no answer, said over the lamp's reply", 20000),
    ("lamp-voice provider: reminder request 82 got no answer in 20000 ms; closed, connection kept", 82, CALL_LIVE, TIMEOUT, "no answer, a reminder", 20000),
    ("lamp-voice provider: request 83 was lost at Generating (Transport); its line is kept to send again once on the next session", 83, CALL_LIVE, ERROR, "lost at Generating", None),
    ("lamp-voice provider: connection lost (Transport); output already received is still delivered", None, CALL_LIVE, ERROR, "connection lost", None),
    ("lamp-voice provider: request 84 settled by Timeout after generation completed", 84, CALL_LIVE, ERROR, "settled after generation", None),
    ("lamp-voice provider: refused a server message (Protocol); fields: {a}", None, CALL_LIVE, REFUSED, "a server message", None),
    ("lamp-voice provider: request 90: no answer to the status before preparing, 4000 ms in", 90, CALL_TASK, TIMEOUT, "no answer to the status", 4000),
    ("lamp-voice provider: request 91: no answer to the preparation, 5000 ms in", 91, CALL_TASK, TIMEOUT, "no answer to the preparation", 5000),
    ("lamp-voice: tasks: now was refused: busy", None, CALL_TASK, REFUSED, "busy", None),
    ("lamp-voice second: request 92: the make has no answer 60 s after its waiting line: it runs on, its one line is said, and the turn ends", 92, CALL_TASK, TIMEOUT, "the make has no answer", 60000),
    ("[lamp-tasks] skill task 12: Harness didn't make the fixed write's run (new_failed) and didn't answer within 30 s, so it isn't asked again", None, CALL_TASK, TIMEOUT, "Harness did not answer", 30000),
    ("[lamp-tasks] skill task 13: Harness didn't make the fixed write's run (new_failed); it answers now with nothing in the run's folder, so it is asked once more", None, CALL_TASK, ERROR, "Harness did not make the run", None),
    ("[lamp-tasks] skill task 14: a run without Harness is unavailable; Harness is signed out, so trying Harness's own lamp target", None, CALL_TASK, REFUSED, "Harness is signed out", None),
    ("lamp-voice provider: no Days memory worker; memory unavailable", None, CALL_MEMORY, ERROR, "memory unavailable", None),
    ("lamp-voice provider: the memory refused a room line's keeper; kept without it", None, CALL_MEMORY, REFUSED, "a room line's keeper", None),
    ("lamp-control: app my-app: a call's end not told to the voice: broken pipe", None, CALL_APP, ERROR, "a call's end was not told", None),
]


@pytest.mark.parametrize("message,request_no,call,outcome,reason,ms", FIXTURES)
def test_each_failure_line_becomes_one_case(message, request_no, call, outcome, reason, ms):
    case = cases.parse_message(message)
    assert case is not None, message
    assert (case.request, case.call, case.outcome, case.reason, case.ms) == (request_no, call, outcome, reason, ms)


def test_every_rule_has_a_fixture_and_a_source_in_the_list():
    seen = {cases.parse_message(m).source for m, *_ in FIXTURES}
    assert seen == {rule[4] for rule in cases.RULES}
    doc = (Path(__file__).resolve().parent.parent / "FAILURE-LINES.md").read_text()
    for source in seen:
        assert source.split(":")[1] in doc, source


def test_free_text_outside_the_vocabulary_is_other_never_echoed():
    case = cases.parse_message("lamp-voice second: request 5 speech to text failed: turn the bedroom light blue please Anna")
    assert case.reason == "other" and "Anna" not in repr(case)
    kind = cases.parse_message("lamp-voice: request 71: the model service refused the turn (Anna said blue); turns are held")
    assert kind.reason == "other"


def test_a_line_the_lamp_says_is_not_a_failure_and_never_read():
    line = 'lamp-voice second: request 9: an app tool\'s call is unanswered at 2500 ms with nothing said; said "one moment, Anna" (x)'
    assert cases.parse_message(line) is None


def test_journalctl_json_lines_give_time_and_order_and_skip_the_rest():
    def entry(message, micros):
        return json.dumps({"MESSAGE": message, "__REALTIME_TIMESTAMP": str(micros)})

    base = int(dt.datetime(2026, 11, 26, 14, 0, 0).timestamp() * 1_000_000)
    lines = [
        entry("lamp-voice second: request 2 speech to text failed: speech-to-text connect timed out", base + 2_000_000),
        entry("lamp-voice second: request 1 chat round 1 had not begun in 4500 ms; the turn ends unanswered", base + 1_000_000),
        entry("lamp-voice second: request 3 heard 5 words, the last 200 ms after the line ended", base + 3_000_000),
        json.dumps({"MESSAGE": [1, 2, 3], "__REALTIME_TIMESTAMP": str(base)}),
        entry("kernel: something else failed", base),
        "not json {",
        "",
    ]
    got = cases.read_cases(lines)
    assert [c.request for c in got] == [1, 2]
    assert got[0].at == dt.datetime(2026, 11, 26, 14, 0, 1)
    assert cases.case_line(got[0]).startswith("14:00:01  request 1  LLM chat  timeout  chat round 1 had not begun, 4500 ms")


def test_no_case_field_holds_a_planted_sentence():
    planted = "turn the bedroom light blue please Anna"
    lines = [
        f"lamp-voice second: request 5 chat round 1 failed after 900 ms: {planted}",
        f"lamp-voice: tasks: now was refused: {planted}",
        f"lamp-voice second: request 6 held line's round failed: {planted}",
    ]
    for case in cases.read_cases(lines):
        assert planted not in repr(case) and "Anna" not in repr(case)
