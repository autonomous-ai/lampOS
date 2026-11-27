# Posture's tests

**How they were made.** Not on a lamp, and not recorded: each case's
`.jsonl` files were written by a script in the format `lamp-bus record`
writes, as Sit back's were: `lamp-seen`'s `humans/posture` every 30 s from
its second check (30 s into the sitting), `window_s` 60, lamp-seen's minute
of two checks (`pose::window::WINDOW_US`, `MIN_SAMPLES`), and `bad_share`
0, 0.5 or 1 over those two, with its 90 s lifespan; one face every 1.5 s through the first 20 minutes and from 68 to 72, else every 30 s (a second, smaller face to one side where a
case says someone else is in view; none once they have gone); and the voice
resting unless a case says they talk. Each case's local time is its
`expect.toml`'s `clock`. Re-recording a case on a lamp, with its owner's
yes, replaces the written one.

**Run** with `lamp-skills test lampOS/skills/posture` (or `cargo test -p
lamp-skills`, which runs every skill's cases). That it reads the posture
alone, one trigger a region, V1's numbers and lines that give no number and
name no condition is
`posture_reads_the_posture_alone_and_names_a_region_never_a_condition` in
`crates/skills/tests/it/skills.rs`. That a bad posture alone brings no word
from Sit back is Sit back's `a-bad-posture-alone-is-let-be`.

| Case | What it holds | Result (`lamp-skills test`) |
|---|---|---|
| `a-word-for-the-neck` | 11:00, both checks bad from 30 s to 10 minutes, region `neck`, then sitting well | Pass: at 30 s, the window full: turns to the face and says the neck's line, once |
| `a-word-for-the-back` | As the neck's, region `trunk` | Pass: the back's line at 30 s, once |
| `a-word-for-the-shoulders` | As the neck's, region `upper_arm` | Pass: the shoulders' line at 30 s, once |
| `a-word-for-the-forearms` | As the neck's, region `lower_arm` | Pass: the forearms' line at 30 s, once |
| `a-word-when-no-region-stands-out` | As the neck's, region `general` (bad by the RULA risk alone) | Pass: V1's "empty / mixed" line, a short stretch, at 30 s |
| `a-word-once-both-checks-are-bad` | One check of two bad (`bad_share` 0.5) to 3 minutes, then both bad | Pass: nothing at 0.5; the neck's line at 3m30 (V1's 0.6 or more is both checks bad) |
| `let-be-below-v1s-share` | `bad_share` 0.5 all through | Pass: nothing |
| `let-be-before-the-window-is-full` | `window_s` 30 (today's lamp-seen never says it), both bad, up and gone at 9 minutes 30 | Pass: nothing: the window never full |
| `no-word-without-lamp-seen` | `humans/posture` declared, never said | Pass: nothing |
| `a-word-with-someone-else-in-view` | A second face in view from 9 to 12 minutes, both checks bad from 9m30 | Pass: said at 9m30 with them there (no others-in-view hold, the owner, 27 Nov) |
| `held-while-they-talk` | Both checks bad from 9m30, a conversation from 9 to 12 minutes | Pass: held, said at 12 and a half, the voice resting 5 s and the lamp quiet 10 s |
| `a-word-late-at-night` | 22:00, bad neck to 10 minutes | Pass: the neck's line at 30 s (no `[quiet]`, the owner, 27 Nov) |
| `off-when-asked` | `wellbeing_nudges(how = "off")` at 10 s, bad neck all through | Pass: nothing (`[stop]`; its kind, `posture`, is taken only where the function is shared, as on a product lamp) |
| `a-new-word-when-the-slump-moves` | Bad neck to 8 minutes, then bad back to 20 | Pass: the neck's line at 30 s, the back's at 8m30, at once: each region its own 15m cooldown (the owner, 27 Nov: "neck slump and back slump is two different thing") |
| `a-second-word-a-quarter-hour-on` | Bad neck for 25 minutes | Pass: the neck's line at 30 s and again at 15m30, not before: the 15m cooldown (the owner, 27 Nov: "only 15m cooldown, not 60m") |
