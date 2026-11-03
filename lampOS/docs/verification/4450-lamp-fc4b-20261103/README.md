# #4450's three desk lines replayed on lamp-fc4b, 2026-11-03 (heron-P1)

#4450 (the core-verifier, lamp-52e6, stable 8670, 30 Oct): in a quiet room
the lamp answered three desk lines that carried no name, desk1 d02, desk3
d10 and after2 d08 (not spoken to 13/16 = 0.81, bar 0.90). The first
builder asked heron (#4710, 12:42) to replay those scenes on lamp-fc4b at
stable 9724, read-only, from the journal alone, and to say whether they are
still answered.

**Answer: d02 and d10 are held on 9724; d08 is still answered.** All eight
desk lines of desk1 and desk3 were held with `nobody spoke to the lamp: no
name, nobody turned to it, no conversation with it going on`. after2's d08,
desk talk 4 s after the lamp answered "Lamp, what time is it?", was let out
as `a conversation with the lamp is going on, only as the conversation
going on: the model is told so` and the model answered it, in both passes
(first answer audio 1824 ms and 1898 ms after the words). That is the case
`LAMP_UNTIED_GOING_ON=held` (#5008, #5034) is for; 9724 does not carry it.

## What ran

- **lamp-fc4b**, the person's own lamp, stable **9724** (`lamp-update:
  switching to build 9724` 12:23:50, `up to date: build 9724` 12:25:10 +07,
  from the stable channel by itself). Product units as installed, no
  drop-in (`/run/systemd/system/lamp-voice.service.d` absent), nothing
  installed, nothing restarted, no `sudo` (the lamp's user has none without
  a password; the journal is readable as a member of `systemd-journal`).
  No `lamp.lock`: the lamp is not shared. Eyes, body, voice, update all
  active. The lamp's clock runs 0.42 s ahead of the Mac's.
- **The clips:** desk-kit-2 (`desk-kit-2.tar`, sha256 `60128bcd…`, fetched
  from the test-kits bucket and checked, as night.py does), the scenes
  `desk1`, `desk3` and `after2` with night.py's line ids and spacing:
  desk1 d01, d02 g+0.6, d03 g+1.2, d04 g+0.5; desk3 d09, d10 g+0.6, d11
  g+1.4, d12 g+0.5; after2 a01, d07 a+4.0, d08 g+0.6; 22 s of quiet before
  each scene. Played with `afplay` from a MacBook Pro's own speakers at
  45 %, about 1 m from the lamp, in the person's room with the door shut;
  one person present, silent, out of the lamp's view. Nobody was in view
  through the run (made voices from a speaker have no face).
- **Read from the journal alone**, a variant of night.py without its
  events tail (needs `sudo`) and its bus taps (lamp-fc4b has no
  `/usr/local/bin/lamp-bus`): one ssh follows `journalctl -u lamp-voice -f
  -o short-unix` while the clips play; after2's `a+4.0` counts from the
  journal's `request N first answer audio` line plus the answer's length
  estimated from its ElevenLabs character count (characters / 14 + 0.6 s;
  38 characters gave 3.3 s). After the run `journalctl -u lamp-voice
  --since --until` over the window is kept here (`journal-pass2.txt`,
  `journal-pass3-after2.txt`: the request lines, in the lamp's clock;
  connection and token lines left out). Requests are matched to clips by
  time and by `request N heard K words` against the lines' own word
  counts. The script is Python, so it stays in heron's scratch folder
  (joining.md); this is what it did. The Mac's clock offset is the least
  of five `date +%s.%N` reads over one ssh connection.

## Three passes

1. **12:51–12:53, void.** The Mac's sound went to headphones that were
   plugged in; the lamp opened two requests and heard 0 words in each.
   Not counted; its PLAYING line on #4710 says so.
2. **12:55–12:57, all three scenes.** The clock offset had been measured
   by timing a whole ssh (+3.3 s, wrong by 2.9 s), so the runner missed
   the lamp's answer to a01 and played d07 after its 14 s wait: 13.6 s
   after the answer's end instead of 4 s. desk1 and desk3 are unaffected
   (their lines are spaced clip to clip).
3. **13:00, after2 alone**, the offset read right (+0.42 s): the answer to
   a01 began 2.9 s after the ask and d07 played 4.0 s after its estimated
   end.

## What the journal said

Pass 2 (requests 30–45) and pass 3 (48–51). Mac clock, +07.

| scene | line | said | pass | request | heard | the voice's verdict | answered |
|---|---|---|---|---|---|---|---|
| desk1 | d01 | Did you send the slides to Sam yet? | 2 | 30 | 8 words | held back: nobody spoke to the lamp: no name, nobody turned to it, no conversation with it going on | no |
| desk1 | **d02** | Not yet, I'll do it after lunch. | 2 | 31 | 7 | held back: nobody spoke to the lamp (as above) | **no** (#4450: 1532 ms on 8670) |
| desk1 | d03 | Are you coming to the meeting at four? | 2 | 32 | 8 | held back: nobody spoke to the lamp | no |
| desk1 | d04 | I think so, but I might be a bit late. | 2 | 33 | 10 | held back: nobody spoke to the lamp | no |
| desk3 | d09 | That lamp is cute, where did you get it? | 2 | 35 | 9 | held back: nobody spoke to the lamp | no |
| desk3 | **d10** | It's from the office, they're testing it. | 2 | 36 | 7 | held back: nobody spoke to the lamp | **no** (#4450: 1683 ms on 8670) |
| desk3 | d11 | Do you want coffee? I'm going downstairs. | 2 | 38 | 7 | held back: nobody spoke to the lamp | no |
| desk3 | d12 | Yes please, a latte, no sugar. | 2 | 39 | 7 | held back: nobody spoke to the lamp | no |
| — | — | nothing played, 12:56:23–12:56:34 (the room) | 2 | 40, 41, 42 | 0, 1, 1 | 41 and 42 held back: nobody spoke to the lamp | no |
| after2 | a01 | Lamp, what time is it? | 2 | 43 | 5 | played: its name was heard in the room's sound | yes, 1528 ms |
| after2 | d07 | What time does the shop close today? (13.6 s after the answer) | 2 | 44 | 7 | played: a conversation with the lamp is going on, only as the conversation going on: the model is told so; the model called `not_for_me` | no sound |
| after2 | **d08** | I think around nine, but I'm not sure. | 2 | 45 | 8 | played: a conversation with the lamp is going on, only as the conversation going on: the model is told so | **yes, 1824 ms** |
| after2 | a01 | Lamp, what time is it? | 3 | 49 | 5 | played: its name was heard in the room's sound | yes, 1564 ms |
| after2 | d07 | What time does the shop close today? (4.0 s after the answer) | 3 | 50 | 7 | played: a conversation going on, the model is told so; its answer cut after 10 ms, 0 words played, by d08 | no sound |
| after2 | **d08** | I think around nine, but I'm not sure. | 3 | 51 | 8 | played: a conversation going on, the model is told so; `goes on from request 50's words, cut short 1830 ms before it began: no sign of a second person: no face in view at request 50's end, no face in view at request 51's, a quiet room, no other voice heard` | **yes, 1898 ms** |

Requests 34 and 37 opened and were folded into their neighbours (no words
line of their own); 31, 37, 39 and 42 "go on from" the request before
them (1500 ms of sound first), as the kit's 0.5–0.6 s gaps invite.
Every played line was heard: 7–10 words of lines of 6–10 words. Each held
line also said `no conversation going on: … it was never called, 0 lines
left alone since it last spoke`.

## What it says, and what it does not

- The quiet-room cases of #4450 (d02, d10) do not reproduce on 9724: the
  hold is the "nobody spoke to the lamp" rule, and it held 8 of 8 desk
  lines in desk1 and desk3 and the room's own two sounds. Which change
  between 8670 and 9724 did it is not read here.
- The after-answer case (d08) reproduces, twice. On 9724 a desk line 4 s
  (or 13.6 s) after the lamp's own answer is inside its conversation
  window and goes to the model with a note; the model answered d08 both
  times and declined d07 once. With `LAMP_UNTIED_GOING_ON=held` (#5034)
  such a line would be held before the model; a replay of after2 on the
  stable that carries it would show whether d08 is then held.
- `LAMP_LIKE_ASKER_MEASURE` (#4859) is not on 9724, so nothing of it was
  measured, and lamp-fc4b has no bus tool, so lamp-name's scores were not
  read; the voice's own `its name was heard in the room's sound` was.
- One lamp, one room, one Mac speaker, one pass per scene (after2 twice).
  The verifier's run on 52e6 had a different room and a different mic
  distance; the counts here are not its counts.
- Nothing on the lamp was changed; the Mac's speaker volume was put back
  (it had been 14 %).
