# Jev against the voice, on labelled lines (#4306)

How right Jev (`lamp-policy`, [policy.md](../../docs/policy.md)) is, in shadow, against the voice's own
judgement and against a label, line by line. The jev-verifier keeps it.

- `lines.txt`: the labelled set, one line each: id, split, room, class, label, speaker, text, and for a
  follow-up its parent. Classes: said to the lamp, a bare "Lamp", a follow-up (the same speaker goes on
  right after the lamp's answer, without the name), desk talk, two people by name, the TV, a remark
  about the lamp, and "Ma'am" said to a person, in a quiet and a busy room. Lines marked `held` are
  never used to tune: they are only scored. All names are made up; no line changes the lamp's light,
  volume, timers or memory. q08 ("Lamp?") is short: the voice may drop it as a short sound, which is a
  verdict like any other.
- `render.sh`: renders each line on the Mac (ElevenLabs, the Mac kit's voices), levelled to -20 LUFS,
  into `clips/` (not committed). The TV is a colleague's voice through a small speaker's band, 6 dB
  under the rest on purpose: the TV class tests what is said, not a voice the lamp has never heard.
- `slot.sh DAY START LAST END`: the way to run it in a booked slot (it re-runs itself under
  `caffeinate` so the Mac stays awake):

      SESS=<session> tools/jev-verify/slot.sh 2026-10-30 02:30 02:40 03:50

  It tries the lamp lock from START; not free by LAST, it writes "aborted, nothing ran" to loans.txt and
  stops. Taken, it posts PLAYING on #3336, runs `replay.sh run` with a deadline of END - 4 min, renews
  the lock, and on exit posts QUIET with the result and releases the lock. `slot-test.sh` tests it.
- `replay.sh run OUT`: wakes the lamp (the Mac goes 45 -> 60 -> 75 only on "held back"), plays the
  quiet block, plays two colleagues' exchanges, then the busy block with talk between its lines (it
  plays in whatever room the lamp reads: under LAMP_SPOKEN_TO=on held-back talk never makes room/state
  busy, crates/voice/src/room_told.rs), then pulls that window's decisions and the voice's verdicts into `OUT`.
  After each pass but the last it pulls and scores: once Jev was not quiet on more than GATE_MISS (2)
  lines not said to the lamp, the #4306 gate (>=70/72) can't be met, so it stops and gives the lamp back.
  A follow-up plays right after the lamp's answer to its parent, only when it answered. Every play
  records `room/state` just before it. The units, their MainPIDs, lamp-policy's cmdline, the build and
  the mute are taken after the preflight and checked at each pass start: any change aborts. With
  DEADLINE set, no line or wait starts after it. It writes a loans.txt line at the start and one at the
  end ("done", or "aborted, nothing ran"). For hello and progress it restarts lamp-policy in shadow
  with `--hello --progress` through a /run drop-in, and puts it back on exit; if that can't be done it
  says "hello/progress NOT MEASURED" and goes on with attention. Put back means lamp-policy runs again
  with the cmdline it had before the run. Every ssh shares one connection (a fresh one took ~5.8 s on
  30/10), and the lamp's clock offset is the midpoint of the fastest of 5 round trips (over 1 s aborts).

  `replay.sh preflight OUT` runs every check, the wake (which must produce a policy decision that did
  not fail) and a busy probe (two exchanges, no labelled line: what `room/state` reads), then
  stops. `OUT` holds what the lamp heard: it stays on the Mac, and is refused inside a git work tree.
- `rescore.py OUT NEW [WEIGHTS.json]`: re-judges a run offline with other attention weights or bands
  (lamp-policy's combine, crates/policy/src/attention.rs), from the seven answers each decision kept;
  then `score.sh NEW`. No WEIGHTS.json gives the shipped values and must score like the run. A changed
  question can't be scored this way. `rescore-test.sh` tests it.
- `score.sh OUT`: the scored table (per class and room, Jev vs the voice vs the label), the totals on
  the held lines, the false answers to desk talk, and the decision latency. Each (pass, id) is one
  trial; it says which ids flipped across passes. A missing or failed Jev decision is wrong, on "not"
  lines too. Busy lines are scored as "background talk", in the room as the lamp read it (how many
  read busy is counted). A trial is clean unless the lamp was in conversation (spoke within 20 s; a follow-up is in
  conversation by design and stays clean) or a quiet line was seen in a room not quiet; totals show all and
  clean trials, and "Decision inputs" gives the held clean rates with 95% Wilson intervals, the margin
  and the verdict. The per-line table shows probabilities, so it defaults to the tune lines. Hello and
  progress decisions are counted by choice: the set has no arrival (no face is ever in view) and hands
  no work over, so those numbers are thin by design and say so. It prints ids and classes, never the
  words. `score-test.sh` tests it with no lamp.

Jev takes the turn with `answer` or `look_up`; the voice takes it when its answer is played.

What a yes needs, for each Jev question (attention, hello, progress): Jev beats the voice on the held
lines, with the margin and the counts stated, and no new false answer to desk talk. Then the same set
again with that `--act` on, and the scores holding.
