# Jev against the voice, on labelled lines (#4306)

How right Jev (`lamp-policy`, [policy.md](../../docs/policy.md)) is, in shadow, against the voice's own
judgement and against a label, line by line. The jev-verifier keeps it.

- `lines.txt`: the labelled set, one line each: id, split, room, class, label, speaker, text. Classes:
  said to the lamp, a bare "Lamp", desk talk, two people by name, the TV, a remark about the lamp, and
  "Ma'am" said to a person, in a quiet and a busy room. Lines marked `held` are never used to tune:
  they are only scored. All names are made up; no line changes the lamp's light, volume, timers or
  memory.
- `render.sh`: renders each line on the Mac (ElevenLabs, the Mac kit's voices), levelled to -20 LUFS,
  into `clips/` (not committed). The TV goes through a small speaker's band, 6 dB under.
- `replay.sh OUT`: wakes the lamp, plays the quiet block, makes the room busy with two colleagues'
  talk (and checks `room/state` reads busy before the busy block), plays it, then pulls that window's
  decisions and the voice's verdicts into `OUT`. Run it only in a booked slot, under the lock:

      tools/lamp-lock.sh lamp-52e6 run jev-verifier <session> "jev-verify set" -- tools/jev-verify/replay.sh OUT

  `OUT` holds what the lamp heard: it stays on the Mac.
- `score.sh OUT`: the scored table (per class and room, Jev vs the voice vs the label), the totals on
  the held lines, the false answers to desk talk, and the decision latency. It prints ids and
  classes, never the words. `score-test.sh` tests it with no lamp.

Jev takes the turn with `answer` or `look_up`; the voice takes it when its answer is played.

What a yes needs, for each Jev question (attention, hello, progress): Jev beats the voice on the held
lines, with the margin and the counts stated, and no new false answer to desk talk. Then the same set
again with that `--act` on, and the scores holding.
