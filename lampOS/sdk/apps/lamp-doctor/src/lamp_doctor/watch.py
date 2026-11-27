"""Lamp Doctor, Store v1: the day's answer times and failed calls, from events the lamp already gives an app.

Grants `hear` (`name.heard`), `speech` (`lamp.said`: its `source` and `cut`, never its `text`, which is dropped on
arrival), `device` (`get device`) and `notes`; the panel is `app.panel.set`. No `net`, no new grant.

Measures, all numbers:
  1. `name.heard` to the next `lamp.said`: the answer time after the name (median, p90).
  2. A `name.heard` with no `lamp.said` within 10 s: no answer, a case.
  3. A `lamp.said` with `cut` true: an answer cut short, a case.
  4. The share of `cached` and `exact` lines among all lines said, against `reply`.
  5. The lamp's own computer: load, memory, disk, temperatures; a case past a bar, each told as "over X".

The score is v6's Voice part on (1) and (2) (`v6.py`); Skills, Memory and SDK are "not scored yet".
"""

from __future__ import annotations

import asyncio
import datetime as dt
from collections import Counter
from typing import Any, Callable

from . import v6
from .score import _median, _percentile

ANSWER_WITHIN_S = 10.0  # a name with no line said in this long got no answer
KEEP_TIMES = 400        # answer times kept in the day's note (a note is 16 KiB at most)
KEEP_CASES = 20
KEEP_ZONES = 8  # temperature zones kept in the day's note, by name (the lamp has a few)
SOURCES = ("reply", "exact", "cached")

# The device bars, each shown on the panel as "over X" so they can be tuned. They are the lamp's own health
# bars (docs/health.md): load is per processor, memory used and disk used are shares, a zone's temperature is
# in degrees. The temperature bar is to be read against the lamp's normal.
BARS = {"load": 1.5, "memory": 0.90, "disk": 0.90, "temp_c": 90.0}  # temp_c: HOT, 75c9's trip_point_1 (second cooling step)
WARM_C = 70.0  # trip_point_0: cooling starts. On the panel only, never a case. trip_point_2 is critical at 110.


def _day_empty(day: str) -> dict[str, Any]:
    return {"day": day, "names": 0, "answered": 0, "no_answer": 0, "cut": 0, "said": {s: 0 for s in SOURCES},
            "times_ms": [], "cases": [], "over": [], "hw": {}}


class Day:
    """One day's counts, with the clock handed in: `now` is monotonic seconds, `clock` the wall time of a case."""

    def __init__(self, clock: Callable[[], dt.datetime] = dt.datetime.now, now: Callable[[], float] | None = None,
                 state: dict[str, Any] | None = None) -> None:
        import time
        self.clock = clock
        self.now = now or time.monotonic
        self.state = state or _day_empty(str(clock().date()))
        self.waiting: float | None = None  # when the name was heard, if no line has followed yet

    # Events.

    def name_heard(self) -> None:
        self.roll()
        self.expire()
        if self.waiting is None:  # a name again while one waits is the same call
            self.waiting = self.now()
            self.state["names"] += 1

    def lamp_said(self, source: str | None, cut: bool = False) -> None:
        self.roll()
        self.expire()
        if source in SOURCES:
            self.state["said"][source] += 1
        if cut:
            self.state["cut"] += 1
            self._case("answer cut short" + (f" ({source})" if source in SOURCES else ""))
        if self.waiting is not None:
            ms = round((self.now() - self.waiting) * 1000)
            self.waiting = None
            self.state["answered"] += 1
            self.state["times_ms"] = (self.state["times_ms"] + [ms])[-KEEP_TIMES:]

    def expire(self) -> None:
        """A name with no line within the window is a no-answer case; called on each event and each tick."""
        if self.waiting is not None and self.now() - self.waiting > ANSWER_WITHIN_S:
            self.waiting = None
            self.state["no_answer"] += 1
            self._case(f"no answer: nothing said within {ANSWER_WITHIN_S:g} s of the name")

    def device(self, reading: dict[str, Any]) -> None:
        """One `get device` reading. A bar crossed is one case, told again only after it was back under."""
        over: dict[str, str] = {}
        if not reading or reading.get("known") is False:
            return
        load, memory, free = reading.get("cpu_load"), reading.get("memory_used"), reading.get("disk_free")
        if isinstance(load, (int, float)) and load > BARS["load"]:
            over["load"] = f"load {load:.2f} over {BARS['load']:g}"
        if isinstance(memory, (int, float)) and memory > BARS["memory"]:
            over["memory"] = f"memory {memory * 100:.0f}% over {BARS['memory'] * 100:.0f}%"
        if isinstance(free, (int, float)) and (1 - free) > BARS["disk"]:
            over["disk"] = f"disk {(1 - free) * 100:.0f}% over {BARS['disk'] * 100:.0f}%"
        temps = reading.get("temperature_c")
        for zone, value in sorted(temps.items()) if isinstance(temps, dict) else []:
            if isinstance(value, (int, float)) and value > BARS["temp_c"] and isinstance(zone, str):
                over[f"temp:{zone[:24]}"] = f"{zone[:24]} {value:.0f} C over {BARS['temp_c']:g}"
        self._hardware(reading)
        before = set(self.state["over"])
        for key, why in over.items():
            if key not in before:
                self._case(f"device: {why}")
        self.state["over"] = sorted(over)
        self.last_device = reading

    last_device: dict[str, Any] | None = None

    def _hardware(self, reading: dict[str, Any]) -> None:
        """Each number's min, average, max and last for the day: fixed keys, no per-sample list."""
        hw = self.state.setdefault("hw", {})

        def add(key: str, value: Any, ndigits: int = 2) -> None:
            if not isinstance(value, (int, float)) or isinstance(value, bool):
                return
            if key not in hw and len([k for k in hw if k.startswith("temp:")]) >= KEEP_ZONES and key.startswith("temp:"):
                return
            h = hw.get(key)
            if h is None:
                hw[key] = {"n": 1, "min": round(value, ndigits), "avg": round(value, ndigits), "max": round(value, ndigits), "last": round(value, ndigits)}
                return
            h["n"] += 1
            h["avg"] = round(h["avg"] + (value - h["avg"]) / h["n"], ndigits)
            h["min"], h["max"], h["last"] = min(h["min"], round(value, ndigits)), max(h["max"], round(value, ndigits)), round(value, ndigits)

        add("load", reading.get("cpu_load"))
        add("memory", reading.get("memory_used"), 3)
        free = reading.get("disk_free")
        add("disk", None if not isinstance(free, (int, float)) else 1 - free, 3)
        up = reading.get("uptime_s")
        if isinstance(up, (int, float)) and not isinstance(up, bool):
            hw["uptime_s"] = {"last": int(up)}
        temps = reading.get("temperature_c")
        for zone, value in sorted(temps.items()) if isinstance(temps, dict) else []:
            if isinstance(zone, str):
                add(f"temp:{zone[:24]}", value, 1)

    # The day.

    def roll(self) -> None:
        today = str(self.clock().date())
        if self.state["day"] != today:
            self.state = _day_empty(today)
            self.waiting = None

    def _case(self, what: str) -> None:
        stamp = self.clock().strftime("%H:%M")
        self.state["cases"] = (self.state["cases"] + [{"at": stamp, "what": what}])[-KEEP_CASES:]

    # What the day came to.

    def questions(self) -> list[dict[str, Any]]:
        """v6's questions: each name answered (its time), each unanswered (a no_answer)."""
        done = [{"ms": ms, "answered": True, "problem": None} for ms in self.state["times_ms"]]
        # `answered` may exceed the kept times once the day is long: the oldest times are dropped, the count is not.
        missing = max(0, self.state["answered"] - len(done))
        done += [{"ms": None, "answered": True, "problem": None}] * missing
        return done + [{"ms": None, "answered": False, "problem": "no_answer"}] * self.state["no_answer"]

    def summary(self) -> dict[str, Any]:
        s = self.state
        qs = self.questions()
        voice = v6.voice(qs)
        n = len(qs)
        times = s["times_ms"]
        said = sum(s["said"].values())
        fixed = s["said"]["cached"] + s["said"]["exact"]
        return {
            "questions": n,
            "voice": voice,
            "lamp": None if voice is None else round(0.4 * voice, 1),
            "band": "grey" if n < v6.MIN_Q else v6.band(voice),
            "few": n < v6.MIN_Q,
            "median_ms": _median(times),
            "p90_ms": _percentile(times, 0.9),
            "within_5s": round(100 * sum(1 for t in times if t <= 5000) / n, 1) if n else None,
            "no_answer": s["no_answer"], "cut": s["cut"], "said": said,
            "fixed_share": round(100 * fixed / said) if said else None,
        }


def _line(text: str) -> str:
    return text[:80]


def _hardware_lines(day: Day) -> list[str]:
    """The worst hardware numbers of the day by how far the day's max went past its bar, most first."""
    rows = []
    for key, h in day.state.get("hw", {}).items():
        if "max" not in h:
            continue
        if key == "load":
            name, bar, scale, unit, f = "load", BARS["load"], 1, "", ".1f"
        elif key == "memory":
            name, bar, scale, unit, f = "memory", BARS["memory"], 100, "%", ".0f"
        elif key == "disk":
            name, bar, scale, unit, f = "disk", BARS["disk"], 100, "%", ".0f"
        else:
            name, bar, scale, unit, f = key[5:], BARS["temp_c"], 1, " C", ".0f"
        top = f"over {bar * scale:g}{unit}" if not key.startswith("temp:") else f"warm over {WARM_C:g} C, hot over {BARS['temp_c']:g} C"
        rows.append((h["max"] / bar, f"{name} {h['last'] * scale:{f}}{unit}, {h['min'] * scale:{f}}-{h['max'] * scale:{f}}, avg {h['avg'] * scale:{f}} ({top})"))
    return [line for _, line in sorted(rows, reverse=True)]


def panel(day: Day) -> tuple[str, list[str]]:
    """The panel: a title and at most 6 lines of 80. The score and the counts, the 2 worst hardware numbers
    (last, day's min-max, average; not sent yet: v1 has no net), and the last 2 cases (the note keeps 20)."""
    s = day.summary()
    if s["voice"] is None:
        first = f"Voice not scored yet: {s['questions']} of 3 questions needed"
    else:
        few = f"; too few ({s['questions']} of {v6.MIN_Q})" if s["few"] else ""
        first = f"Score {s['lamp']} / 10 (Voice {s['voice']} x 0.4, rest not scored yet){few}"
    answer = "after the name: " + (
        f"median {s['median_ms'] / 1000:.1f} s, p90 {s['p90_ms'] / 1000:.1f} s" if s["median_ms"] is not None else "no time yet")
    share = f"; cached/exact {s['fixed_share']}%" if s["fixed_share"] is not None else ""
    second = f"{answer}; no answer {s['no_answer']}; cut {s['cut']}{share}"
    hardware = _hardware_lines(day)[:2] or ["no reading yet"]
    hardware[0] = "not sent yet: " + hardware[0]  # v1 has no net: the day's numbers stay on the lamp
    cases = [f"{c['at']} {c['what']}" for c in reversed(day.state["cases"][-2:])]
    title = "Lamp Doctor" + (f" {s['lamp']} / 10" if s["lamp"] is not None else "")
    return title[:40], [_line(x) for x in (first, second, *hardware, *cases)][:6]


NOTE_KEY = "day"


async def run(lamp: Any, day: Day, *, poll_s: float = 60.0, panel_s: float = 60.0, tick_s: float = 1.0,
              sleep: Callable[[float], Any] = asyncio.sleep, rounds: int | None = None) -> None:
    """Subscribe, then each tick expire a waiting name; each minute read the device, write the panel and the day's
    note. `rounds` bounds the loop for a test; the app runs it forever."""
    saved = await lamp.notes.get(NOTE_KEY)
    if isinstance(saved, dict) and saved.get("day") == str(day.clock().date()):
        day.state = {**_day_empty(saved["day"]), **saved}

    lamp.on("name.heard", lambda event: day.name_heard())
    lamp.on("lamp.said", lambda event: day.lamp_said(event.get("source"), bool(event.get("cut"))))  # never `text`

    elapsed, done = poll_s, 0  # read the device and write at once on the first round
    while rounds is None or done < rounds:
        day.expire()
        if elapsed >= poll_s:
            elapsed = 0.0
            try:
                day.device((await lamp.get("device")).get("device", {}))
            except Exception:
                pass  # a lamp with no reading or no grant: the panel says nothing of it
            await _write(lamp, day)
        await sleep(tick_s)
        elapsed += tick_s
        done += 1


async def _write(lamp: Any, day: Day) -> None:
    title, lines = panel(day)
    for write in (lambda: lamp.panel(title, lines), lambda: lamp.notes.set(NOTE_KEY, day.state)):
        try:
            await write()
        except Exception:
            pass  # one panel a minute, thirty notes a minute: a refusal is tried again next minute
