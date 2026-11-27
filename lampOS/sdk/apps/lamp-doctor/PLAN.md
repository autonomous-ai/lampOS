# Lamp Doctor: plan (#8726)

An SDK app that scores how the lamp's conversations go, on the lamp, and
sends only a fixed schema through `log.send`. It holds `turns`,
`conversation`, `model.ask` and `log.send`, and not `net`. English only.

## What the app does, once a day

1. `turns`: read the day's finished-turn records (no words).
2. `score`: from those records alone, compute the numbers, the failure
   categories, the tier (T1 to T4) and a 0 to 100 score. No model, no text.
3. `model.ask` (at most once; the lamp caps it at one a day): with the
   numbers, and with the day's heard words only if `conversation` is held,
   ask the lamp's model which of the **fixed failure categories** it sees.
4. `diagnose`: keep only the categories the answer names. Everything else in
   the answer is dropped, so free text cannot reach step 5.
5. `log.send`: one report of the fixed schema. The lamp refuses any string
   outside the listed categories, so this is checked twice: here, and by the
   lamp.

## Tests first (all in `tests/`, failing until the code exists)

- `test_score.py`: the score from a set of turn records.
- `test_report.py`: the report holds only the fixed fields.
- `test_diagnose.py`: a diagnosis with free text, a heard sentence and a
  name never reaches `log.send`; `model.ask` is called once; if the model
  fails the report still goes without a diagnosis.
- `test_app.py`: the whole day against a fake of Ten's ops; a strict fake
  `log.send` refuses anything outside the schema, and the app never trips it.

## Shapes (Ten's design on #8726, 12:52)

`tests/fake_ops.py` holds them in one place; it is the app's only copy of
lamp-control's side until Ten's ops exist.

- **Grants** (an app's socket only): `turns`, `conversation`, `model`
  (`model.ask`), `report` (`log.send`). Never `net` with `conversation`.
- **`turns`** (`get turns`, event `turns.finished`): one record per turn:
  `turn`, `at` (unix minute), `heard_to_first_sound_ms`, `cut_ins`,
  `calls` = `[{name, ms, timed_out}]` (the lamp's own tool names),
  `outcome` (`answered|stuck|refused|held|asked_again`), `words_heard`,
  `words_said`, `heard_wrongly`, `name_missed`, `held_then_repeated`,
  `route`, `build`. No text, no speaker.
- **`model.ask`**: `{prompt, input}` to the lamp's own provider; 1 call a
  day, prompt at most 2 KiB, input at most 8 KiB, answer at most 1,000
  characters; over the cap it is refused `rate_limited`. Without
  `conversation`, `input` may hold only the `turns` numbers.
- **`log.send`**: the report below, parsed into a closed type; a string
  outside the lists is refused whole (`bad_args`); at most once an hour.
  The lamp fills `build` itself, so the app sends none.
- **The report:** `turns`, `answered_within_5s`,
  `heard_to_first_sound_ms_median`, `heard_to_first_sound_ms_p90`,
  `asked_again`, `held`, `cut_ins`, `tool_timeouts`, `stuck`, `tier`,
  `score` (0 to 100), `failures` = `[{category, count}]`, `route`,
  `diagnosis_version`.
- **Failure categories** (Ten owns the list): `heard_wrongly`,
  `slow_first_sound`, `call_held`, `answer_cut_off`, `tool_timeout`,
  `silent_turn`.

## Open questions

1. `answered_within_5s` is a percent (0 to 100, one decimal) in this app.
   Is that Ten's meaning, or a count?
2. #7452's tier table is not in the repo; `score.DEPTH` is a placeholder
   from tool names to T1 to T4, to be replaced by Backend's.
3. `min_build` is 1 until the first build that carries Ten's ops lands.

## Then

1. Code to make the tests pass (score, report, diagnose, app).
2. `app.json` and `lamp app check` / `pack`.
3. Run it on the virtual lamp (lamp-sim), then on one lamp.

## On this laptop, before the lamp's grants exist

`src/lamp_doctor/local.py` scores a day from the lead's log viewer rows (one
per answered request, timings and token counts, never words) with the same
`score_day`, prints it line by line and writes a small page. It opens no
connection: the rows come from a file or standard input.

    cd lampOS/sdk/apps/lamp-doctor
    curl -s 'http://127.0.0.1:8790/metrics?since=0&until=9999999999999' \
      | python3 -I run_local.py - --html report.html

Only answered requests reach those rows, so held, stuck and asked-again turns
are not seen, and no row names a tool call (tier T1, no tool timeouts).

## Score v6 (the owner's scale, 0 to 10; bands 8 and 6)

`src/lamp_doctor/v6.py` ports #7452's `score_v6.py`: failure weights, soft
ones, count and depth caps, `speed_pts`, the Voice part (0.6 reliability +
0.4 speed), part weights (Voice 0.40, Skills 0.25, Memory 0.20, SDK 0.15),
`MIN_Q` 20 and the bands. `score` in the report is the Voice part, 0 to 10;
`tests/test_v6.py` checks it against the values v6's own docstring states
(2 % failing 8.4, 5 % 6.4, 10 % 4.1, 20 % 1.7). Skills, Memory, SDK, the
tester score and the depth cap are **not scored yet** (the page says so).

| v6 input | Lamp Doctor gets it | from |
|---|---|---|
| question's first-answer time `ms` | now (numbers) | turns: heard_to_first_sound_ms |
| failure: stuck | now (numbers) | turns: outcome held or stuck |
| failure: had to repeat (misheard) | now (numbers) | turns: outcome asked_again, or the flags heard_wrongly, name_missed |
| failure: very slow (over 8 s) | now (numbers) | turns: heard_to_first_sound_ms over 8000 |
| failure: no answer | now (numbers) | turns: answered but no first sound; today the viewer's rows only have answered requests |
| failure: error | not available yet | a lamp error is not in the turn record; a tool timeout is (calls[].timed_out), to confirm it counts |
| lamp signals: skill build failed or refused, task lost or never run, memory not saved, answer lost, speech failed, issue filed | not available yet | journal lines; not in `turns`. Needs a numbers-only field from Ten, or the `logs` switch |
| question's capabilities (caps) for Skills, Memory, SDK, tier, area | later, via model | tool names are in turns (calls[].name); the tool-name to capability table of the #7452 generator is not in the four files |
| question's tier by the words (text_tier) and area (text_area) | later, via model | the words, via model.ask on the lamp; or the caps above |
| conversation level 2+ (follow-ups), generic description | later, via model | the words, via model.ask on the lamp |
| proof of success for Skills, Memory, SDK | not available yet | the lamp's own markers (tools: calls=, skill-run, SDK door); not in `turns` |
| window day or night, MIN_Q 20 | now (numbers) | turns: at (unix minute); counts |
| sdk.installed, sdk.paired | not available yet | off on the public page (proof method), and not an app's to read |


## Store v1 (the person, 26 Nov 14:57: "a doctor app on the store")

Grants that ship today, no `net`, no new grant, no backend: `hear` (`name.heard`), `speech` (`lamp.said`:
only `source` and `cut` are read, `text` is dropped), `device` (`get device`), `notes`, and the panel
(`app.panel.set`). `min_build` 14268: the lowest `lampos-lamp-voice-N` whose commit holds 5fcede282c (the
app socket's panel), by `git merge-base --is-ancestor` over the release tags, 26 Nov. `name.heard` is older (27 Oct).
`src/lamp_doctor/watch.py`:

1. name.heard to the next lamp.said: the answer time (median, p90).
2. A name with no line in 10 s: no answer, a case with its time (a v6 `no_answer` question).
3. `cut` true: an answer cut short, a case.
4. Cached or exact lines as a share of all lines said.
5. `get device` once a minute (0.2.0: the day's min, average, max and last of load, memory, disk, uptime and every temperature zone, fixed keys in the note, the 2 worst on the panel as "hardware, not sent yet"; sending is v2): load over 1.5 (per processor), memory over 90 %, disk over 90 %, a zone over 90 C, hot (75c9 trip_point_1, the second cooling step), is a case; over 70 C, warm (trip_point_0, cooling starts), is on the panel only; trip 2 is critical at 110
   (the lamp's own health bars, docs/health.md; the temperature bar is to be read against 75c9's normal). Each is
   one case until it is back under, and the panel says each as "over X".

Score: v6's Voice part on 1 and 2; Skills, Memory, SDK "not scored yet"; lamp score = 0.4 x Voice. The panel
(title and 6 lines of 80, written once a minute) shows the score, the answer times and counts, the share and the
bars, and the last 3 cases. The day's counts are kept in notes (`day`, written with the panel, read back at start,
rolled at midnight). The `turns`/`report` part (v2, `run_day`, with the model diagnosis behind `with_model`) is
not in the manifest.

Run on the laptop against the SDK's fake lamp: `tests/test_entry.py` runs the app as the lamp does.

## v1 scope earlier (14:54, kept for the laptop cases view)

Timings and failed-call cases from the log viewer's rows and the journal: `run_local.py`, `cases.py`,
`FAILURE-LINES.md` (the journal lines, file:line). The laptop run:
`journalctl -o json ... | python3 -I run_local.py rows.json --journal -`.

## v2 entry (`lamp_doctor.v2`, not in the Store manifest)

Grants `turns`, `report`, `device` only: `report` is refused beside `hear`, `speech`, `microphone`, `camera`, `notes` and
`remember`, so the day's records live in memory (`Ledger`), fed by `turns.finished`, and in the report, never in notes.
Once an hour, when there are new records and at least three questions, the day so far goes out with `log.send` (the
lamp takes one report an hour per app, the cloud keeps one per lamp per day, lamp-control sets `day` and `build`).
`rate` waits its `retry_ms`; `off` and `refused` wait the next hour; `no_grant` and `bad_request` are raised. Both
grants need `LAMP_DOCTOR_TURNS` and `LAMP_DOCTOR_REPORT`, off in every product unit: tested against the stub, not a lamp.
