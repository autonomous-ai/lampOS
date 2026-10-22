# Where V2 is: the build rows and the quality rows

Written by the first builder from the team's review page at 2026-10-22 07:23 (+07). The lead asked for it on #1711 (20 Oct, 16:43), so anyone can read where V2 is without waiting for the hourly post. It's a copy, updated a few times a day. The hourly figures on #1722 are the live ones.

**Build 86.6% over 72 rows. Quality 67.8% over 16 rows.**

**How a build row is counted** (the lead's rule): 1.0 when its work is merged and tried on a lamp; less while parts are missing or untried. Build is the mean of the rows. Quality rows score how well a part works today, against a target; quality is the mean of the scores.

## Build rows

| Row | Score | Issues | Owners | Where it is |
|---|---|---|---|---|
| The voice never dies (SD stalls, power cuts, midnight) | 1.00 | #784 #762 #1178 | fifth, eighth | Midnight fix and stall recorder merged; |
| Speed: tool turns and comma pauses | 1.00 | #799 #782 | second, fourth |  |
| Who is talking, and when not to answer | 1.00 | #753 #792 #800 | fourth, second | #800 behind switches |
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
| Touch strip and head pads | 0.85 | #711 #1362 #2445 #2460 | first | 21 Oct 12:33, up from 0.80: a hand held on the head put lamp-52e6 to sleep (the camera closed in 81 ms, the body folded) and a tap woke it (Fifth, #2460); |
| Two people at the lamp | 1.00 | #829 #1158 #1278 | fourth | All merged: day and night bars (#1158, 11:49) and a follow-up within 20 s told it may follow on (#1278, 13:57). |
| “Stop” over its own voice | 1.00 | #828 #1047 #1264 | fourth | Built: stop words heard over its voice (#1047, 09:11) and listening gaps in its pauses (#1264, 11:52), both off until measured. |
| Skills: say service, Reminders, Task news | 1.00 | #939 #1042 #1237 #1253 | second | All merged: the say service and the Policy gate, Reminders (with #1253’s fix), Task news as a skill (#1042, 11:44) |
| Skills: body and light shared between skills | 1.00 | #940 #1239 #1252 | — | All merged: the light side, the body’s side (#1239, 11:45) and the skills side (#1252, 12:32), behind their switches |
| Skills: creator, Stretch break, Stuffy air, Desk tutor | 1.00 | #941 #1066 #1011 #1260 | ten | All merged: Stuffy air (#1066), the reading pose (#1011, 08:58), Desk tutor’s nod and tilt (#1260, 10:23); |
| Two-minute self-test | 1.00 | #812 #983 #1243 #1245 | ten | All merged: every part checked, reported in health (#1243), run at first boot and every 6 hours (#1245), and the head’s small move (#983, 12:23) |
| One install for a new lamp | 1.00 | #811 | nine | Shown 19 Oct 15:1x on a second lamp, lamp-c0a4: build 3162 installed over V1 from a test-signed local channel, all 45 files equal to the build’s checksums, V1 stopped and kept, no reboot; |
| No internet at home | 1.00 | #830 #1242 #1256 #1247 | nine | 21 Oct 14:1x: its lamp runs passed on lamp-75c9 (Nine): the outage found with nobody speaking, the offline line once, “back online” after, and the unavailable face. |
| Everything is a skill, core and installable, no one-off code | 0.65 | #940 #938 #1667 #1696 #1698 #2643 #2654 #2674 | seventh | 21 Oct 23:39, up from 0.6: who was here’s words are merged too (#2674, Seventh), all 24 lines in that skill’s own file. |
| An owner makes their own skill | 0.60 | #941 #1698 #2424 | sixth, ten | 21 Oct 13:0x, up from 0.5: making one by voice is merged into the voice branch (#2424, off until tried): the lamp makes a skill, asks to put it on, and installs it at its person’s spoken yes. |
| A Skills Store to publish skills | 0.45 | #1698 | fourteen | 21 Oct 17:5x, up from 0.4: the catalogue the lamp reads is up on the test environment (Backend2, #1698): Lamp 2 packages listed with their address, checksum, name and version, as the lamp’s install takes them; |
| Updates over the internet, by themselves | 0.65 | #737 | migrate, second | 20 Oct 06:11: shown again on lamp-4ace on the merged code (El’s test, three cases: a good build kept, one that fails to start put back in under 2 min, one that never confirms put back after its 10 min), the lamp left exa… |
| A voice cheaper than Gemini Live, as fast and as good | 1.00 | #754 #1686 #1712 #1973 #2135 #2136 | sixth, first, fourth, ten, backend, third, eighth, twelve | 20 Oct 10:1x, built, by the lead’s rule (it works and it is fast enough are separate): on the main line since #1973, on lamp-4ace, and 32 of 32 answered for the lead on lamp-52e6. |
| A person brings their own DeepSeek and Deepgram keys, in setup or the app | 0.80 | #2094 | el, fourteen | 21 Oct 15:4x, up from 0.45: shown end to end on lamp-06c1 with its person in the app (El, #2094): each key saved from the app, checked and kept on the lamp only (0600, root), the voice then on the person’s own Deepgram a… |
| Lamp 2 in the Autonomous app | 1.00 | #1396 | ten, mobile, backend | 20 Oct 11:4x, built: the app's Skills page and name and time zone (ecm-sds-mobile#8, #9) merged into the app's v3 after their tries on lamp-0c89. |
| Talks with you naturally: turns, answers, web answers, the time | 1.00 | #694 | second, nine | Built (the conversation itself); |
| Hears its name, a bare “Lamp” too | 1.00 | #694 #725 | second, nine | Built; |
| Eye contact: turns to you and to its name, looks away to think, nods | 1.00 | #715 | fifth | Built |
| Always alive: breathing, small drift, a glance at a bang | 1.00 | #710 #714 #715 | fifth, second | Built |
| Moves with its words while it speaks | 1.00 | #933 #1067 #1365 #2030 | thirteen, fifth, third | 20 Oct 09:4x, built: #1759 (with #1067’s sway) merged into lamp-voice after its try on lamp-52e6 with the lead, so every lamp nods and sways while it speaks, sparse as the lead chose. |
| Never tips itself over | 1.00 | #1072 #704 | eighth | Built with V1’s limits; |
| One inner state (mood) every behaviour reads, and familiarity | 0.70 | #713 #1670 #1728 #1749 #1772 | fourth, ten | 21 Oct 15:4x, up from 0.6: on lamp-75c9 (Nine, #1670) the ring and Expression took Mood’s breath, and a clap quickened it: pace 1.00 to 1.15, liveliness 0 to 0.31 a minute after, easing back. |
| Joys: sways to music, watches you leave, core skills | 0.70 | #1669 #1707 #1748 | sixth | 21 Oct 14:3x: the glance at a laugh is taken out of the row (the lead on #1711: you never asked for it; |
| Dance when asked | 1.00 | #999 | jarvis | Built (the Dance skill, V1’s dance clips) |
| Bedtime and Morning as a skill, with night caps on light and volume | 0.50 | #1672 #1732 | jarvis, second | Built 19 Oct 11:1x as a core skill, off until tried (#1732, in review): dimmer, warmer, quieter and slower at night. |
| “What do you see?” and showing it a page | 1.00 | #836 #842 | seventh | Built |
| Seeing someone speak (is the face it looks at talking?) | 0.85 | #720 #1470 #2008 #2026 | fifth, ten | 21 Oct 14:08, up from 0.75: passed on lamp-52e6 with stand-ins (Sixth, #2407): a speaking stranger’s lines are the room’s and are told nothing of the known person’s; |
| The eye: looks toward you, blinks, a face for each state | 1.00 | #712 | first | Built (ring eye, asleep face) |
| Its light by voice and in the app, kept across a restart | 1.00 | #1472 #1484 #1464 #1468 | — | Built |
| Volume by voice and in the app | 1.00 | #1458 #1465 | — | Built |
| A light for booting and a “not feeling well” face for a failed part, built into the system | 0.80 | — | third | 20 Oct 11:35, up from 0.50: not feeling well tried on lamp-52e6 with the lead’s hand under the head (row 8): it came 301 s after the eyes stopped, drooped once slowly, and the light came back after; |
| Light scenes (“reading light”) and pointing the head and light (“light my book”), core skills | 1.00 | #1679 #1730 | twelve | 20 Oct 12:07, built: tried on lamp-52e6 with the lead, row 5 (reading tipped the head 148 counts down and leaned the arm in, focus too) and row 4 (pointing the light toward a named side). |
| “Look for my keys”, an installable skill | 0.70 | #1685 #1738 #2444 #2466 | first | 21 Oct 13:3x, up from 0.5: its lamp run passed on lamp-c0a4 (Thirteen, #1871): keys on the desk found, keys moved aside found after the left and the right, out of view given up gently after five looks, and a spoken stop … |
| The light dims and goes out when nobody is there, a core skill | 1.00 | #1677 #1721 #1747 | nine | Built and tried on lamp-4ace 20 Oct 01:23 (trials.sh with Tests’ #1999 stages, lamp-skills at 782e6ef34): with nobody there the light-out skill asked the ring to fade the light over 30 s, held it while nobody was there, … |
| What you teach it (“don’t say hello in the morning”) is kept and honoured | 1.00 | #813 | seventh | Built |
| Forgets on request: that, everything, my face, and everything it came from | 0.80 | #1586 #1693 | fourth, twelve | 21 Oct 03:0x, up from 0.75: “forget what I said about my bicycle” forgets only that, and nothing when nothing matches (#2277, Friday, merged, off until it is tried); |
| Remembers what matters without being asked (“how was the wedding?”) | 0.50 | #1693 #2648 #2680 | friday, twelve | 22 Oct 00:0x, up from 0.4: two pieces merged tonight (Twelve): what the lamp notices in a conversation is kept with its day and followed up once, the day after, only for the person who told it (#2648); |
| Keeps going when one part fails | 1.00 | — | — | Built: each part its own unit, restarted alone, health says how the lamp is |
| Change the Wi-Fi, name or time zone after setup, without a reset | 0.80 | #1982 #1995 #2038 | fourteen, nine | 21 Oct 12:3x, up from 0.75: the name and the time zone changed from the app on lamp-0c89, each read back and set back (Mobile, #1959); |
| Hand-overs from a home lamp: Harness installed, the person signed in | 0.75 | #751 #765 #798 #1854 #2702 | seventh, eighth | 22 Oct 04:04, up from 0.7: a sign-in question someone talked over is offered again 30 s later and then heard whole, and the yes is taken: 6 of 6 steps on lamp-4ace with a stand-in Harness (Eighth, #2702, off unless set). |
| Updates cover the whole lamp, so V3, V4 and V5 update a shipped lamp in place: units, models, the updater itself, rollback, every part’s saved data read across versions | 0.50 | #1369 #1505 #2700 | migrate, tests | 22 Oct 02:29, up from 0.45: the updater now switches itself only when its independent way back is installed and armed, and waits otherwise while the parts still update (Second, #2709, off unless set; |
| The company sees its lamps in homes: health, crash and support logs, opt-in | 0.90 | #1683 #1755 | jarvis, mobile, backend | 21 Oct 13:45, up from 0.85: the crash report too, on lamp-06c1 with its person (Jarvis, #1683), every report kept and then deleted on the app’s no, confirmed by the backend. |
| The factory’s per-lamp steps: body poses, touch pads, the update key | 0.60 | #1682 #1717 #1744 | third, thirteen | 20 Oct 16:0x, up from 0.50: the camera is found on any port (#2215, merged) and the factory stands the lamp for its head check (#2238, passed on 52e6); |
| Reads your mood from your face or voice | 0.30 | — | ten, fourth | The face half tried on lamp-75c9, 21 Oct 11:1x (Ten): a smile was taken once, small and short, as built, but the face reader never saw a held long face (0 of 54 reads), so the low side needs a better reader (#2132); |
| Notices your activity and posture | 1.00 | — | — | Built and tried on lamp-4ace 20 Oct 01:23: with LAMP_ACTIVITY on the eyes say what the person at the desk is doing on room/activity (away in an empty room); |
| Habits and wellbeing nudges beyond Stretch break, as skills | 1.00 | — | — | 20 Oct 11:46, built: all four tried on lamp-52e6 with the lead (row 12: rest your eyes, a sip of water, stand up, wind down), each passed with its skill done. |
| Text your lamp from your phone, in the Autonomous app | 0.70 | #1396 #1673 #1726 #1742 | ten, mobile, backend, fourth | 20 Oct 11:3x, up from 0.25: on lamp-0c89 from a phone, a texted reminder was set and said at the lamp 2 min later, and a moving skill was refused from a text; |
| Email, calendar and other connectors, through hand-overs | 0.60 | #751 #2740 | eighth, el | 22 Oct 05:41, up from 0.5: “what’s on my calendar today?” shown end to end on lamp-4ace with a stand-in for the computer’s agent: handed over, answered, and both made-up events told back, 6 of 6 (Eighth, #2740); |
| Guard mode, an installable skill | 0.65 | — | jarvis | 22 Oct 06:46, up from 0.55: Jarvis’s #2688 merged (74b90c317), each behind its own switch, off unless set: arrivals by a door or footsteps that a face or speech follows (LAMP_GUARD_SOUNDS), on and off from the owner’s ap… |
| “Was anyone here while I was out?”, an installable skill | 1.00 | #1989 #2019 | nine, tests | 20 Oct 10:1x, built by the lead’s rule (merged and tried on lamp-4ace at 03:3x); |
| Music to relax, an installable skill | 0.25 | #1685 | second, sixth, first | Being built (Second, 19 Oct 11:2x): an original 48-second piece, only when asked |
| Faces from a photo and “who is this?”, an installable skill | 0.80 | — | mobile, ten | Its lamp run passed on lamp-0c89, 21 Oct 11:2x (Mobile, #1968): the selfie kept unconfirmed, asked at the lamp, kept on a yes, a two-face photo refused, the person named when asked; |
| Light shows and effects, a core skill | 1.00 | #1730 #2028 | twelve, nine | Built and merged (#1730): party, music and calm, under a minute, no flashing, stopping within 0.2 s when someone speaks. |
| “I’m unplugging you”: it folds safely first, a core skill | 1.00 | #1699 #2478 | fifth | 21 Oct 11:30, built: #1699 merged as #2478 after its rebase, and Prepare to unplug is in every lamp’s default skills. |
| Smoke and bad-air alerts, a core skill | 1.00 | — | — | 20 Oct 11:42, built: tried on lamp-52e6 with the lead (row 18): a smoke reading said once, the head lifted a little, an amber breath. |
| Every lamp passes an approved hardware test before it ships: stations, records, a 3–5 lamp pilot | 0.25 | #1700 #812 #1682 #1741 | first, ten, thirteen | Asked 19 Oct 08:3x. |

## Quality rows

| Row | Now | Target | Where it is |
|---|---|---|---|
| Fast answers | 0.60 | 0.85 | 22 Oct 02:0x, down from 0.8, a correction: the 1.59 s and 1.61 s below were mostly the soft “Hm.” wait sound, not the answer. |
| Knows when you have finished | 0.80 | 0.85 | 20 Oct 07:25, back to 0.80: the matching end-of-speech helper is on lamp-4ace (ready 2.3 s after the voice starts); |
| Cutting in | 0.50 | 0.75 | 20 Oct 03:45 on the cheaper voice, lamp-4ace: two lines played over a long story each made the lamp pause and then go on, and after two such pauses it no longer lets anyone cut in on that story, so a third line 30 s late… |
| Stop words | 0.50 | 0.75 | 21 Oct 22:5x, the speech-to-text path scored on the same two recordings, on lamp-4ace through the product’s own relay: it caught none of the ten “stop” lines. |
| Its own echo | 0.65 | 0.70 | Measured right today (#968): the canceller takes a median 54 dB off the lamp’s own voice on lamp-52e6, the same hardware; |
| Not spoken to | 0.70 | 0.90 | 21 Oct 07:3x, up from 0.65: with the name rule back on and its fix for a name at a line’s start (#2376), two played tests in a row answered all 12 of the day’s lines and joined two people’s talk 1, 0, 3 and then 1, 0, 0 … |
| Several people | 0.60 | 0.85 | 21 Oct 02:3x, telling one person from another by voice, on lamp-4ace’s own microphone with the lamp answering: another voice never once sounded like the same person (56 of 56), and a person’s own voice was only mistaken … |
| Sounds that are not speech | 0.80 | 0.80 | 18 Oct 17:48, the same 48 noises now judged from the lamp’s own state recorded during the run (the 17 Oct zeros came from an older recording, so they do not compare): answered 0 times, but 12 of 48 taken for a voice and … |
| Speaking up on its own | 0.80 | 0.85 | 18 Oct 15:39, everything the lamp said unasked since midnight (Jarvis’s readout, 14 sessions): 17 reminders and timers, a median 0.98 s after they were due (16 within 5 s, one held back by the link), and 17 pieces of tas… |
| Remembers you: your name and what you told it | 0.55 | 0.90 | 21 Oct 15:3x: why the lamp forgot your name this week, likely: on the cheaper voice route the model is never told the name of the face it knows (the Live route was), and a name given mid-conversation doesn’t reach that c… |
| A good companion | 0.85 | 0.90 | 18 Oct 16:40: asked the time in London it gave Ho Chi Minh City’s (twenty to five, when London’s was 10:40); |
| Both microphones | 0.65 | 0.80 | 22 Oct 06:41, up from 0.6: step 4 passed on lamp-75c9 from the privacy test input (Nine, #694, the lead’s 05:5x ruling): after the switch closes, the room microphone stops within one 100 ms level period and gives nothing… |
| Body | 0.70 | 0.80 | 17 Oct: the guarded body (#1192) on lamp-4ace since 07:34: stands for a question, turns to the voice, gestures and folds, with no refused or bad-read stop; |
| Head and eye | 0.60 | 0.80 | 22 Oct 05:0x, up from 0.55: the arm also leans toward you during a real answer (lamp-c0a4, 21 Oct 21:4x, Thirteen: a played story request answered, the arm at its 45/60 lean while the lamp spoke, the face looked at throu… |
| The cheaper voice (Deepgram + DeepSeek + ElevenLabs) as good as Live | 0.75 | 0.90 | 21 Oct 19:4x, a gap found: in two played tests tonight (about 50 replies each) the cheaper voice went silent inside an answer 33 times, a median 0.42 s, 23 of them over 0.3 s; |
| Light and all of it together | 0.80 | 0.80 | 20 Oct 01:2x–03:2x on lamp-4ace: the light-out, scenes and light-show trials all passed, the light put back each time; |
