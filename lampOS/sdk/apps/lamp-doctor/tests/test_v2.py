"""v2: the turns come in as events, the day goes out as one `log.send`, hourly, never beside the readers' grants."""

import asyncio

import pytest
from fake_ops import strict_check, turn

from lamp_doctor import v2
from lampos import client, schema
from lampos.errors import LampError


class StubLamp:
    def __init__(self, errors=()):
        self.handlers, self.sent, self.errors = {}, [], list(errors)

    def on(self, event, handler):
        self.handlers[event] = handler

    async def request(self, op, **fields):
        client._check(op, fields)  # the SDK's own name check
        if self.errors:
            raise self.errors.pop(0)
        self.sent.append((op, fields))
        return {}


def event(**over):
    return {"ev": "turns.finished", **turn(**over)}


def fill(ledger, n=5):
    for i in range(n):
        ledger.add(event(turn=i))


def test_grants_are_turns_and_report_and_device_only():
    assert set(v2.GRANTS) == {"turns", "report", "device"}
    readers = {"hear", "speech", "microphone", "camera", "notes", "remember"}
    assert not readers & set(v2.GRANTS)


def test_the_event_and_the_op_are_in_the_schema():
    assert "turns.finished" in schema.app_events() and "log.send" in schema.app_ops()


def test_a_turn_event_is_kept_without_its_ev():
    ledger = v2.Ledger()
    ledger.add(event())
    assert "ev" not in ledger.records[0] and ledger.fresh()


def test_the_day_so_far_goes_out_in_the_agreed_schema():
    lamp, ledger = StubLamp(), v2.Ledger()
    fill(ledger)
    assert asyncio.run(v2.send_once(lamp, ledger)) == v2.SEND_EVERY_S
    (op, fields), = lamp.sent
    assert op == "log.send"
    strict_check(fields["report"])
    assert "day" not in fields["report"] and "build" not in fields["report"]  # lamp-control sets both


def test_nothing_new_nothing_sent():
    lamp, ledger = StubLamp(), v2.Ledger()
    fill(ledger)
    asyncio.run(v2.send_once(lamp, ledger))
    asyncio.run(v2.send_once(lamp, ledger))
    assert len(lamp.sent) == 1
    ledger.add(event(turn=99))
    asyncio.run(v2.send_once(lamp, ledger))
    assert len(lamp.sent) == 2


def test_under_three_questions_nothing_is_sent():
    lamp, ledger = StubLamp(), v2.Ledger()
    fill(ledger, 2)
    asyncio.run(v2.send_once(lamp, ledger))
    assert lamp.sent == []


def test_a_rate_refusal_waits_its_retry_ms_and_sends_the_same_day_again():
    lamp, ledger = StubLamp(errors=[LampError("rate", code="rate", retry_ms=90_000)]), v2.Ledger()
    fill(ledger)
    assert asyncio.run(v2.send_once(lamp, ledger)) == 90
    assert lamp.sent == [] and ledger.fresh()
    asyncio.run(v2.send_once(lamp, ledger))
    assert len(lamp.sent) == 1


def test_off_or_refused_tries_next_hour_and_no_grant_is_raised():
    for code in ("off", "refused"):
        lamp, ledger = StubLamp(errors=[LampError("x", code=code)]), v2.Ledger()
        fill(ledger)
        assert asyncio.run(v2.send_once(lamp, ledger)) == v2.SEND_EVERY_S and ledger.fresh()
    lamp, ledger = StubLamp(errors=[LampError("x", code="no_grant")]), v2.Ledger()
    fill(ledger)
    with pytest.raises(LampError):
        asyncio.run(v2.send_once(lamp, ledger))


def test_run_subscribes_and_sends_on_each_round():
    lamp, ledger, waits = StubLamp(), v2.Ledger(), []

    async def sleep(s):
        waits.append(s)
        lamp.handlers["turns.finished"](event(turn=len(waits) + 10))

    fill(ledger)
    asyncio.run(v2.run(lamp, ledger, sleep=sleep, rounds=2))
    assert "turns.finished" in lamp.handlers
    assert len(lamp.sent) == 2 and waits == [v2.SEND_EVERY_S] * 2


def test_the_memory_is_bounded():
    ledger = v2.Ledger()
    for i in range(v2.MAX_RECORDS + 10):
        ledger.add(event(turn=i))
    assert len(ledger.records) == v2.MAX_RECORDS
