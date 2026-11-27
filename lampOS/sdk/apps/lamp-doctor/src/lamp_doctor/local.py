"""Lamp Doctor on this laptop, on real numbers, before the lamp's grants exist.

The adapter reads the rows of the lead's log viewer (`GET /metrics`, or its
`metrics-cache.json`): one row per answered request with timings and token
counts, never words. It maps them into turn records, runs the same `score_day`
the app runs on a lamp, and prints the day line by line and as a small page.

    curl -s 'http://127.0.0.1:8790/metrics?since=0&until=9999999999999' | python3 -I run_local.py -
    python3 -I run_local.py metrics-cache.json --html report.html

(run from the app's folder, `lampOS/sdk/apps/lamp-doctor`; `run_local.py` puts `src` on the path, which `-I` drops)

It opens no connection: the rows come from a file or from standard input, so
this module imports nothing that talks to a network.

What the rows cannot show: only answered requests reach them, so held,
stuck and asked-again turns are not seen, and no row names a tool call (so no
tier or tool timeout). The report says so.
"""

from __future__ import annotations

import argparse
import datetime as dt
import html
import json
import sys
from typing import Any

from . import v6
from .cases import Case, case_line, read_cases, unanswered_turns
from .score import FAILURES, WITHIN_S, _median, _percentile, _question, score_day

NOTES = (
    "Only answered requests are in these rows: held, stuck and asked-again turns are not seen. With --journal, "
    "a request a failed call ended (the turn ends unanswered, a failed chat round or speech to text) is added "
    "as no_answer or error; without it the score is of the answers the lamp gave.",
    "No row names a tool call or a lamp error, so tool timeouts and lamp errors are 0 here.",
)
NOT_SCORED = (
    ("Skills", "needs each question's capability (tool names) and proof from the lamp's log"),
    ("Memory", "needs each question's capability (tool names) and proof from the lamp's log"),
    ("SDK", "needs each question's capability and proof from the lamp's log"),
    ("Tester score", "needs the tiers and follow-ups, from the words (later, via the lamp's model) or the tool names"),
    ("Overall (depth cap)", "the cap is not applied: it needs the deepest tier asked"),
)


def rows_from(data: Any) -> list[dict[str, Any]]:
    """The viewer's `/metrics` list, or its cache file (a dict keyed by time)."""
    rows = list(data.values()) if isinstance(data, dict) else list(data)
    return [r for r in rows if isinstance(r, dict) and isinstance(r.get("piece_at"), (int, float))]


def in_day(rows: list[dict[str, Any]], day: dt.date) -> list[dict[str, Any]]:
    return [r for r in rows if dt.datetime.fromtimestamp(r["piece_at"] / 1000).date() == day]


def to_turns(rows: list[dict[str, Any]], route: str = "live", build: str = "viewer") -> list[dict[str, Any]]:
    """Each row becomes a turn record. Only numbers are read."""
    turns = []
    for row in sorted(rows, key=lambda r: r["piece_at"]):
        first = row.get("first_audio_ms")
        turns.append({
            "turn": row.get("request", 0),
            "at": int(row["piece_at"] // 60000),
            "route": route,
            "build": build,
            "outcome": "answered",
            "heard_to_first_sound_ms": first if isinstance(first, (int, float)) else None,
            "cut_ins": 0,
            "calls": [],
            "words_heard": row.get("words", 0) or 0,
            "words_said": 0,
            "heard_wrongly": False,
            "name_missed": False,
            "held_then_repeated": False,
        })
    return turns


def _stage(rows: list[dict[str, Any]], key: str) -> dict[str, int] | None:
    values = [r[key] for r in rows if isinstance(r.get(key), (int, float))]
    if not values:
        return None
    return {"n": len(values), "median": _median([round(v) for v in values]), "p90": _percentile([round(v) for v in values], 0.9)}


STAGES = (
    ("stt_last_ms", "last word to text"),
    ("llm_ttft_ms", "model first token"),
    ("llm_end_ms", "model end"),
    ("first_piece_ms", "to the voice"),
    ("tts_first_byte_ms", "voice first byte"),
    ("first_audio_ms", "heard to first sound"),
)


def stages(rows: list[dict[str, Any]]) -> list[tuple[str, dict[str, int]]]:
    return [(label, s) for key, label in STAGES if (s := _stage(rows, key))]


def cache_hit_pct(rows: list[dict[str, Any]]) -> float | None:
    both = [(r["cached_tokens"], r["prompt_tokens"]) for r in rows if r.get("prompt_tokens") and "cached_tokens" in r]
    total = sum(p for _, p in both)
    return round(100 * sum(c for c, _ in both) / total, 1) if total else None


def slow_requests(rows: list[dict[str, Any]]) -> list[tuple[int, int]]:
    slow = [(r.get("request", 0), round(r["first_audio_ms"])) for r in rows
            if isinstance(r.get("first_audio_ms"), (int, float)) and r["first_audio_ms"] > WITHIN_S]
    return sorted(slow, key=lambda x: -x[1])


def lamp_score(voice: float | None) -> float | None:
    """v6 with no proof for Skills, Memory and SDK: only Voice's 40 % can be earned, so 4.0 at most."""
    return None if voice is None else round(v6.PART_W["Voice"] * voice, 1)


def all_turns(rows: list[dict[str, Any]], cases: list[Case] | None = None) -> list[dict[str, Any]]:
    """The answered rows plus one unanswered turn for each request a case ended (no answer, or an error)."""
    answered = {r.get("request") for r in rows}
    return to_turns(rows) + unanswered_turns(cases or [], answered)


def _view(rows: list[dict[str, Any]], cases: list[Case] | None = None):
    qs = [_question(t) for t in all_turns(rows, cases)]
    return qs, v6.voice(qs)


def cases_lines(cases: list[Case] | None) -> list[str]:
    if cases is None:
        return []
    if not cases:
        return ["Cases (failed or timed-out calls): none in this journal"]
    return [f"Cases (failed or timed-out calls): {len(cases)}"] + [f"  {case_line(c)}" for c in cases]


def report_lines(day: dt.date, rows: list[dict[str, Any]], cases: list[Case] | None = None) -> list[str]:
    if not rows:
        return [f"Lamp Doctor, {day}: no answered requests in these rows."] + cases_lines(cases)
    turns = all_turns(rows, cases)
    scored = score_day(turns)
    qs, voice = _view(rows, cases)
    sp, _ = v6.speed(qs)
    lamp = lamp_score(voice)
    few = scored["few"]
    lines = [
        f"Lamp Doctor, {day}  (score v6, 0 to 10: 8 and up good, 6 to 8 OK, under 6 poor)",
        f"requests: {scored['turns']} ({len(rows)} answered, {scored['turns'] - len(rows)} ended by a failed call)",
        (f"too few questions ({scored['turns']} of {v6.MIN_Q}): v6 gives no score on the page below {v6.MIN_Q}; shown for reading only" if few
         else f"questions: {scored['turns']} (enough: {v6.MIN_Q} needed)"),
        f"Lamp score: {lamp} / 10  = 0.4 x Voice; Skills, Memory, SDK: not proven yet, 0 of 6" if lamp is not None
        else "Lamp score: needs 3 questions",
        f"Voice part: {voice} / 10, {v6.band(voice)}  (40 % of the lamp score, 4 points)" if voice is not None else "Voice part: needs 3 questions",
        f"  reliability {round(v6.reliability(qs), 1)} / 10 (60 %), speed {round(sp, 1)} / 10 (40 %)",
        f"answered within 5 s: {scored['answered_within_5s']}%",
        f"heard to first sound: median {scored['heard_to_first_sound_ms_median']} ms, p90 {scored['heard_to_first_sound_ms_p90']} ms",
    ]
    for label, st in stages(rows):
        if label != "heard to first sound":
            lines.append(f"{label}: median {st['median']} ms, p90 {st['p90']} ms ({st['n']} requests)")
    if (hit := cache_hit_pct(rows)) is not None:
        lines.append(f"prompt cache hit: {hit}%")
    if slow := slow_requests(rows):
        lines.append("slower than 5 s: " + ", ".join(f"request {n} ({ms} ms)" for n, ms in slow[:5]))
    failures = scored["failures"]
    lines.append("failure categories: " + (", ".join(f"{k} {failures[k]}" for k in FAILURES if k in failures) or "none counted"))
    lines.extend(cases_lines(cases))
    lines.extend(f"NOT SCORED YET, {name}: {why}" for name, why in NOT_SCORED)
    lines.append("Overall: not scored, the depth is unknown (v6 caps it at 5.9 while only simple questions are known)")
    lines.extend(f"note: {n}" for n in NOTES)
    return lines


def cases_table(cases: list[Case] | None) -> str:
    if cases is None:
        return "<h2>Cases</h2><p class=\"sub\">No journal given.</p>"
    if not cases:
        return "<h2>Cases</h2><p class=\"sub\">No failed or timed-out calls in this journal.</p>"
    head = "".join(f"<th>{h}</th>" for h in ("when", "request", "call", "outcome", "reason", "ms", "source"))
    rows = "".join(
        "<tr>" + "".join(f"<td>{html.escape(str(v))}</td>" for v in (
            c.at.strftime("%H:%M:%S") if c.at else "", "" if c.request is None else c.request, c.call, c.outcome,
            c.reason, "" if c.ms is None else c.ms, c.source)) + "</tr>"
        for c in cases)
    return f"<h2>Cases ({len(cases)})</h2><div class=\"scroll\"><table><tr>{head}</tr>{rows}</table></div>"


def report_html(day: dt.date, rows: list[dict[str, Any]], cases: list[Case] | None = None) -> str:
    lines = [l for l in report_lines(day, rows, cases) if not l.startswith(("Cases", "  "))]
    voice = _view(rows, cases)[1] if rows else None
    lamp = lamp_score(voice)
    total = len(all_turns(rows, cases))
    few = bool(rows) and total < v6.MIN_Q
    body = "".join(f"<li class=\"{'no' if line.startswith(('NOT SCORED', 'Overall')) else ''}\">{html.escape(line)}</li>" for line in lines)
    head = ""
    if lamp is not None:
        grey = "grey" if few else v6.band(lamp)
        head = (f'<div class="bar"><div class="fill {grey}" style="width:{lamp * 10:.0f}%"></div></div>'
                f'<p class="big">Lamp score {lamp} / 10</p>'
                f'<p class="sub">Voice part {voice} / 10 (40 %). Skills, Memory, SDK: not proven yet, 0 of 6.</p>'
                + (f'<p class="sub few">too few questions ({total} of {v6.MIN_Q})</p>' if few else ""))
    return f"""<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<meta name="color-scheme" content="light dark"><title>Lamp Doctor {html.escape(str(day))}</title>
<style>
:root{{--bg:#fff;--fg:#1a1a1a;--mut:#666;--track:#e5e5e5;--green:#2f7d4f;--amber:#b7791f;--red:#c0392b;--grey:#8a8a8a}}
@media (prefers-color-scheme:dark){{:root{{--bg:#111;--fg:#eee;--mut:#999;--track:#333;--green:#58b27a;--amber:#e0a040;--red:#e5604f;--grey:#8a8a8a}}}}
body{{background:var(--bg);color:var(--fg);font:16px/1.5 system-ui,sans-serif;margin:0;padding:16px;max-width:42rem}}
.bar{{background:var(--track);border-radius:6px;height:14px;overflow:hidden}}.fill{{height:100%}}
.green{{background:var(--green)}}.amber{{background:var(--amber)}}.red{{background:var(--red)}}.grey{{background:var(--grey)}}
.big{{font-size:2rem;margin:.4rem 0 0}}.sub{{margin:.1rem 0;color:var(--mut)}}.few{{font-weight:600}}
ul{{padding-left:1.1rem;margin-top:1.2rem}}li{{margin:.2rem 0}}li.no{{color:var(--mut)}}
table{{border-collapse:collapse;font-size:.85rem}}td,th{{border-bottom:1px solid var(--track);padding:.25rem .5rem;text-align:left;white-space:nowrap}}.scroll{{overflow-x:auto}}
</style></head><body><h1>Lamp Doctor</h1>{head}{cases_table(cases)}<ul>{body}</ul></body></html>
"""


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="lamp_doctor.local", description="Score a day from the log viewer's metrics rows.")
    parser.add_argument("source", help="a metrics JSON file, or - for standard input")
    parser.add_argument("--day", help="YYYY-MM-DD in this computer's time zone (default: today)")
    parser.add_argument("--journal", help="journalctl -o json lines (a file, or - for standard input) to list failed calls as cases")
    parser.add_argument("--html", help="also write the report as a small page here")
    args = parser.parse_args(argv)
    if args.source == "-" and args.journal == "-":
        parser.error("only one of source and --journal can be standard input")
    raw = sys.stdin.read() if args.source == "-" else open(args.source, encoding="utf-8").read()
    day = dt.date.fromisoformat(args.day) if args.day else dt.date.today()
    cases = None
    if args.journal:
        text = sys.stdin.read() if args.journal == "-" else open(args.journal, encoding="utf-8").read()
        cases = [c for c in read_cases(text.splitlines()) if c.at is None or c.at.date() == day]
    rows = in_day(rows_from(json.loads(raw)), day)
    print("\n".join(report_lines(day, rows, cases)))
    if args.html:
        with open(args.html, "w", encoding="utf-8") as out:
            out.write(report_html(day, rows, cases))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
