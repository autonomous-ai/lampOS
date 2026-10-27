# Own-computer add-ons on lamp-a0ae, 26 Oct 2026 (devin-0-P3, #3563)

`lamp-addons` from #3563's head `6a28a5482`, ARM64 run 40295270326, binary SHA-256
`c44a17b7e9793e3708afe3cb0684378795239ca9fa51a51accd929de98bf77fb`, installed under
`/opt/lampOS/addons/<sha256>/` and run as `/run` units (gone at a reboot): the judge with
`LAMP_ADDONS=on`, and `lamp-addons machine` with `LAMP_MACHINE_ADDONS=on` as `lampos`, with
Harness's data under `/var/lib/lampos-tasks`. The units as run are at the end of `journal-addons.txt`.
The yes file named the owner's Mac. The lamp's Harness (0.4.72, by `lamp-update harness-on`) was
signed in to the owner's account; the Mac ran Harness 0.4.72.

## What was run

From the owner's Mac beside the lamp, with the add-on tools of `daniel-auto/lamp-devin-add-on`
(`watch`, which reads `addon state --machine <id>`, and `try`, which proposes one warm light
`why=evening`, level 2, 30 s, and follows its decision):

| Time (+07) | Proposal | The judge | Back on the Mac |
|---|---|---|---|
| 16:36:59.359 | light, put to the judge as `own-ceb3630785c1` | dropped in 1.4 ms: "the room is busy, or that is not known" (a conversation had just ended) | 16:37:00.343 |
| 16:42:53.952 | light | dropped: "someone is talking to the lamp or it is speaking, or that is not known" | yes |
| 16:53:00.579 | light, once `room/state` said `quiet` | **kept**: "asked of the ring at idle for 30000 ms" | 16:53:01.621 |

`mac-watch.jsonl` is the Mac's log of the same: the lamp's published `voice/state` and `light/state`
as they changed through conversations (`resting` → `listening` → `thinking` → `speaking`), the three
decisions, and two "offline" lines that were Harness answering "AMBIGUOUS" for a lamp name two
machines share (the old one from before the reset), read wrongly by the first version of `watch`.

Also shown in the journal: Harness restarting under the service after the sign-in (16:34:12, one
`503 UNAVAILABLE`, then "Harness restarted; publishing again" and a fresh `hello` at 16:34:18).

## Not shown

- A `say` proposal: the voice takes no add-on's say yet (#2530).
- The yes withdrawn on a lamp (shown only in `tests/machine.rs`).
- Reasons other than `ADDON_WHYS` (the owner's Slack add-on's `messages`/`news`).
- Times measured by the lamp's clock against the Mac's: each row's times are each machine's own.

Nothing of what was said in the room is kept here.

## The stream (devin-0-P4), 17:03

The same lamp, `lamp-addons` built from this branch's head in Docker (`rust:1.96.0-bookworm`, arm64), SHA-256
`36ed8876d74b16775a551443c7882a0bac387c91f4e6592cf168906f8a64626c`, with `machine --stream` and the yes
`{"stream": true, "words": true}`. The owner's Mac ran `watch` through Harness and asked the lamp, by its own
speaker, "Hey Lamp, what is the capital of Japan?". `mac-stream.jsonl` is the Mac's log of lines #33 to #52:
`voice/state`, `name/rising`, `name/heard`, `voice/heard`, `voice/turn`, `policy/decisions`, `voice/said`,
`body/gesture` and diagnostics, none missed, each read on the Mac about 2 s after its time on the lamp. The
words in it are the Mac's synthesized question and the lamp's answer.
