"""A diagnosis with free text never reaches log.send."""

import asyncio

import pytest

from fake_ops import FAILURES, FakeOps, turn
from lamp_doctor.diagnose import diagnose, parse_categories

SENTENCE = "turn the bedroom light blue please Anna"


def run(coro):
    return asyncio.run(asyncio.wait_for(coro, 10))


def test_parse_keeps_only_listed_categories():
    answer = '["misheard", "' + SENTENCE + '", "error", 7, null]'
    assert parse_categories(answer) == ["misheard", "error"]


def test_parse_of_prose_keeps_the_listed_words_and_nothing_else():
    answer = f'The person said "{SENTENCE}" and the lamp was slow: very_slow, stuck.'
    assert parse_categories(answer) == ["very_slow", "stuck"]


def test_parse_of_nothing_useful_is_empty():
    for answer in ("", "I cannot tell", "{", None):
        assert parse_categories(answer) == []


def test_a_diagnosis_asks_the_model_once_and_returns_categories():
    ops = FakeOps([turn()], answer='["no_answer"]')
    assert run(diagnose(ops, {"turns": 1, "score": 80})) == ["no_answer"]
    assert len(ops.asked) == 1


def test_the_prompt_lists_the_categories_and_asks_for_nothing_else():
    ops = FakeOps([turn()], answer="[]")
    run(diagnose(ops, {"turns": 1}))
    prompt = ops.asked[0]["prompt"]
    assert all(name in prompt for name in FAILURES)


def test_at_the_cap_the_diagnosis_is_empty_and_does_not_raise():
    ops = FakeOps([turn()], answer='["no_answer"]', cap=0)
    assert run(diagnose(ops, {"turns": 1})) == []


def test_a_model_that_raises_gives_an_empty_diagnosis():
    ops = FakeOps([turn()])

    async def boom(*a, **k):
        raise RuntimeError("the provider is down")

    ops.model_ask = boom
    assert run(diagnose(ops, {"turns": 1})) == []


def test_model_ask_gets_only_prompt_and_input():
    ops = FakeOps([turn()], answer="[]")
    run(diagnose(ops, {"turns": 1}))
    assert set(ops.asked[0]) == {"prompt", "input"}
