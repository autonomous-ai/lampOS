"""Store v1: name.heard to lamp.said, no answer, cut short, the fixed share, the device bars, the score, the panel,
and the app on the SDK's fake lamp. Numbers only: a said line's text is never kept."""

import asyncio
import datetime as dt

import pytest

from lamp_doctor import v6, watch
from lamp_doctor.watch import Day

T0 = dt.datetime(2026, 11, 26, 15, 0, 0)


class Clock:
    def __init__(self):
        self.t = 0.0

    def now(self):
        return self.t

    def wall(self):
        return T0 + dt.timedelta(seconds=self.t)

    def at(self, t):
        self.t = t


def day():
    c = Clock()
    return Day(clock=c.wall, now=c.now), c


def test_the_answer_time_is_the_name_to_the_next_line():
    d, c = day()
    c.at(0), d.name_heard()
    c.at(2.5), d.lamp_said("reply")
    c.at(10), d.name_heard()
    c.at(11), d.lamp_said("exact")
    s = d.summary()
    assert d.state["times_ms"] == [2500, 1000] and s["median_ms"] == 1750 and s["p90_ms"] == 2500
    assert d.state["names"] == 2 and d.state["answered"] == 2 and s["no_answer"] == 0


def test_a_name_again_while_one_waits_is_the_same_call():
    d, c = day()
    c.at(0), d.name_heard()
    c.at(1), d.name_heard()
    c.at(3), d.lamp_said("reply")
    assert d.state["names"] == 1 and d.state["times_ms"] == [3000]


def test_a_name_with_no_line_in_10_s_is_a_no_answer_case_with_its_time():
    d, c = day()
    c.at(0), d.name_heard()
    c.at(9.9), d.expire()
    assert d.state["no_answer"] == 0
    c.at(10.1), d.expire()
    assert d.state["no_answer"] == 1 and d.state["cases"][-1] == {"at": "15:00", "what": "no answer: nothing said within 10 s of the name"}
    c.at(20), d.lamp_said("reply")  # a line long after is not its answer
    assert d.state["answered"] == 0 and d.state["no_answer"] == 1


def test_a_line_after_the_window_does_not_answer_the_old_name():
    d, c = day()
    c.at(0), d.name_heard()
    c.at(12), d.lamp_said("reply")
    assert d.state["no_answer"] == 1 and d.state["answered"] == 0


def test_a_cut_line_is_a_case_and_text_is_never_kept():
    d, c = day()
    c.at(0), d.lamp_said("reply", cut=True)
    assert d.state["cut"] == 1 and d.state["cases"][-1]["what"] == "answer cut short (reply)"
    assert "text" not in repr(d.state)


def test_the_share_of_cached_and_exact_lines():
    d, c = day()
    for source in ("cached", "exact", "reply", "reply", "cached"):
        d.lamp_said(source)
    assert d.summary()["fixed_share"] == 60 and d.summary()["said"] == 5


def test_device_over_a_bar_is_one_case_until_it_is_back_under():
    d, c = day()
    ok = {"cpu_load": 0.3, "memory_used": 0.5, "disk_free": 0.5, "temperature_c": {"cpu": 55.0}, "known": True}
    hot = {**ok, "cpu_load": 2.0, "temperature_c": {"cpu": 95.0}, "disk_free": 0.05, "memory_used": 0.95}
    d.device(ok)
    assert d.state["cases"] == []
    d.device(hot), d.device(hot)
    whats = [x["what"] for x in d.state["cases"]]
    assert sorted(whats) == ["device: cpu 95 C over 90", "device: disk 95% over 90%", "device: load 2.00 over 1.5",
                             "device: memory 95% over 90%"]
    d.device(ok), d.device(hot)
    assert len(d.state["cases"]) == 8
    d.device({"known": False}), d.device({})
    assert len(d.state["cases"]) == 8


def test_the_score_is_v6_voice_on_answers_and_no_answers():
    d, c = day()
    for i in range(6):
        c.at(i * 20), d.name_heard(), c.at(i * 20 + 1.5), d.lamp_said("reply")
    assert d.summary()["voice"] == v6.voice([{"ms": 1500, "answered": True, "problem": None}] * 6)
    c.at(200), d.name_heard(), c.at(215), d.expire()
    s = d.summary()
    assert s["questions"] == 7 and s["voice"] < 9.6 and s["few"] and s["band"] == "grey"
    assert s["lamp"] == round(0.4 * s["voice"], 1) and s["within_5s"] == round(100 * 6 / 7, 1)


def test_under_three_questions_voice_is_not_scored():
    d, c = day()
    c.at(0), d.name_heard(), c.at(1), d.lamp_said("reply")
    title, lines = watch.panel(d)
    assert title == "Lamp Doctor" and lines[0].startswith("Voice not scored yet: 1 of 3")


def test_the_panel_fits_6_lines_of_80_with_the_2_worst_hardware_lines_and_the_last_2_cases():
    d, c = day()
    for i in range(5):
        c.at(i * 100), d.name_heard(), c.at(i * 100 + 20), d.expire()
    for i in range(4):
        c.at(600 + i), d.lamp_said("reply", cut=True)
    for i in range(5):
        c.at(700 + i), d.lamp_said("cached")
    d.device({"cpu_load": 5.4, "memory_used": 0.61, "disk_free": 0.5, "uptime_s": 99, "known": True,
              "temperature_c": {"cpu_thermal": 95.0, "gpu_thermal": 60.0}})
    title, lines = watch.panel(d)
    assert len(title) <= 40 and len(lines) == 6 and all(1 <= len(x) <= 80 and x.isprintable() for x in lines)
    assert lines[2] == "not sent yet: load 5.4, 5.4-5.4, avg 5.4 (over 1.5)"
    assert lines[3] == "cpu_thermal 95 C, 95-95, avg 95 (warm over 70 C, hot over 90 C)"
    assert lines[4].startswith("15:11 device: ") and lines[5].startswith("15:11 device: ")  # the last 2 cases
    assert "no answer 5; cut 4; cached/exact 56%" in lines[1]


def test_the_hardware_keeps_min_average_max_and_last_in_fixed_keys():
    d, c = day()
    for load, temp in ((1.0, 50.0), (3.0, 90.0), (2.0, 70.0)):
        d.device({"cpu_load": load, "memory_used": 0.5, "disk_free": 0.7, "uptime_s": 10, "known": True,
                  "temperature_c": {"cpu_thermal": temp}})
    hw = d.state["hw"]
    assert hw["load"] == {"n": 3, "min": 1.0, "avg": 2.0, "max": 3.0, "last": 2.0}
    assert hw["temp:cpu_thermal"] == {"n": 3, "min": 50.0, "avg": 70.0, "max": 90.0, "last": 70.0}
    assert hw["disk"]["last"] == 0.3 and hw["memory"]["last"] == 0.5 and hw["uptime_s"] == {"last": 10}
    assert set(hw) == {"load", "memory", "disk", "uptime_s", "temp:cpu_thermal"}


def test_the_note_stays_small_after_a_day_of_samples_and_many_zones():
    import json
    d, c = day()
    for i in range(1440):
        c.at(i * 60)
        d.device({"cpu_load": i % 7, "memory_used": 0.4, "disk_free": 0.6, "uptime_s": i, "known": True,
                  "temperature_c": {f"zone{z}": 40.0 + z for z in range(12)}})
    assert len(d.state["hw"]) <= 3 + 1 + watch.KEEP_ZONES
    assert len(json.dumps(d.state)) < 4000 and d.state["hw"]["load"]["n"] == 1440


def test_a_reading_with_no_numbers_adds_nothing():
    d, c = day()
    d.device({"known": False}), d.device({"cpu_load": "x", "temperature_c": {"a": None}, "known": True})
    assert d.state["hw"] == {}


def test_the_day_rolls_at_midnight():
    d, c = day()
    c.at(0), d.lamp_said("reply", cut=True)
    c.at(24 * 3600), d.name_heard()
    assert d.state["day"] == "2026-11-27" and d.state["cut"] == 0 and d.state["names"] == 1


# On the SDK's fake lamp, as the app runs it.

from lampos import Lamp
from lampos.testing import FakeLamp

GRANTS = ("hear", "speech", "device", "notes")


def arun(coroutine):
    return asyncio.run(asyncio.wait_for(coroutine, 30))


def test_the_app_counts_events_from_the_fake_lamp_and_writes_panel_and_notes():
    async def go():
        async with FakeLamp(app=True, app_name="lamp-doctor", app_grants=GRANTS) as fake:
            async with await Lamp.connect_app(path=fake.app_path) as lamp:
                d = Day()
                task = asyncio.create_task(watch.run(lamp, d, poll_s=0.2, tick_s=0.05, rounds=12, sleep=asyncio.sleep))
                await asyncio.sleep(0.05)
                await fake.emit("name.heard", score=0.95)
                await asyncio.sleep(0.05)
                await fake.emit("lamp.said", text="hello Anna", by="lamp", source="reply", cut=False)
                await fake.emit("lamp.said", text="stop", by="lamp", source="cached", cut=True)
                await task
                assert d.state["names"] == 1 and d.state["answered"] == 1 and d.state["cut"] == 1
                assert fake.app_panel["title"].startswith("Lamp Doctor")
                assert all(len(x) <= 80 for x in fake.app_panel["lines"]) and len(fake.app_panel["lines"]) <= 6
                note = fake.notes[next(iter(fake.notes))]["day"]
                assert note["answered"] == 1 and "Anna" not in repr(note) and "Anna" not in repr(fake.app_panel)

    arun(go())


def test_a_second_start_reads_the_day_back_from_notes():
    async def go():
        async with FakeLamp(app=True, app_name="lamp-doctor", app_grants=GRANTS) as fake:
            today = str(dt.datetime.now().date())
            async with await Lamp.connect_app(path=fake.app_path) as lamp:
                await lamp.notes.set("day", {**watch._day_empty(today), "cut": 7})
                d = Day()
                await watch.run(lamp, d, poll_s=10, tick_s=0.01, rounds=1)
                assert d.state["cut"] == 7

    arun(go())


def test_a_warm_zone_is_on_the_panel_only_and_a_hot_one_is_a_case():
    d, c = day()
    base = {"cpu_load": 0.2, "memory_used": 0.4, "disk_free": 0.6, "known": True}
    d.device({**base, "temperature_c": {"cpu_thermal": 80.0}})
    assert d.state["cases"] == []  # warm (over 70): the panel's, not a case
    assert any("cpu_thermal 80 C" in line and "warm over 70 C, hot over 90 C" in line for line in watch.panel(d)[1])
    d.device({**base, "temperature_c": {"cpu_thermal": 90.0}})
    assert d.state["cases"] == []  # at the bar is not over it
    d.device({**base, "temperature_c": {"cpu_thermal": 91.0}})
    assert [x["what"] for x in d.state["cases"]] == ["device: cpu_thermal 91 C over 90"]
