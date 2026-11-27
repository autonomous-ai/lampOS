"""A whole day against a fake of Ten's ops: what leaves is the schema, always."""

import asyncio
import json

from fake_ops import FAILURES, BadReport, FakeOps, strict_check, turn
from lamp_doctor.app import run_day

SENTENCE = "turn the bedroom light blue please Anna"


def run(coro):
    return asyncio.run(asyncio.wait_for(coro, 10))


def test_a_day_sends_one_report_and_it_passes_the_strict_check():
    ops = FakeOps([turn(turn=i) for i in range(5)], answer='["no_answer"]')
    sent = run(run_day(ops))
    assert sent is True
    assert len(ops.sent) == 1 and not ops.refused
    strict_check(ops.sent[0])
    assert ops.sent[0]["turns"] == 5


def test_a_model_that_echoes_the_heard_sentence_does_not_get_it_out():
    ops = FakeOps([turn(turn=i) for i in range(3)], answer=json.dumps([SENTENCE, "misheard"]), heard=[{"heard": SENTENCE}])
    run(run_day(ops))
    assert not ops.refused
    wire = json.dumps(ops.sent)
    assert "Anna" not in wire and "bedroom" not in wire and SENTENCE not in wire


def test_heard_words_may_go_to_the_model_but_never_to_log_send():
    ops = FakeOps([turn(turn=i) for i in range(3)], answer='["misheard"]', heard=[{"heard": SENTENCE}])
    run(run_day(ops))
    assert not any(SENTENCE in json.dumps(r) for r in ops.sent)


def test_no_turns_sends_nothing():
    ops = FakeOps([])
    assert run(run_day(ops)) is False
    assert ops.sent == [] and ops.asked == []


def test_a_day_with_the_cap_already_reached_still_reports():
    ops = FakeOps([turn(turn=i) for i in range(3)], cap=0)
    assert run(run_day(ops)) is True
    assert {f["category"] for f in ops.sent[0]["failures"]} <= {"misheard", "slow_first_sound",
        "call_held", "answer_cut_off", "tool_timeout", "no_answer"}


def test_a_refused_report_is_sent_again_later_unchanged():
    ops = FakeOps([turn(turn=i) for i in range(3)])
    calls, naps = [], []

    async def refuse(report):
        calls.append(json.dumps(report, sort_keys=True))
        raise BadReport("the lamp is private")

    async def nap(seconds):
        naps.append(seconds)

    ops.log_send = refuse
    assert run(run_day(ops, tries=3, retry_s=3600, sleep=nap)) is False
    assert len(calls) == 3 and len(set(calls)) == 1
    assert naps == [3600, 3600]


def test_a_report_refused_once_goes_the_next_time():
    ops = FakeOps([turn(turn=i) for i in range(3)])
    real, count = ops.log_send, []

    async def flaky(report):
        count.append(1)
        if len(count) == 1:
            raise BadReport("the lamp is private")
        return await real(report)

    async def nap(seconds):
        pass

    ops.log_send = flaky
    assert run(run_day(ops, sleep=nap)) is True
    assert len(ops.sent) == 1


def test_under_three_questions_nothing_is_sent():
    ops = FakeOps([turn(turn=1)])
    assert run(run_day(ops)) is False and ops.sent == []
