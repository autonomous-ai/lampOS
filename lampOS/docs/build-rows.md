# Where V2 is: the build rows and the quality rows

Written by the first builder from the team's review page at 2026-10-20 17:43 (+07). The lead asked for it on #1711 (20 Oct, 16:43), so anyone can read where V2 is without waiting for the hourly post. It's a copy, updated a few times a day. The hourly figures on #1722 are the live ones.

**Build 82.5% over 72 rows. Quality 66.6% over 16 rows.**

**How a build row is counted** (the lead's rule): 1.0 when its work is merged and tried on a lamp; less while parts are missing or untried. Build is the mean of the rows. Quality rows score how well a part works today, against a target; quality is the mean of the scores.

## Build rows

| Row | Score | Issues | Owners | Where it is |
|---|---|---|---|---|
| The voice never dies (SD stalls, power cuts, midnight) | 1.00 | #784 #762 #1178 | fifth, eighth | Midnight fix and stall recorder merged; |
| Speed: tool turns and comma pauses | 1.00 | #799 #782 | second, fourth |  |
| Who is talking, and when not to answer | 1.00 | #753 #792 #800 | fourth, second, seventh | #800 behind switches |
| Privacy switch closes both microphones | 1.00 | #795 | — |  |
| Faces: remember, hello by name | 1.00 | #759 #775 | first, seventh | Untried with a real face |
| Hand work to Claude Code or Codex, and its news | 1.00 | #751 #765 #798 #733 | seventh |  |
| Backup voice when Gemini is down | 1.00 | #778 #754 | sixth | The relay chat it uses has been down since 02:29 |
| Gives way to a hand; wakes after power | 1.00 | #780 #790 #702 #703 #701 | fifth | Arm-goal guard merged (#1192) |
| “Lamp, that was weird” marks a moment | 1.00 | #803 | sixth |  |
| The first five minutes | 0.80 | #841 | seventh | 20 Oct 14:0x, up from 0.70: try 6 with the lead (Seventh): the touch step passes on current Controls with the head pads on lines 97/98, and the name part passes on a played call (3 of 3); |
| Speaks first when it should (the Policy) | 1.00 | #752 | second |  |
| Each person’s memory kept apart | 1.00 | #764 | seventh |  |
| Sleep, and waking on its name | 1.00 | #749 #714 | fourth, second |  |
| Reminders, timers, memory across days | 1.00 | #748 #743 | — |  |
| Setup for a home, 5 s reset | 0.90 | #806 #1209 | first | 20 Oct 13:3x, up from 0.65: lamp-06c1 done end to end with its person (El, #806): the 5 s reset wiped it and rebooted, it was set up again from the app (connected, the link and keys kept 600), “Hey Lamp” heard and answer… |
| Touch strip and head pads | 0.70 | #711 | first | 20 Oct 10:1x, up from 0.50: on lamp-c0a4 with its person (Thirteen), every head-pad touch read as touch, stroke, sleep with the pads’ real lines (37 and 97), and nothing came in 20 s hands away; |
| Two people at the lamp | 1.00 | #829 #1158 #1278 | fourth | All merged: day and night bars (#1158, 11:49) and a follow-up within 20 s told it may follow on (#1278, 13:57). |
| “Stop” over its own voice | 1.00 | #828 #1047 #1264 | fourth | Built: stop words heard over its voice (#1047, 09:11) and listening gaps in its pauses (#1264, 11:52), both off until measured. |
| Skills: say service, Reminders, Task news | 1.00 | #939 #1042 #1237 #1253 | second | All merged: the say service and the Policy gate, Reminders (with #1253’s fix), Task news as a skill (#1042, 11:44) |
| Skills: body and light shared between skills | 1.00 | #940 #1239 #1252 | — | All merged: the light side, the body’s side (#1239, 11:45) and the skills side (#1252, 12:32), behind their switches |
| Skills: creator, Stretch break, Stuffy air, Desk tutor | 1.00 | #941 #1066 #1011 #1260 | ten | All merged: Stuffy air (#1066), the reading pose (#1011, 08:58), Desk tutor’s nod and tilt (#1260, 10:23); |
| Two-minute self-test | 1.00 | #812 #983 #1243 #1245 | ten | All merged: every part checked, reported in health (#1243), run at first boot and every 6 hours (#1245), and the head’s small move (#983, 12:23) |
| One install for a new lamp | 1.00 | #811 | nine | Shown 19 Oct 15:1x on a second lamp, lamp-c0a4: build 3162 installed over V1 from a test-signed local channel, all 45 files equal to the build’s checksums, V1 stopped and kept, no reboot; |
| No internet at home | 1.00 | #830 #1242 #1256 #1247 | nine | Built: the offline start tested both ways (#1242, #1256), and lamp/online asks the internet itself, so the company’s service being down is told apart (#1247, 11:47). |
| Everything is a skill, core and installable, no one-off code | 0.50 | #940 #938 #1667 #1696 #1698 | seventh | The owner, 19 Oct 07:2x: core skills always installed, installable ones like apps. |
| An owner makes their own skill | 0.50 | #941 #1698 | ten | Merged 19 Oct 09:3x (#1698): a skill is packed into a checked file and installed or removed on a running lamp, within its declared permissions. |
| A Skills Store to publish skills | 0.25 | #1698 | — | The package a store would carry is built (#1698: versioned, every file checked, a place for a signature). |
| Updates over the internet, by themselves | 0.65 | #737 | second | 20 Oct 06:11: shown again on lamp-4ace on the merged code (El’s test, three cases: a good build kept, one that fails to start put back in under 2 min, one that never confirms put back after its 10 min), the lamp left exa… |
| A voice cheaper than Gemini Live, as fast and as good | 1.00 | #754 #1686 #1712 #1973 #2135 #2136 | sixth, first, fourth, ten, backend, third, eighth, twelve | 20 Oct 10:1x, built, by the lead’s rule (it works and it is fast enough are separate): on the main line since #1973, on lamp-4ace, and 32 of 32 answered for the lead on lamp-52e6. |
| A person brings their own DeepSeek and Deepgram keys, in setup or the app | 0.45 | #2094 | el, fourteen | 20 Oct 14:2x, up from 0.25: the app side is merged (ecm-sds-mobile#11); |
| Lamp 2 in the Autonomous app | 1.00 | #1396 | ten, mobile, backend | 20 Oct 11:4x, built: the app's Skills page and name and time zone (ecm-sds-mobile#8, #9) merged into the app's v3 after their tries on lamp-0c89. |
| Talks with you naturally: turns, answers, web answers, the time | 1.00 | #694 | second, nine | Built (the conversation itself); |
| Hears its name, a bare “Lamp” too | 1.00 | #694 #725 | second, nine | Built; |
| Eye contact: turns to you and to its name, looks away to think, nods | 1.00 | #715 | fifth | Built |
| Always alive: breathing, small drift, a glance at a bang | 1.00 | #710 #714 #715 | fifth, second | Built |
| Moves with its words while it speaks | 1.00 | #933 #1067 #1365 #2030 | thirteen, fifth, third | 20 Oct 09:4x, built: #1759 (with #1067’s sway) merged into lamp-voice after its try on lamp-52e6 with the lead, so every lamp nods and sways while it speaks, sparse as the lead chose. |
| Never tips itself over | 1.00 | #1072 #704 | eighth | Built with V1’s limits; |
| One inner state (mood) every behaviour reads, and familiarity | 0.60 | #713 #1670 #1728 #1749 #1772 | fourth, ten | Built and merged, off until switched on (Ten): the mood part as a product unit, whose one breath the ring and the head take (#1728); |
| Joys: sways to music, glances at a laugh, watches you leave, core skills | 0.65 | #1669 #1707 #1748 | sixth | 20 Oct 16:0x, up from 0.50: watches you leave passed on lamp-52e6 with the lead (row 14, #2250: stood_up published on a real stand-up); |
| Dance when asked | 1.00 | #999 | jarvis | Built (the Dance skill, V1’s dance clips) |
| Bedtime and Morning as a skill, with night caps on light and volume | 0.50 | #1672 #1732 | jarvis, second | Built 19 Oct 11:1x as a core skill, off until tried (#1732, in review): dimmer, warmer, quieter and slower at night. |
| “What do you see?” and showing it a page | 1.00 | #836 #842 | seventh | Built |
| Seeing someone speak (is the face it looks at talking?) | 0.75 | #720 #1470 #2008 #2026 | fifth, ten | The mouth reading is merged and off. |
| The eye: looks toward you, blinks, a face for each state | 1.00 | #712 | first | Built (ring eye, asleep face) |
| Its light by voice and in the app, kept across a restart | 1.00 | #1472 #1484 #1464 #1468 | — | Built |
| Volume by voice and in the app | 1.00 | #1458 #1465 | — | Built |
| A light for booting and a “not feeling well” face for a failed part, built into the system | 0.80 | — | — | 20 Oct 11:35, up from 0.50: not feeling well tried on lamp-52e6 with the lead’s hand under the head (row 8): it came 301 s after the eyes stopped, drooped once slowly, and the light came back after; |
| Light scenes (“reading light”) and pointing the head and light (“light my book”), core skills | 1.00 | #1679 #1730 | twelve | 20 Oct 12:07, built: tried on lamp-52e6 with the lead, row 5 (reading tipped the head 148 counts down and leaned the arm in, focus too) and row 4 (pointing the light toward a named side). |
| “Look for my keys”, an installable skill | 0.50 | #1685 #1738 | first | Built and merged 19 Oct 18:01 (#1871), installable, not in the product: the desk first unless you name a place, then a few places to either side, saying what it found; |
| The light dims and goes out when nobody is there, a core skill | 1.00 | #1677 #1721 #1747 | nine | Built and tried on lamp-4ace 20 Oct 01:23 (trials.sh with Tests’ #1999 stages, lamp-skills at 782e6ef34): with nobody there the light-out skill asked the ring to fade the light over 30 s, held it while nobody was there, … |
| What you teach it (“don’t say hello in the morning”) is kept and honoured | 1.00 | #813 | seventh | Built |
| Forgets on request: that, everything, my face, and everything it came from | 0.75 | #1586 #1693 | fourth, twelve | Tried on lamp-4ace 20 Oct 03:14 on the cheaper voice (trials.sh forget, #1712 026728a70): “remember my violet umbrella…” kept, then “please forget that” forgot it (#2007 + #2025’s note: “forget it, never mind” about the … |
| Remembers what matters without being asked (“how was the wedding?”) | 0.40 | #1693 | twelve | Built 19 Oct 23:1x (#1974, Twelve), merged into #1712, off behind LAMP_MEMORY_OBSERVE: at a conversation’s end the model proposes what to keep and the memory checks it. |
| Keeps going when one part fails | 1.00 | — | — | Built: each part its own unit, restarted alone, health says how the lamp is |
| Change the Wi-Fi, name or time zone after setup, without a reset | 0.75 | #1982 #1995 #2038 | nine | 20 Oct 04:46–05:08: the Wi-Fi from the app is merged (#1995, #2033: kept only once the broker is reached, else back on the old network; |
| Hand-overs from a home lamp: Harness installed, the person signed in | 0.25 | #751 #765 #798 | seventh | Install’s “Harness at boot” is built; |
| Updates cover the whole lamp, so V3, V4 and V5 update a shipped lamp in place: units, models, the updater itself, rollback, every part’s saved data read across versions | 0.45 | #1369 #1505 | — | 20 Oct 04:49–05:29 (El): the image’s own units carried by an update (#1945), the lowest updater a build needs (#1801), models (#1879) and settings (#1913) merged; |
| The company sees its lamps in homes: health, crash and support logs, opt-in | 0.85 | #1683 #1755 | jarvis, mobile, backend | 20 Oct 15:1x, up from 0.50: run end to end on lamp-0c89 (Jarvis, Mobile, the backend): asked at setup, reports sent, the app’s no stops them within a second, and the backend deletes the record; |
| The factory’s per-lamp steps: body poses, touch pads, the update key | 0.60 | #1682 #1717 #1744 | thirteen | 20 Oct 16:0x, up from 0.50: the camera is found on any port (#2215, merged) and the factory stands the lamp for its head check (#2238, passed on 52e6); |
| Reads your mood from your face or voice | 0.25 | — | — | The owner, 07:1x: in V2. |
| Notices your activity and posture | 1.00 | — | — | Built and tried on lamp-4ace 20 Oct 01:23: with LAMP_ACTIVITY on the eyes say what the person at the desk is doing on room/activity (away in an empty room); |
| Habits and wellbeing nudges beyond Stretch break, as skills | 1.00 | — | — | 20 Oct 11:46, built: all four tried on lamp-52e6 with the lead (row 12: rest your eyes, a sip of water, stand up, wind down), each passed with its skill done. |
| Text your lamp from your phone, in the Autonomous app | 0.70 | #1396 #1673 #1726 #1742 | ten, mobile, backend, fourth | 20 Oct 11:3x, up from 0.25: on lamp-0c89 from a phone, a texted reminder was set and said at the lamp 2 min later, and a moving skill was refused from a text; |
| Email, calendar and other connectors, through hand-overs | 0.50 | #751 | — | The owner, 07:1x: through the computer’s agent. |
| Guard mode, an installable skill | 0.55 | — | — | 20 Oct 05:56: it now counts only while the ring has recently confirmed its mark (#1963, after Third’s privacy read), and its lamp trial is merged (#2045, Nine). |
| “Was anyone here while I was out?”, an installable skill | 1.00 | #1989 #2019 | nine, tests | 20 Oct 10:1x, built by the lead’s rule (merged and tried on lamp-4ace at 03:3x); |
| Music to relax, an installable skill | 0.25 | #1685 | first | Being built (Second, 19 Oct 11:2x): an original 48-second piece, only when asked |
| Faces from a photo and “who is this?”, an installable skill | 0.50 | — | — | Built and merged 20 Oct 00:50 (Ten: #1968, #1977, #1979), off behind LAMP_PHOTO_FACES: a face from a photo the app sends is kept unconfirmed until its person says yes at the lamp, asked once, deleted on a no or after 30 … |
| Light shows and effects, a core skill | 1.00 | #1730 #2028 | twelve, nine | Built and merged (#1730): party, music and calm, under a minute, no flashing, stopping within 0.2 s when someone speaks. |
| “I’m unplugging you”: it folds safely first, a core skill | 0.80 | — | — | 20 Oct 10:1x, up from 0.40: row 1 passed on lamp-52e6 at 08:56 (ask, fold in 6.7 s, head and base home, Ready, the readiness line), the lead at the arm; |
| Smoke and bad-air alerts, a core skill | 1.00 | — | — | 20 Oct 11:42, built: tried on lamp-52e6 with the lead (row 18): a smoke reading said once, the head lifted a little, an amber breath. |
| Every lamp passes an approved hardware test before it ships: stations, records, a 3–5 lamp pilot | 0.25 | #1700 #812 #1682 #1741 | first, ten, thirteen | Asked 19 Oct 08:3x. |

## Quality rows

| Row | Now | Target | Where it is |
|---|---|---|---|
| Fast answers | 0.75 | 0.85 | 18 Oct 19:2x, settled over two days of lamp-4ace (Twelve’s tables): the slow tail is short, unsure asks (one or two words: 42% and 29% over 2 s), and it is the model’s own time: our end of the person’s turn reaches it at… |
| Knows when you have finished | 0.80 | 0.85 | 20 Oct 07:25, back to 0.80: the matching end-of-speech helper is on lamp-4ace (ready 2.3 s after the voice starts); |
| Cutting in | 0.50 | 0.75 | 20 Oct 03:45 on the cheaper voice, lamp-4ace: two lines played over a long story each made the lamp pause and then go on, and after two such pauses it no longer lets anyone cut in on that story, so a third line 30 s late… |
| Stop words | 0.50 | 0.75 | 19 Oct 01:3x: why a played “Stop.” is often missed: the sound the lamp sends to be written down for stop words is the microphone before the echo canceller, where its own voice is loudest, so what comes back is mostly its… |
| Its own echo | 0.65 | 0.70 | Measured right today (#968): the canceller takes a median 54 dB off the lamp’s own voice on lamp-52e6, the same hardware; |
| Not spoken to | 0.65 | 0.90 | 20 Oct 17:2x, down from 0.75, the 16:34 played test on lamp-4ace: after a question to the lamp, its own answers kept “a conversation with the lamp is going on” alive, so it answered two people’s own talk in the next two … |
| Several people | 0.60 | 0.85 | 19 Oct 02:33, telling two people apart by voice, with its main fault fixed (#1627): it now does its job, the second person’s umbrella line answered 0 of 3 with it against 3 of 3 without; |
| Sounds that are not speech | 0.80 | 0.80 | 18 Oct 17:48, the same 48 noises now judged from the lamp’s own state recorded during the run (the 17 Oct zeros came from an older recording, so they do not compare): answered 0 times, but 12 of 48 taken for a voice and … |
| Speaking up on its own | 0.80 | 0.85 | 18 Oct 15:39, everything the lamp said unasked since midnight (Jarvis’s readout, 14 sessions): 17 reminders and timers, a median 0.98 s after they were due (16 within 5 s, one held back by the link), and 17 pieces of tas… |
| Remembers you: your name and what you told it | 0.40 | 0.90 | 20 Oct 13:2x, new row after the owner’s report that the lamp forgot their name: they said it while their face wasn’t recognised, so it was kept in the room’s memory, not theirs; |
| A good companion | 0.85 | 0.90 | 18 Oct 16:40: asked the time in London it gave Ho Chi Minh City’s (twenty to five, when London’s was 10:40); |
| Both microphones | 0.60 | 0.80 | Voice on one, the room on the other (ears). |
| Body | 0.70 | 0.80 | 17 Oct: the guarded body (#1192) on lamp-4ace since 07:34: stands for a question, turns to the voice, gestures and folds, with no refused or bad-read stop; |
| Head and eye | 0.50 | 0.80 | Leans toward you (strong eye), looks up at a tap (#849). |
| The cheaper voice (Deepgram + DeepSeek + ElevenLabs) as good as Live | 0.75 | 0.90 | 20 Oct 09:4x, up from 0.70, the first real person on it: the lead talked to lamp-52e6 for ten minutes (08:10–08:20): 32 of 32 lines answered, none held, 5 empty requests, words heard to the first answer audio a median 1.… |
| Light and all of it together | 0.80 | 0.80 | 20 Oct 01:2x–03:2x on lamp-4ace: the light-out, scenes and light-show trials all passed, the light put back each time; |
