# Decisions for the owner: routing asks v3

Two blind reviewers (A and B, Opus) labelled all 100 asks from `policy.md` and
`capabilities.md`, without seeing v2's labels. Result of the three-way diff
(`review/diff3.json`): **78 keep, 22 change, 0 decide**. A and B gave the same
rule, doors and `acts` on all 100 asks.

**Outcome (2026-11-20).** The user delegated these decisions:

- Sections 2 and 3 were decided by a lamp-user arbiter (`review/user-lens.md`). It agreed with the reviewers on all but two: case 75 is P2 `memory`, and case 83 (with its twin 84) is P7b, doors `[answer, task]`.
- Sections 1 and 4 are product rulings. They went ahead as proposed and are marked provisional in `policy.md`; the lamp team confirms or changes them on handoff. Row 1.2 was overtaken by the arbiter's decision on 83 and 84.

- Case 94 (row 3.7) was later set back to P3 `[ask]`: the final review found that the policy's "about three items" line gives P3 for three museums.

The tables below are the record of what was asked. Recommendations were Claude's.

## 1. Changes to confirm in bulk (22 cases)

| # | Cases | v2 | v3 proposed | Rule | Owner: |
|---|---|---|---|---|---|
| 1.1 | 55, 56, 57, 58, 65, 66, 67, 69, 70, 78, 81, 82, 92, 93, 94, 95, 96, 100 | hand_over, doors `[ask]` | hand_over, doors `[ask, task]` | P4b | |
| 1.2 | 83, 84 (bedtime stories) | hand_over, doors `[answer, ask]` | answer | P7a | |
| 1.3 | 85, 86 (speeches) | hand_over, doors `[answer, ask]` | hand_over, doors `[answer, task]` | P7b | |

## 2. Labels kept, `acts` added (10 cases)

Both reviewers kept v2's `answer` and named the tool class v2 left out.

| # | Cases | `acts` added | Owner: |
|---|---|---|---|
| 2.1 | 13 | `reminder` | |
| 2.2 | 19 | `memory` | |
| 2.3 | 20, 21, 22 | `weather` | |
| 2.4 | 33, 34, 35 | `search` | |
| 2.5 | 44, 45 | `skill` | |

Cases 1, 2, 32, 40 and 41 (time and date) stay `answer` with no `acts`: the
time and date are already in the request.

## 3. Borderline cases the reviewers were unsure about

A and B agreed on each, but at least one marked it low confidence.

| # | Case | Reviewers' label | The other reading | Recommendation | Owner: |
|---|---|---|---|---|---|
| 3.1 | 15 "Turn it down a bit, the kids are asleep." | P2, `acts: volume` | `acts: light` (dim the light) | `volume`; twin 16 says "speak more quietly" | |
| 3.2 | 75 "Start a reading log for me and add The Hobbit…" | P4, `[task]` | P2 `memory` (`remember` could keep it) | P4; a log is a kept record | |
| 3.3 | 77 "Get last year's monthly rainfall for HCMC and tell me which months were wettest." | P3, `[ask]` | P2 `search` (one search) | P3; it works through a table | |
| 3.4 | 78 "Lấy số liệu dân số các nước Đông Nam Á rồi xếp hạng…" | P4b, `[ask, task]` | P3, `[ask]` | P4b; eleven countries do not fit three sentences | |
| 3.5 | 82 "…trẻ 5 tuổi nên ngủ bao nhiêu tiếng rồi lên lịch sinh hoạt buổi tối…" (twin 81) | P4b, `[ask, task]` | P3, `[ask]` | P4b; it asks for a schedule | |
| 3.6 | 83 "Write a bedtime story about a dragon for my son, about ten minutes long." | P7a, answer | P4b (the parent reads it later) | P7a; the lamp can tell it tonight | |
| 3.7 | 94 "Look up the opening hours and ticket prices of three museums in Hanoi and make a list." | P4b, `[ask, task]` | P3, `[ask]` | P4b; "make a list" asks for a list | |

## 4. New rulings to confirm

| # | Ruling | Owner: |
|---|---|---|
| 4.1 | P4b: long writing to keep, with no file asked for, passes through `ask` **or** `task`. | |
| 4.2 | P7b: a speech the person will deliver passes as `answer` or `task`, not `ask`. v2 said "never the task door" for creative pieces. | |
| 4.3 | P8: something refused for safety or privacy is answered by the voice and never handed over. Used by new cases only. | |
| 4.4 | O2: summaries of a page or the news (53, 71, 72) stay `ask` only (P3). Both reviewers agreed with v2 here. | |
