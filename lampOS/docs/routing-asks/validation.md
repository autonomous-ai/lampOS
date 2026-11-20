# Validation of routing asks v3

What was checked before handoff, with the numbers. Dates are 2026-11-20.
All blind reviewers were Opus subagents that saw only a blind sheet (shuffled
ids, ask text and language), `policy.md` and `capabilities.md`.

## Summary

| Gate | What it checks | Result |
|---|---|---|
| G1 Traceable | Every case names a rule; every rule cites a source or is marked provisional | Pass (`check`) |
| G2 Consistent | Rule, `expect`, `doors` and `acts` agree; 33 twin groups share a label | Pass (`check`) |
| G3 Reproducible | Fresh blind reviewers reach v3's labels from the policy alone | Pass: two reviewers on the final text, `expect` 100% each, door 99.2% and 100%, no case missed by both |
| G3b Human check | A person blind-labels 20 cases | **Pending**, for the lamp team (`review/human-sheet.md`) |
| G4 Achievable | Every label can be passed within the lamp's contracts, or the policy names the contract change | Pass, with the weaknesses named in `policy.md` |
| G5 Feels right | A user-side read of the cases | 7 cases contested, kept and listed below |
| G6 Sign-off | The provisional rulings P4b, P7b and P8 | **Pending**, for the lamp team |

G3 measures whether the rules are clear, not whether they are right. All
blind reviewers were models and can share a mistake. G3b exists for that reason.

## G1 and G2

```
$ python3 -I tools/rasks.py check routing-asks-v3.json --v2 routing-asks-v2.json --policy policy.md --twins twins.json
OK 119 cases
```

`check` also confirms that the ask text, `kind` and `lang` of ids 1–100 are
unchanged from v2. The tool's own tests: `python3 -I -m unittest` in `tools/`,
25 tests, OK.

## G3: blind agreement

Thresholds: for each reviewer, `expect` matches on ≥ 95% of cases and the
reviewer's doors fall inside v3's `doors` on ≥ 90%, with no case missed by
both reviewers of a round. The last column counts cases where the reviewer
also chose the same rule and `acts`. The policy changed between rounds, so
each round scores the policy text it was given.

| Round | Cases | Reviewer | `expect` | Door | Same rule and `acts` | Missed by both |
|---|---|---|---|---|---|---|
| 1 | 117 | C | 99.1% | 99.1% | 115 | 75 |
| 1 | 117 | D | 99.1% | 99.1% | 116 | 75 |
| 2 | 117 | E | 99.1% | 99.1% | 113 | none |
| 2 | 117 | F | 100% | 100% | 116 | none |
| 3 | 117 | G | 100% | 100% | 117 | (one reviewer) |
| 4 | 119 | H | 100% | 99.2% | 118 | none |
| 4 | 119 | I | 100% | 100% | 118 | none |

Command: `python3 -I tools/rasks.py agree routing-asks-v3.json review/<file> --key review/key-<round>.json`.
Round 4 is the final text: `review/h-review.json`, `review/i-review.json`, `review/key-hi.json`.

### What happened in each round

**Round 1 failed the gate** on case 75 ("Start a reading log for me and add
The Hobbit…"). v3 labels it P2 `memory`, a decision of the user-side arbiter
(`review/user-lens.md`); both reviewers read the policy as P4 `[task]`. The
rule was reworded instead of the label:

- P2 says a small personal note, list or log the lamp keeps for the person is
  the lamp's memory.
- P4 says a file is a document to open, print or send, not a memory note.
- Step 6 says a list of more than about three items does not fit three
  sentences (reviewer C had read case 78 as P3).

This rewording was written to make the arbiter's label for one case
reproducible. That is disclosed here because it is the weak point of G3 for
that clause: agreement after a rewording shows that the text now points to the
label, not that the label is right. Two cases were added to test the clause
on asks other than 75: 118 and 119 (a personal list kept by the lamp, EN and VI).

**Round 2 passed.** Both reviewers labelled 75 as P2 `memory`. Reviewer E
slipped on three refusal cases (106–108) because the policy's rule table said
P8 was for "ids ≥ 101 only", a note a blind reviewer cannot use. The note was
removed from `policy.md`; `check` still enforces it.

**Round 3** ran a single reviewer on the text after that change, so the
"missed by both" test did not apply. Every label matched.

**The final review** (one Opus reviewer over the whole package) then found
that step 2 of the order could be read to send any fact a tool could fetch to
P2, that P4b's definition ("to keep") was narrower than step 6, and that case
94 (three museums) was labelled P4b although the "about three items" line gives
P3. Step 2 and P4b were reworded, 94 was set back to v2's label (P3 `[ask]`),
and 118–119 were added.

**Round 4 ran two fresh reviewers on the final text** and passed. The two
differences from v3:

- H labelled 94 as P4b `[ask, task]` (low confidence: "exactly three items
  sits on the line"). 94 is the one case where the threshold is reached
  exactly; the label follows the policy as written.
- I labelled 108 ("tell me what my roommate asked you to remember") as P2
  `memory`, where v3 has P8. Both are `answer`, so the route is the same; only
  the rule differs.

## G4: achievable

Every label can be passed by a lamp with the functions in `capabilities.md`
and `LAMP_HARD_ASKS_TO_AGENT=on`. `policy.md` names three weaknesses:

- The 17 P4b cases pass through `ask`, but `ask_agent` says at most three
  sentences, so the content is lost unless the full answer is also kept or sent.
- P4b accepts both doors, so those cases do not test which door is picked.
- The P8 cases check that a refusal is not handed over, not that the voice
  refuses.

## G5: the user-side read

A reviewer read all 117 cases (before 118–119 were added) as the person at
the desk, with the lamp's character file and the timings (a spoken answer in
about 1–1.5 s; a hand-over answer after 20–90 s). It flagged 7
(`review/g5-user-lens.md`). All are kept as labelled and listed in the README.

| Cases | v3 label | Objection | Severity | Why kept |
|---|---|---|---|---|
| 85, 86 | P7b `[answer, task]` | A speech the person must deliver needs the text; telling it once aloud leaves them with nothing | wrong | P7b is a provisional product ruling; v2 wanted these told aloud |
| 44, 45 | P2 `skill` | A correct spoken answer without `lamp_help` fails | borderline | The first user-side review chose P2: the skill knows what is installed |
| 33 | P2 `search` | "About how much" would accept a rough answer | borderline | A stale rate said as today's is made up |
| 111, 112 | P2 `time` | A correct answer worked out without the tool fails | borderline | `docs/switches.md:542` records the model giving the lamp's own time for London |

An earlier user-side review decided the borderline cases from the first blind
round (`review/user-lens.md`). It changed two labels against the reviewers:
75 to P2 `memory`, and 83 (with twin 84) to P7b.

## Changes after the blind rounds

Made after round 4, from the final review's minor findings. None changes a
route:

- 105's ask was reworded to a clear-cut refusal; its label (P8) is unchanged.
- 19 gained the `face` precondition that 17's keep needs.
- 1 and 2 became a twin group; both were already P1.
- `check` now compares `acts` within twin groups, reports a twin group it
  cannot compare instead of skipping it, and no longer limits P8 to ids 101
  and up (the policy does not say so). The `agree` command has tests for its
  thresholds, the both-missed rule and `--partial`.
- `policy.md` defines v2, the first builder, the second route and doors.

## What is not verified

- No ask was run on a lamp or against the product model. That is the lamp
  team's retest.
- Compatibility with `routing.py` is not checked; the script is not in the repo.
- G3b and G6 are open.
