# Tomorrow's plan's tests

**How they were made.** Not on a lamp, and not recorded: each case's
`.jsonl` files were written by a script in the format `lamp-bus record`
writes, as A meal check's were: one face at the desk (0.4 of the picture
high, moving a little each time) every 15 s while someone sits there, an
empty list while they are away, and the voice resting. Each case's local
time is its `expect.toml`'s `clock`. Re-recording a case on a lamp, with
its owner's yes, replaces the written one.

A replay shows when the read comes due and what it asks `lamp-services`
for. The read's own gate (the owner alone in view, no voice for a minute,
not private, paused or in quiet hours) and what is said are
`service_read`'s, tested in `crates/skills`, and are not replayed here.

**Run** with `lamp-skills test lampOS/skills/tomorrows-plan` (or `cargo test -p
lamp-skills`, which runs every skill's cases).

| Case | What it holds | Result (`lamp-skills test`) |
|---|---|---|
| `reads-once-an-evening` | 19:00, 90 minutes at the desk alone | Pass: one read of tomorrow's calendar at 2 minutes, never again |
| `not-a-glance-in-passing` | 19:00, at the desk one minute, then away | Pass: nothing, under its two minutes |
| `not-in-the-afternoon` | 14:00, an hour at the desk | Pass: nothing, before 18:00 |
| `not-late-at-night` | 21:40, an hour at the desk | Pass: nothing, past 21:30 |
