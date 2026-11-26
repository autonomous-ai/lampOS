"""The laptop adapter: the log viewer's timing rows into turn records, scored, printed, and a page. Numbers only."""

import datetime as dt
import json

from lamp_doctor import local

DAY = dt.date(2026, 11, 20)


def at(hour, minute=0, day=DAY):
    return int(dt.datetime(day.year, day.month, day.day, hour, minute).timestamp() * 1000)


def row(request, hour, first_audio=1500, day=DAY, **more):
    out = {"request": request, "piece_at": at(hour, request % 60, day), "first_audio_ms": first_audio, "first_piece_ms": 900,
           "tts_first_byte_ms": 400, "tts_chars": 12, "tts_conn": "reused"}
    out.update(more)
    return out


ROWS = [
    row(1, 9, 1200, words=5, stt_last_ms=250, llm_ttft_ms=900, llm_end_ms=1100, prompt_tokens=1000, cached_tokens=900),
    row(2, 9, 2500, words=3, stt_last_ms=300, llm_ttft_ms=1500, llm_end_ms=1700, prompt_tokens=1000, cached_tokens=800),
    row(3, 10, 9000, words=7, stt_last_ms=280, llm_ttft_ms=6000, llm_end_ms=6500, prompt_tokens=1000, cached_tokens=0),
    row(4, 11, None),
]
ROWS[3].pop("first_audio_ms")


def test_the_cache_file_and_the_metrics_list_both_read():
    as_list = local.rows_from(ROWS)
    as_dict = local.rows_from({str(r["piece_at"]): r for r in ROWS})
    assert len(as_list) == len(as_dict) == 4
    assert local.rows_from([{"request": 1}, "x", {"piece_at": "no"}]) == []


def test_rows_become_answered_turn_records_with_numbers_only():
    turns = local.to_turns(ROWS)
    assert [t["turn"] for t in turns] == [1, 2, 3, 4]
    assert all(t["outcome"] == "answered" and t["calls"] == [] for t in turns)
    assert turns[0]["heard_to_first_sound_ms"] == 1200 and turns[3]["heard_to_first_sound_ms"] is None
    assert turns[2]["words_heard"] == 7
    assert not any(isinstance(v, str) and v not in ("answered", "live", "viewer") for t in turns for v in t.values())


def test_only_the_asked_day_is_scored():
    other = row(9, 9, 800, day=DAY + dt.timedelta(days=1))
    assert [r["request"] for r in local.in_day(ROWS + [other], DAY)] == [1, 2, 3, 4]


def test_the_lines_carry_the_day_the_score_and_the_slow_request():
    text = "\n".join(local.report_lines(DAY, ROWS))
    assert "Lamp Doctor, 2026-11-20" in text
    assert "requests: 4 (4 answered, 0 ended by a failed call)" in text
    assert "answered within 5 s: 50.0%" in text  # requests 1 and 2 of 4; request 4 has no first sound
    assert "request 3 (9000 ms)" in text
    assert "slow_first_sound 1" in text
    assert "Voice part:" in text and "too few questions (4 of 20)" in text
    assert "Lamp score:" in text and "= 0.4 x Voice; Skills, Memory, SDK: not proven yet, 0 of 6" in text
    for name in ("Skills", "Memory", "SDK", "Tester score", "Overall (depth cap)"):
        assert f"NOT SCORED YET, {name}" in text
    assert "Overall: not scored, the depth is unknown" in text
    assert "held, stuck and asked-again turns are not seen" in text


def test_stage_medians_and_the_cache_hit_come_from_the_rows():
    names = dict(local.stages(ROWS))
    assert names["last word to text"]["median"] == 280
    assert names["model first token"]["p90"] == 6000
    assert local.cache_hit_pct(ROWS) == 56.7  # (900+800+0) of 3000 prompt tokens


def test_no_rows_say_so_and_do_not_score():
    assert local.report_lines(DAY, []) == ["Lamp Doctor, 2026-11-20: no answered requests in these rows."]
    assert "score" not in local.report_html(DAY, []).split("<ul>")[0]


def test_the_page_holds_the_lines_escaped_and_no_script():
    page = local.report_html(DAY, ROWS)
    assert page.startswith("<!doctype html>") and "<script" not in page
    assert "Lamp Doctor, 2026-11-20" in page and "Lamp score" in page and "too few questions (4 of 20)" in page and "not proven yet, 0 of 6" in page and "NOT SCORED YET, Skills" in page


def test_main_reads_a_file_and_writes_the_page(tmp_path, capsys):
    source = tmp_path / "rows.json"
    source.write_text(json.dumps({str(r["piece_at"]): r for r in ROWS}))
    out = tmp_path / "report.html"
    assert local.main([str(source), "--day", str(DAY), "--html", str(out)]) == 0
    printed = capsys.readouterr().out
    assert "Lamp score:" in printed
    assert "<ul>" in out.read_text()


def test_the_module_opens_no_connection():
    source = open(local.__file__, encoding="utf-8").read()
    imports = [line for line in source.splitlines() if line.startswith(("import ", "from "))]
    assert not any(name in line for line in imports for name in ("socket", "urllib", "http", "requests", "ssl", "websocket"))


def test_the_voice_score_is_v6s_for_known_rows():
    # rows 1 to 3 are answered at 1.2 s, 2.5 s and 9 s: the 9 s one is very slow (weight 0.5 of 3 questions).
    qs, voice = local._view(ROWS[:3])
    reliability = 10 * 2.718281828 ** (-9 * 0.5 / 3)
    speed = 10 * (1.0 + 0.7 + 0.0) / 3
    assert abs(voice - round(0.6 * reliability + 0.4 * speed, 1)) < 0.11


def test_the_lamp_score_is_four_tenths_of_voice_only():
    assert local.lamp_score(9.6) == 3.8 and local.lamp_score(10.0) == 4.0 and local.lamp_score(None) is None
    assert "Lamp score 3.8" not in local.report_html(DAY, [])


def test_cases_go_on_the_lines_and_the_page(tmp_path, capsys):
    import json as _json, datetime as _dt
    from lamp_doctor import local
    micros = int(_dt.datetime(2026, 11, 26, 14, 0, 1).timestamp() * 1_000_000)
    journal = tmp_path / "j.json"
    journal.write_text(_json.dumps({"MESSAGE": "lamp-voice second: request 7 chat round 2 had not begun in 4500 ms; the turn ends unanswered",
                                    "__REALTIME_TIMESTAMP": str(micros)}) + "\n")
    rows = tmp_path / "rows.json"
    rows.write_text("[]")
    page = tmp_path / "p.html"
    assert local.main([str(rows), "--day", "2026-11-26", "--journal", str(journal), "--html", str(page)]) == 0
    out = capsys.readouterr().out
    assert "Cases (failed or timed-out calls): 1" in out and "request 7  LLM chat  timeout" in out
    assert "<h2>Cases (1)</h2>" in page.read_text() and "<td>LLM chat</td>" in page.read_text()


def _case(message, request_at=None):
    from lamp_doctor import cases as _cases
    return _cases.parse_message(message, request_at or dt.datetime(2026, 11, 20, 12, 0, 0))


SIX = [row(n, 9, 1500) for n in range(1, 7)]
ENDS = "lamp-voice second: request 382 chat round 1 had not begun in 3001 ms; the turn ends unanswered"


def test_a_had_not_begun_case_is_a_no_answer_question_that_lowers_the_score():
    from lamp_doctor import v6
    case = _case(ENDS)
    before = local._view(SIX)[1]
    qs, after = local._view(SIX, [case])
    assert len(qs) == 7 and qs[-1]["problem"] == "no_answer" and not qs[-1]["answered"]
    assert after < before
    scored = local.score_day(local.all_turns(SIX, [case]))
    assert scored["turns"] == 7 and scored["failures"]["no_answer"] == 1
    assert scored["answered_within_5s"] == round(100 * 6 / 7, 1)  # the unanswered turn is in the denominator
    lines = "\n".join(local.report_lines(DAY, SIX, [case]))
    assert "failure categories: no_answer 1" in lines and "answered within 5 s: 85.7%" in lines
    assert "6 answered, 1 ended by a failed call" in lines
    assert v6.failure(qs[-1]) == "no_answer"


def test_a_failed_round_or_stt_is_an_error_and_a_fallback_stays_a_case_only():
    failed = _case("lamp-voice second: request 383 chat round 1 failed after 900 ms: service answered HTTP 503")
    stt = _case("lamp-voice second: request 384 speech to text failed: speech-to-text connect failed")
    relay = _case("lamp-voice second: DeepSeek: the person's own key was refused (HTTP 401); this request goes through the relay (LAMP_OWN_KEYS)")
    onboarding = _case("lamp-voice second: request 385 chat round 1 had not begun in 4500 ms; the onboarding line's own words are said")
    look = _case("lamp-voice second: request 386 look round had not begun in 4500 ms; the lamp says it could not look")
    assert [c.problem for c in (failed, stt, relay, onboarding, look)] == ["error", "error", None, None, None]
    qs, _ = local._view(SIX, [failed, stt, relay, onboarding, look])
    assert sorted(q["problem"] for q in qs if q["problem"]) == ["error", "error"] and len(qs) == 8


def test_one_request_counts_once_and_an_answered_request_stays_a_case():
    twice = [_case(ENDS), _case("lamp-voice second: request 382 chat round 1 failed after 900 ms: service answered HTTP 503")]
    assert len(local.all_turns(SIX, twice)) == 7
    answered_too = [row(382, 9, 1500)]
    assert len(local.all_turns(answered_too, [_case(ENDS)])) == 1
