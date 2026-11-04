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

## after2 again on stable 9788, 16:50–16:57 (the first builder's yes, 13:26)

Stable moved to 9788 at 16:30 (it carries #5034: the product unit sets
`LAMP_UNTIED_GOING_ON=held`). lamp-fc4b took it by itself: `lamp-update:
up to date: build 9788` at 16:48:09, the voice restarted at 16:47:01, and
`systemctl show lamp-voice` has `LAMP_UNTIED_GOING_ON=held`. Nothing was
installed or changed by hand. The same runner, the same clips, `after2`
four times (passes a–d, 16:50, 16:52, 16:54, 16:56), the Mac's speaker at
45 % beside the lamp, one person present, silent, nobody in view.

**The chat service was failing through the run**, on this lamp and on
others (Mobile on lamp-0c89 and Migrate on lamp-3e83 posted the same on
#4710 at 16:5x; Friday's bench on lamp-06c1 stopped on DeepSeek's 503 "too
busy"). On lamp-fc4b since 16:47: 9 `a service answered HTTP 503
(service_unavailable_error)`, 3 `said its thoughts are slow`, 7 `ended
before its answer: a newer line began`, and **one** `first answer audio`,
67 419 ms after its words. So "Lamp, what time is it?" was never answered
within the runner's 14 s in any pass, and d07 went 4 s after a 14 s wait
instead of 4 s after an answer. The passes do not reproduce #4450's
after-answer condition; they are kept for what they do show
(`journal-9788-pass-a.txt` to `-d.txt`).

| pass | line | request | the voice's verdict | answered |
|---|---|---|---|---|
| a | a01 | 13 | played: its name was heard in the room's sound; its chat began in 1854 ms and said nothing; cut by d07 20 s later (`0 of its answer's 0 words were played`) | no sound in 20 s |
| a | d07 | 14 | held back: nobody spoke to the lamp: no name, nobody turned to it, no conversation with it going on; its chat round failed, HTTP 503 | no |
| a | d08 | 15 | held back: nobody spoke to the lamp (as above); then, as "request 13's words, answered in its place", **first answer audio 67 419 ms** later, at 16:52:25 | the ask's answer, 67 s late |
| b | a01 | 16 | played: the text shows it was called; it began while that late answer was still playing; cut by d07 | no sound in 20 s |
| b | d07 | 17 | **held back: a conversation with the lamp is going on, but the line has no tie to the lamp: no reply to its question, no word of its last answer, no ask of the lamp** (`LAMP_UNTIED_GOING_ON=held`) | no |
| b | d08 | 18 | **held back, the same `held` reason**; its chat round (for the words answered in its place) failed, HTTP 503 | no |
| c | a01 | 21 | played: the text shows it was called; chat began in 1615 ms and said nothing; cut by d07 | no sound in 20 s |
| c | d07 | 22 | held back: nobody spoke to the lamp | no |
| c | d08 | 23 | held back: nobody spoke to the lamp; HTTP 503 | no |
| d | a01 | 26 | played: its name was heard in the room's sound; chat began in 783 ms and said nothing; `ended before its answer: a newer line began` | no sound in 20 s |
| d | d07 | 27 | held back: nobody spoke to the lamp; HTTP 503, `said its thoughts are slow` | no |
| d | d08 | — | no request opened (the lamp was saying its thoughts are slow) | no |

What it says:

- **`held` fires on 9788.** In pass b the lamp was in a conversation (its
  67 s-late answer was playing), and both desk lines were held with the
  new reason, `a conversation with the lamp is going on, but the line has
  no tie to the lamp`. On 9724 the same lines in a conversation were let
  out to the model and d08 was answered (above). That is one pass, in an
  odd conversation.
- **The clean case is not shown.** With the chat service answering, a01
  is answered in about 3 s and d07 comes 4 s after the answer's end; then
  d08 either is held by `held` or is not. That run is still owed, and
  takes a few minutes once the service answers again.
- The desk lines were held in every pass (8 of 8 held or unopened), but
  in a, c and d for the old reason, since no conversation was going on.
- The 503s are the chat service's, seen on four lamps at once; not 9788's
  and not this lamp's. Before the switch (16:00–16:47) the lamp had no 503
  and no answer either, since nobody spoke to it.

## after2 on stable 9980 with `held` by a drop-in, 04 November 2026

Stable 9868 and 9928 reverted #5034's unit line (#5109, Seventh's owner
count on lamp-52e6), so `LAMP_UNTIED_GOING_ON=held` is off in the product
again and waits on #5110. The switch's code is still in the build, so the
clean after2 run #4450 was owed (the 9788 passes ran under the chat
service's 503s) was played on lamp-fc4b at stable 9980 with `held` set by
a systemd drop-in for the run only, `lamp-voice.service.d/heron-held.conf`
(`[Service] Environment=LAMP_UNTIED_GOING_ON=held`), a `daemon-reload` and
a restart of lamp-voice each way, and once more between the passes (the
drop-in was put back after pass a and set again for pass b); the drop-in
came out at 14:31:48 and the lamp is as found. Journal-only, the same three clips and spacing as the
9788 passes, the person's yes, the slot posted and released on #4710.

| pass | played | a01 "Lamp, what time is it?" | d07 "What time does the shop close today?" (a+4.0) | d08 "I think around nine, but I'm not sure." (g+0.6) |
|---|---|---|---|---|
| a | 14:27:06, 53 s after the restart | no request raised at all (lamp-name's minute line: `heard=0`); void | request 2, held: "nobody spoke to the lamp … no conversation with it going on" (the plain reason) | request 3, the same plain reason |
| b | 14:30:51, 90 s after the restart | request 2, **answered**: "its name was heard in the room's sound"; first answer audio 2257 ms | request 3, let through as the conversation going on ("of its 7 words, 1 in the lamp's answer to request 2 so far": "time"), then **the model called `not_for_me`** | request 4, **held by the rule**: "a conversation with the lamp is going on, but the line has no tie to the lamp: no reply to its question, no word of its last answer, no ask of the lamp" |

Pass b is the run: the ask answered, d08 held by the untied rule with no
model turn, d07 kept from the room by the model. On 9724 (`held` off) d08
was answered. One data point for #5110: a one-word overlap ("time") was
enough for d07 to pass the untied check, while a real follow-up that shares
no word with the answer fails it. Pass a is one sample of a line played within a minute
of a restart raising no request; it is kept only so nobody plays that soon.

`journal-9980-held-pass-b.txt` has pass b's request lines from the lamp's
journal, lamp clock (GMT+7), as the 9788 files do. The request numbers in
it are the journal's; the replay script's own attribution of requests to
clips by time was off by one for d07 and d08 in this pass and is not used.
