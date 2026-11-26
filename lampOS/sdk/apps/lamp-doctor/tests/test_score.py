"""The score from a set of turn records: numbers only, no text, no model."""

from fake_ops import turn
from lamp_doctor.score import score_day


def test_an_empty_day_scores_nothing():
    day = score_day([])
    assert day["turns"] == 0
    assert day["score"] is None and day["tier"] is None


def test_all_answered_within_5s_scores_high():
    day = score_day([turn(turn=i, heard_to_first_sound_ms=1500) for i in range(10)])
    assert day["turns"] == 10
    assert day["answered_within_5s"] == 100
    assert day["score"] == 10.0 and day["band"] == "green"
    assert day["failures"] == {}


def test_the_numbers_are_counted_from_the_records():
    records = [
        turn(turn=1, heard_to_first_sound_ms=1000),
        turn(turn=2, heard_to_first_sound_ms=3000),
        turn(turn=3, heard_to_first_sound_ms=9000),
        turn(turn=4, outcome="held", heard_to_first_sound_ms=None),
        turn(turn=5, outcome="asked_again", heard_to_first_sound_ms=2000, heard_wrongly=True),
        turn(turn=6, outcome="stuck", heard_to_first_sound_ms=None),
        turn(turn=7, heard_to_first_sound_ms=2000, cut_ins=2),
        turn(turn=8, heard_to_first_sound_ms=2000, calls=[{"name": "set_light", "ms": 4500, "timed_out": True}]),
    ]
    day = score_day(records)
    assert day["turns"] == 8
    assert day["held"] == 1 and day["asked_again"] == 1 and day["stuck"] == 1
    assert day["cut_ins"] == 2 and day["tool_timeouts"] == 1
    # answered within 5 s: turns 1, 2, 5, 7, 8 of 8
    assert day["answered_within_5s"] == 62.5
    assert day["heard_to_first_sound_ms_median"] == 2000
    assert day["heard_to_first_sound_ms_p90"] == 3000 or day["heard_to_first_sound_ms_p90"] == 9000


def test_failure_categories_come_from_the_records_and_are_fixed():
    day = score_day([
        turn(turn=1, heard_wrongly=True),
        turn(turn=2, heard_wrongly=True),
        turn(turn=3, outcome="held", heard_to_first_sound_ms=None),
        turn(turn=4, heard_to_first_sound_ms=9000),
        turn(turn=5, calls=[{"name": "x", "ms": 4500, "timed_out": True}]),
        turn(turn=6, outcome="stuck", heard_to_first_sound_ms=None, words_heard=0),
    ])
    assert day["failures"] == {
        "heard_wrongly": 2, "call_held": 1, "slow_first_sound": 1, "tool_timeout": 1, "silent_turn": 1,
    }


def test_a_worse_day_scores_lower():
    good = score_day([turn(turn=i, heard_to_first_sound_ms=1500) for i in range(10)])
    bad = score_day([turn(turn=i, heard_to_first_sound_ms=9000, outcome="asked_again", heard_wrongly=True) for i in range(10)])
    assert 0 <= bad["score"] < good["score"] <= 100


def test_the_score_is_v6s_voice_part_from_0_to_10():
    for records in ([turn()] * 3, [turn(outcome="stuck", heard_to_first_sound_ms=None)] * 5):
        value = score_day(records)["score"]
        assert isinstance(value, float) and 0 <= value <= 10


def test_under_three_questions_there_is_no_voice_score_and_under_twenty_it_is_marked_few():
    assert score_day([turn(), turn()])["score"] is None
    assert score_day([turn()] * 5)["few"] is True and score_day([turn()] * 20)["few"] is False


def test_the_tier_follows_the_deepest_tool_used():
    assert score_day([turn(calls=[{"name": "set_light", "ms": 300, "timed_out": False}])])["tier"] == "T1"
    assert score_day([turn(calls=[{"name": "start_agent_task", "ms": 900, "timed_out": False}])])["tier"] == "T4"
    assert score_day([turn()])["tier"] == "T1"


def test_route_and_build_are_the_days_most_common():
    day = score_day([turn(route="live", build="1"), turn(route="live", build="1"), turn(route="second", build="1")])
    assert (day["route"], day["build"]) == ("live", "1")


def test_scoring_reads_no_text_even_if_a_record_carried_some():
    """A record with a stray text field is scored as if it were not there."""
    stray = turn(text="turn the light blue, Anna")
    day = score_day([stray])
    assert "Anna" not in repr(day) and "blue" not in repr(day)


def test_a_day_with_no_first_sound_sends_numbers_not_nulls():
    day = score_day([turn(outcome="held", heard_to_first_sound_ms=None), turn(outcome="stuck", heard_to_first_sound_ms=None)])
    assert day["heard_to_first_sound_ms_median"] == 0 and day["heard_to_first_sound_ms_p90"] == 0
