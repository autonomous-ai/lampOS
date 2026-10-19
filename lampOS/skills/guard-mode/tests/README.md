# Guard mode's tests

**How they were made.** Not on a lamp: guard mode has not run on one. Each
case's `.jsonl` files were written by a script in the format `lamp-bus
record` writes, as Light out's were: `room/presence` once a second, each good
for 3 s, someone until 20 s after the last face or voice, as the eyes say it
with `LAMP_PRESENCE=on`; `humans/faces/tracked` once a second, half a second
off, a face 0.2 of the picture's height while someone is near; the person's
ask on `voice/heard` at 2 s; and in one case a sample of `voice/privacy`, the
voice's lease, once a second. What the eyes know of the face at the lamp is
not on the bus (it is the eyes' known file, which the program reads), so a
case says it as `[[known]]`: from `at`, that name, or with none, nobody
known. Each `expect.toml` sets the local time of the first message
(`clock = "14:20"`), and the model calls `keep_watch` at 4 s. The mark is
`light`, the mark given back `ends`, a text `text`.

**Run** with `lamp-skills test lampOS/skills/guard-mode` (or `cargo test -p
lamp-skills`, which runs every skill's cases).

| Case | What it holds | Result |
|---|---|---|
| `a-visit-gives-one-text-with-its-time-and-length` | Asked at 14:20; the desk empty from 49 s; someone at 14:31 (660 s) for two minutes, a face it doesn't know | Pass: the mark at 4 s; one text at 799 s, when they've gone: "Someone I don't know was at your desk at 14:31, for about 2 minutes." |
| `a-second-visit-within-ten-minutes-is-not-texted` | Visits at 14:31, 14:36 and 14:45, a minute each, heard but no face | Pass: texts at 739 s and 1579 s; none for the second, 300 s after the first text |
| `your-face-back-ends-it` | A, whose face the eyes know, asks and leaves; back at 14:40 and known at 1202 s | Pass: the mark at 4 s, given back at 1202 s; no text |
| `someone-known-is-said-as-known-never-by-name` | A asks and leaves; B, also known, at the desk for three minutes | Pass: one text at 859 s, "Someone you've introduced me to was at your desk at 14:31, for about 3 minutes."; no name |
| `the-privacy-switch-closed-says-so-each-closing` | The switch closed from 300 s for 30 s, and again from 600 s | Pass: one text each closing, at 300.25 s and 600.25 s, "I can't see right now, the privacy switch is closed.", and nothing while it stays closed (each closing told, at most one line a minute: the peer read's P3 on #1807) |
| `presence-while-the-switch-is-closed-is-no-visit` | The switch closed from 380 s to 500 s while `room/presence` still says someone at 400-470 s (a presence the eyes would not give: reviewer-two's P3 on #1807); open, someone at 700-770 s | Pass: the switch's line at 380.25 s, no text for the closed visit, and one at 770 s, "Someone was at your desk at 14:31, for about a minute." |
| `never-on-from-a-text` | The tool called with nothing heard at the lamp, as a typed message would; a visit after | Pass: nothing, no mark and no text |
| `stop-gives-the-mark-back` | On, then "stop" at the lamp and `turn = "off"` at 300 s; a visit after | Pass: the mark given back at 300 s; the visit after is not texted |
