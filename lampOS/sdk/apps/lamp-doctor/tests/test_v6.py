"""The v6 port: the owner's numbers (#7452), checked against the values its own docstring states."""

import math

from lamp_doctor import v6


def q(ms=1500, answered=True, problem=None):
    return {"ms": ms, "answered": answered, "problem": problem}


def test_reliability_follows_the_documented_examples():
    # "2 % failing -> 8.4, 5 % -> 6.4, 10 % -> 4.1, 20 % -> 1.7" (a weight-1 failure: no_answer)
    for pct, expected in ((2, 8.4), (5, 6.4), (10, 4.1), (20, 1.7)):
        qs = [q(problem="no_answer", answered=False, ms=None)] * pct + [q()] * (100 - pct)
        assert round(v6.reliability(qs), 1) == expected


def test_failure_kinds_and_weights():
    assert v6.failure(q(problem="stuck", answered=False, ms=None)) == "stuck"
    assert v6.failure(q(ms=9000)) == "very_slow" and v6.failure(q(ms=8000)) is None
    assert v6.FAIL["stuck"] == 1.5 and v6.FAIL["misheard"] == 0.5 and v6.FAIL["very_slow"] == 0.5
    assert v6.SOFT == {"misheard", "very_slow"}


def test_speed_points_are_the_owners_scale():
    assert [v6.speed_pts(m) for m in (2000, 3000, 5000, 8000, 8001)] == [1.0, 0.7, 0.4, 0.2, 0.0]


def test_count_caps_and_depth_caps():
    stuck = lambda: q(problem="stuck", answered=False, ms=None)
    assert v6.count_cap([stuck()]) == 8.5 and v6.count_cap([stuck()] * 3) == 7.4 and v6.count_cap([stuck()] * 10) == 5.4
    assert v6.count_cap([q(problem="misheard")] * 20) is None  # soft failures are not counted
    assert v6.depth_capped(9.9, "T2") == 5.9 and v6.depth_capped(9.9, "T3") == 8.5 and v6.depth_capped(9.9, None) == 5.9


def test_the_voice_part_is_sixty_percent_reliability_and_forty_speed():
    qs = [q(ms=1000)] * 10
    assert v6.voice(qs) == 10.0
    slow = [q(ms=4000)] * 10  # speed_pts 0.4 each
    assert v6.voice(slow) == round(0.6 * 10 + 0.4 * 4.0, 1)
    assert v6.voice([q(), q()]) is None  # under 3 questions
    only_failures = [q(problem="stuck", answered=False, ms=None)] * 4
    assert v6.voice(only_failures) == round(10 * math.exp(-9 * 1.5), 1)  # no answer times: reliability alone


def test_bands_are_8_and_6():
    assert [v6.band(x) for x in (10, 8, 7.9, 6, 5.9, None)] == ["green", "green", "amber", "amber", "red", "grey"]


def test_the_parts_weigh_ten():
    assert math.isclose(sum(v6.PART_W.values()), 1.0) and v6.PART_W["Voice"] == 0.40 and v6.MIN_Q == 20


def test_every_input_has_a_source_and_none_is_unmarked():
    assert {kind for _, kind, _ in v6.INPUTS} == {"now", "later", "no"}
