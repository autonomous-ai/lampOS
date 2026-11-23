# Posture's tests

**How they were made.** Not on a lamp, and not recorded: each case's
`.jsonl` files were written by a script in the format `lamp-bus record`
writes, as Sit back's were: `lamp-seen`'s `humans/posture` every 30 s from
its third sample (60 s into the sitting), `window_s` the sitting's length up
to lamp-seen's ten minutes (`pose::window::WINDOW_US`), with its 90 s
lifespan; one face every 1.5 s through the first 20 minutes and from 68 to 72, else every 30 s (a second, smaller face to one side where a
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
| `a-word-for-the-neck` | 11:00, sat 20 minutes, `bad_share` 0.7, region `neck` | Pass: at minute 10, the window full: turns to the face and says the neck's line, once |
| `a-word-for-the-back` | As the neck's, region `trunk` | Pass: the back's line at 10, once |
| `a-word-for-the-shoulders` | As the neck's, region `upper_arm` | Pass: the shoulders' line at 10, once |
| `a-word-for-the-forearms` | As the neck's, region `lower_arm` | Pass: the forearms' line at 10, once |
| `a-word-when-no-region-stands-out` | As the neck's, region `none` (bad by the RULA risk alone) | Pass: V1's "empty / mixed" line, a short stretch, at 10 |
| `a-word-at-v1s-share-exactly` | `bad_share` 0.6, V1's `POSE_BAD_RATIO` | Pass: the neck's line at 10 (V1 nudges at 0.6 or more) |
| `let-be-below-v1s-share` | `bad_share` 0.55 all through | Pass: nothing |
| `let-be-before-the-window-is-full` | `bad_share` 0.9, up and gone at 9 minutes 30 | Pass: nothing: the window never full |
| `no-word-without-lamp-seen` | `humans/posture` declared, never said | Pass: nothing |
| `held-while-someone-else-is-in-view` | Bad neck, a second face in view from 9 to 12 minutes | Pass: held at 10, said at 13, a minute after they went |
| `held-while-they-talk` | Bad neck, a conversation from 9 to 12 minutes | Pass: held at 10, said at 12 and a half, the voice resting 5 s and the lamp quiet 30 s |
| `quiet-at-night` | 22:00, bad neck all through | Pass: nothing (`[quiet]` 22:00 to 08:00) |
| `off-when-asked` | `wellbeing_nudges(how = "off")` at a minute, bad neck all through | Pass: nothing (`[stop]`; its kind, `posture`, is taken only where the function is shared, as on a product lamp) |
| `one-word-an-hour-whatever-the-region` | Bad neck to 15 minutes, then bad back to 50 | Pass: the neck's line at 10, nothing more (the 60m cooldown, and the wellbeing group's hour between the triggers) |
| `a-second-word-an-hour-on` | Bad neck for 80 minutes | Pass: the neck's line at 10 and again at 70, not before: V1 judged a 60-min window; V2 judges lamp-seen's 10-min window, and the 60m cooldown keeps it to one an hour |
