# G5: the person's view of routing-asks-v3

Flagged cases only. Known and accepted points (P4b through `ask`, the task
result's location, P5 hand-overs) are not repeated.

| id | ask (shortened) | current label | what feels wrong to the person | label I would give | severity |
|---|---|---|---|---|---|
| 85 | Speech for grandparents' 50th, ~3 min | P7b `[answer, task]` | They have to stand up and give this speech later. If the lamp says it once aloud, they are left with nothing to read from, and the label still passes that. | P4 `[task]` (written down and kept) | wrong |
| 86 | vi: 3-minute speech for the end-of-year class reunion | P7b `[answer, task]` | Same: a speech they will give themselves needs to be written down, not heard once. | P4 `[task]` | wrong |
| 44 | "So what are you able to help me with?" | P2 `skill` | A quick, correct answer from what the lamp knows about itself would please them, but the label fails it. | P1 (still passes a `lamp_help` call) | borderline |
| 45 | vi: "What can you do?" | P2 `skill` | Same as 44. | P1 | borderline |
| 33 | vi: "About how much is 50 dollars in dong?" | P2 `search` | They said "about". An instant "roughly 1.3 million" would satisfy them, but it fails unless the lamp searches. | P1 (still passes a `web_search` call) | borderline |
| 111 | Time in London now | P2 `time` | A correct instant answer (London is 7 hours behind Ho Chi Minh City in November) fails, though the tool is safer around daylight-saving changes. | P1 (still passes `the_time`) | borderline |
| 112 | vi: Time in Paris now | P2 `time` | Same as 111 (Paris is 6 hours behind). | P1 | borderline |

Checked 117 of 117; flagged 7 (2 wrong, 5 borderline).
