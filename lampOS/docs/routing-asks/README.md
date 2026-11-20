# Routing asks v3: handoff to the lamp team

`routing-asks-v3.json` is the **spec** of how the lamp's voice should route
what people say: answer it itself, or hand it to the lamp's agent, and through
which door. It describes the behaviour we want, not what today's code does.
The lamp changes until it passes.

v3 rebuilds v2 (`routing-asks-100.json` from #7930, kept here as
`routing-asks-v2.json`). Every label now names a rule in `policy.md`. 32 of
the 100 cases changed label and 19 cases are new (ids 101–119).

## What needs your decision first

Three rulings are new and marked **provisional** in `policy.md`. Please confirm
or change them. Each lists the cases that move if you decide otherwise.

| Ruling | What it says | Cases |
|---|---|---|
| P4b | A result too long to say in three sentences (long writing, or a list of more than about three items), with no file asked for, passes through `ask` **or** `task`. v2 allowed `ask` only, but `ask_agent` returns at most three spoken sentences. | 55–58, 65–67, 69, 70, 78, 81, 82, 92, 93, 95, 96, 100 |
| P7b | A long creative piece the person asks to have written (a story, a speech) passes as told aloud **or** `task`. v2 said "answer or ask, never the task door". | 83–86 |
| P8 | Something refused for safety or privacy is answered by the voice, never handed over. | 105–108 |

Eight labels are contested: seven by a reviewer who judged from the user's
side (`review/g5-user-lens.md`) and one by the blind reviewers. They are kept
as labelled; you may want to rule on them.

| Cases | Label in v3 | The objection |
|---|---|---|
| 85, 86 (speeches) | P7b, told aloud or `task` | The person must deliver the speech later, so only `task` gives them a text. |
| 44, 45 ("what can you do?") | P2 `skill` | A correct spoken answer without `lamp_help` would fail. |
| 33 ("about how much is 50 dollars in dong?") | P2 `search` | A rough answer from memory would fail. |
| 111, 112 (the time in another city) | P2 `time` | A correct answer worked out without `the_time` would fail. Kept because `docs/switches.md:542` records the model giving the lamp's own time for London. |
| 75 (reading log) | P2 `memory` | v2 and the first four blind reviewers read it as `task`. A user-side reviewer chose memory; P2 was then reworded so that a personal list is the lamp's memory, and later blind reviewers agreed. 118 and 119 test the same clause. See `validation.md`. |

One check is still open: **a person on your team** blind-labels 20 cases from
the policy (`review/human-sheet.md`, about fifteen minutes). All reviewers so
far were models, which can share a mistake.

## Files

| File | What it is |
|---|---|
| `routing-asks-v3.json` | The 119 cases |
| `policy.md` | The rules (P1–P8), the order they apply in, and the scoring contract |
| `capabilities.md` | What the voice can do itself and what each door carries, with file:line |
| `changelog.md` | Every label that differs from v2, with the reason, and the new cases |
| `validation.md` | The checks v3 passed and the agreement numbers |
| `decisions.md` | The decisions taken while building v3 |
| `routing-asks-v2.json` | Byte copy of v2 |
| `twins.json` | Groups of cases that must share a label (EN/VI pairs and paraphrases) |
| `review/` | The blind sheets, the reviewers' labels and the user-side reviews |
| `tools/rasks.py` | The gates: `check`, `blind`, `diff3`, `agree` |

## Configuration v3 assumes

- `LAMP_VOICE_ROUTE=second` (DeepSeek chat).
- `LAMP_HARD_ASKS_TO_AGENT=on` in **both** lamp-voice and lamp-tasks, so that
  `ask_agent` is offered. It is off on `origin/subdarren/task-progress`.
- `LAMP_MEMORY_SOCKET` set and `LAMP_MEMORY_PERSONAL_NEEDS_FACE=on`.
- **One language per run.** A lamp has one language, so run the English cases
  (no `lang`) and the Vietnamese cases (`lang: "vi"`) on separately configured
  lamps or in separate runs.

## How to score a run

Only the **first function call** of the turn is scored. Do not wait for the
agent's answer: an ask can take up to 120 s and only one runs at a time.

| Case | Passes when |
|---|---|
| `expect: answer`, no `acts` (P1, P7a, P8) | The first call is not a hand-over. Calling one of the lamp's own tools is fine. |
| `expect: answer` with `acts` (P2) | The first call is a function of that `acts` class. |
| `expect: hand_over` | The first call's door is in `doors`. |

Doors: `ask` = `ask_agent`; `task` = `start_agent_task`; `answer` = no
hand-over call.

Functions per `acts` class:

| `acts` | Functions |
|---|---|
| `light` | `light_on`, `light_off`, `light_colour`, `light_brighter`, `light_dimmer`, `light_warmer`, `light_cooler` |
| `volume` | `volume_up`, `volume_down`, `volume_mute`, `volume_unmute` |
| `timer` | `set_timer` |
| `reminder` | `set_reminder`, `repeat_reminder`, `list_reminders`, `cancel_reminder`, `snooze_reminder` |
| `memory` | `remember`, `recall`, `forget` |
| `time` | `the_time` |
| `weather` | `weather` |
| `search` | `web_search`, `fetch_page` |
| `skill` | `use_skill` with `skill: "lamp_help"` |

Preconditions (`pre`) must be set up before the ask is played:

- `face`: a known face is in view (17, 18, 19, 75, 118, 119). For 19 it must be
  the same face as in 17, because a personal keep is tied to that face.
- `after:<id>`: that case has run first in the same session (19 after 17,
  109 after 12, 110 after 11).

## What changed in the file format

Compatibility with `routing.py` is **unverified**: the script is not in the
repo. No v2 field was removed or renamed, and the ask text of ids 1–100 is
byte-identical to v2. These are the additions your scorer needs to know:

- New fields: `rule` (every case), `pre` (list, optional), `new` (`true` on
  ids 101 and up).
- New `acts` classes: `time`, `weather`, `search`, `skill`.
- `doors` may now be `[answer, task]` (P7b).
- Case 75 changed from `hand_over` to `answer`, so it no longer has `doors`.

Scores are not comparable with the 19 Nov run of 81–88 of 100, because labels
changed.

## Not covered in v3

- Mixed asks ("turn the light blue and plan a trip").
- Follow-ups on running work (`check_agent_task`, a "yes" after a read-back).
  Both need more than one scored call or an earlier turn.
- Linked accounts (calendar, email), which have no rule yet.

## Known weaknesses

- The 17 P4b cases accept both doors, so they test that the lamp hands over,
  not which door it picks.
- Through `ask`, long writing loses everything past three spoken sentences.
  For that door to serve the P4b cases, the agent's full answer would need to
  be kept or sent as well as said.
- The P8 cases (105–108) pass whenever the voice does not hand over. They
  check that a refusal is not passed to the agent; they do not check that the
  voice refuses. A lamp that answers them in full still passes.
- After a `task` hand-over the lamp has no screen: it must tell the person
  where the result is. v3 does not test this.

## When a case splits across runs

If the same ask routes differently from run to run, take it back to
`policy.md` first: either the model has a bug or the rule is unclear. Change
the rule, then the label.

## Re-running the gates

From this folder:

```
python3 -I tools/rasks.py check routing-asks-v3.json --v2 routing-asks-v2.json --policy policy.md --twins twins.json
cd tools && python3 -I -m unittest
```

`check` must print `OK 119 cases`.
