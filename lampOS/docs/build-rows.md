# Where V2 is: the build rows and the quality rows

Written by the first builder from the team's review page at 2026-10-22 19:47 (+07). The lead asked for it on #1711 (20 Oct, 16:43), so anyone can read where V2 is without waiting for the hourly post. It's a copy, updated a few times a day. The hourly figures on #1722 are the live ones.

**Build 89.8% over 72 rows. Quality 61.9% over 16 rows.**

**How a build row is counted** (the lead's rule): 1.0 when its work is merged and tried on a lamp; less while parts are missing or untried. Build is the mean of the rows. Quality rows score how well a part works today, against a target; quality is the mean of the scores.

**Not a ship gate:** a row marked so stays in V2 and in the build figure, but a lamp ships with it off if its runs haven't passed by then (the lead, 22 Oct on #1711).

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
| The first five minutes | 0.90 | #841 #2843 #2844 | seventh | 22 Oct 10:2x, up from 0.8: row 10 passed whole on lamp-52e6 from #2526’s code (Seventh, #2843), merged at 10:01 (f71a62f55). |
| Speaks first when it should (the Policy) | 1.00 | #752 | second |  |
| Each person’s memory kept apart | 1.00 | #764 | seventh |  |
| Sleep, and waking on its name | 1.00 | #749 #714 | fourth, second |  |
| Reminders, timers, memory across days | 1.00 | #748 #743 | — |  |
| Setup for a home, 5 s reset | 0.90 | #806 #1209 #2861 | first | 20 Oct 13:3x, up from 0.65: lamp-06c1 done end to end with its person (El, #806): the 5 s reset wiped it and rebooted, it was set up again from the app (connected, the link and keys kept 600), “Hey Lamp” heard and answer… |
| Touch strip and head pads | 0.90 | #711 #1362 #2445 #2460 #2857 | first | 22 Oct 12:2x, up from 0.85: the head pads only now. |
| Two people at the lamp | 1.00 | #829 #1158 #1278 | fourth | All merged: day and night bars (#1158, 11:49) and a follow-up within 20 s told it may follow on (#1278, 13:57). |
| “Stop” over its own voice | 1.00 | #828 #1047 #1264 | fourth | Built: stop words heard over its voice (#1047, 09:11) and listening gaps in its pauses (#1264, 11:52), both off until measured. |
| Skills: say service, Reminders, Task news | 1.00 | #939 #1042 #1237 #1253 | second | All merged: the say service and the Policy gate, Reminders (with #1253’s fix), Task news as a skill (#1042, 11:44) |
| Skills: body and light shared between skills | 1.00 | #940 #1239 #1252 | — | All merged: the light side, the body’s side (#1239, 11:45) and the skills side (#1252, 12:32), behind their switches |
| Skills: creator, Stretch break, Stuffy air, Desk tutor | 1.00 | #941 #1066 #1011 #1260 | ten | All merged: Stuffy air (#1066), the reading pose (#1011, 08:58), Desk tutor’s nod and tilt (#1260, 10:23); |
| Two-minute self-test | 1.00 | #812 #983 #1243 #1245 | ten | All merged: every part checked, reported in health (#1243), run at first boot and every 6 hours (#1245), and the head’s small move (#983, 12:23) |
| One install for a new lamp | 1.00 | #811 | nine | Shown 19 Oct 15:1x on a second lamp, lamp-c0a4: build 3162 installed over V1 from a test-signed local channel, all 45 files equal to the build’s checksums, V1 stopped and kept, no reboot; |
| No internet at home | 1.00 | #830 #1242 #1256 #1247 | nine | 21 Oct 14:1x: its lamp runs passed on lamp-75c9 (Nine): the outage found with nobody speaking, the offline line once, “back online” after, and the unavailable face. |
| Everything is a skill, core and installable, no one-off code | 0.70 | #940 #938 #1667 #1696 #1698 #2643 #2654 #2674 #2822 #2823 #2824 #2825 #2826 | seventh | 22 Oct 15:1x, up from 0.65: look for things’ walk, its note, places, order and words now live in its skill’s file, read by the voice from lamp-skills’ tools file (Thirteen, #2916, a16545d92; |
| An owner makes their own skill | 0.70 | #941 #1698 #2424 #2811 #2813 #2814 #2815 #2816 | sixth, ten | 22 Oct 15:1x, up from 0.65: a skill typed in the app is taken on the voice’s own socket, handed over as its brief and told aloud (LAMP_APP_MAKE_SKILL, blue’s #2887 carried as #2935, merged 15:00 as 8613304e4). |
| A Skills Store to publish skills | 0.55 | #1698 #2801 #2802 | fourteen | 22 Oct 11:4x, up from 0.45: a sent skill is checked and waits for review before it is listed, shown on the test environment (Backend3, #2802): its files, checksums and permissions shown in the lamp’s words, a changed byt… |
| Updates over the internet, by themselves | 0.65 | #737 #1812 | migrate, second | 20 Oct 06:11: shown again on lamp-4ace on the merged code (El’s test, three cases: a good build kept, one that fails to start put back in under 2 min, one that never confirms put back after its 10 min), the lamp left exa… |
| A voice cheaper than Gemini Live, as fast and as good | 1.00 | #754 #1686 #1712 #1973 #2135 #2136 | sixth, first, fourth, ten, backend, third, eighth, twelve | 20 Oct 10:1x, built, by the lead’s rule (it works and it is fast enough are separate): on the main line since #1973, on lamp-4ace, and 32 of 32 answered for the lead on lamp-52e6. |
| A person brings their own DeepSeek and Deepgram keys, in setup or the app | 0.90 | #2094 #2845 #2846 | el, fourteen | 22 Oct 18:0x, up from 0.8: setup’s own-keys step passed on a freshly reset lamp-06c1 (El, #2845 closed). |
| Lamp 2 in the Autonomous app | 1.00 | #1396 | ten, mobile, backend | 20 Oct 11:4x, built: the app's Skills page and name and time zone (ecm-sds-mobile#8, #9) merged into the app's v3 after their tries on lamp-0c89. |
| Talks with you naturally: turns, answers, web answers, the time | 1.00 | #694 | second, nine | Built (the conversation itself); |
| Hears its name, a bare “Lamp” too | 1.00 | #694 #725 | second, nine | Built; |
| Eye contact: turns to you and to its name, looks away to think, nods | 1.00 | #715 | fifth | Built |
| Always alive: breathing, small drift, a glance at a bang | 1.00 | #710 #714 #715 | fifth, second | Built |
| Moves with its words while it speaks | 1.00 | #933 #1067 #1365 #2030 | thirteen, fifth, third | 20 Oct 09:4x, built: #1759 (with #1067’s sway) merged into lamp-voice after its try on lamp-52e6 with the lead, so every lamp nods and sways while it speaks, sparse as the lead chose. |
| Never tips itself over | 1.00 | #1072 #704 | eighth | Built with V1’s limits; |
| One inner state (mood) every behaviour reads, and familiarity *(not a ship gate)* | 0.70 | #713 #1670 #1728 #1749 #1772 #2829 #2830 #2831 | fourth, ten | 21 Oct 15:4x, up from 0.6: on lamp-75c9 (Nine, #1670) the ring and Expression took Mood’s breath, and a clap quickened it: pace 1.00 to 1.15, liveliness 0 to 0.31 a minute after, easing back. |
| Joys: sways to music, watches you leave, core skills | 0.70 | #1669 #1707 #1748 #2832 #2833 #2834 | sixth | 21 Oct 14:3x: the glance at a laugh is taken out of the row (the lead on #1711: you never asked for it; |
| Dance when asked | 1.00 | #999 | jarvis | Built (the Dance skill, V1’s dance clips) |
| Bedtime and Morning as a skill, with night caps on light and volume | 0.65 | #1672 #1732 #2803 #2804 | jarvis, second | 22 Oct, up from 0.5: evening, the glow, bedtime twice and a night sound’s end passed on lamp-06c1 (Jarvis, 21–22 Oct). |
| “What do you see?” and showing it a page | 1.00 | #836 #842 | seventh | Built |
| Seeing someone speak (is the face it looks at talking?) | 0.85 | #720 #1470 #2008 #2026 #2859 #2860 | fifth, ten | 21 Oct 14:08, up from 0.75: passed on lamp-52e6 with stand-ins (Sixth, #2407): a speaking stranger’s lines are the room’s and are told nothing of the known person’s; |
| The eye: looks toward you, blinks, a face for each state | 1.00 | #712 | first | Built (ring eye, asleep face) |
| Its light by voice and in the app, kept across a restart | 1.00 | #1472 #1484 #1464 #1468 | — | Built |
| Volume by voice and in the app | 1.00 | #1458 #1465 | — | Built |
| A light for booting and a “not feeling well” face for a failed part, built into the system | 0.90 | #2847 #2848 | third | 22 Oct 18:0x, up from 0.85: the waking face passed on cold power-ons, 3 of 3 on lamp-06c1 (El, #2847 closed). |
| Light scenes (“reading light”) and pointing the head and light (“light my book”), core skills | 1.00 | #1679 #1730 | twelve | 20 Oct 12:07, built: tried on lamp-52e6 with the lead, row 5 (reading tipped the head 148 counts down and leaned the arm in, focus too) and row 4 (pointing the light toward a named side). |
| “Look for my keys”, an installable skill | 0.85 | #1685 #1738 #2444 #2466 #2835 #2904 | first | 22 Oct 17:5x, up from 0.8: a push on the lamp ends its search at once, a real push and a test input each ending it on lamp-c0a4 (Thirteen, #2983 carried as #2990, fe49ed3c4). |
| The light dims and goes out when nobody is there, a core skill | 1.00 | #1677 #1721 #1747 | nine | Built and tried on lamp-4ace 20 Oct 01:23 (trials.sh with Tests’ #1999 stages, lamp-skills at 782e6ef34): with nobody there the light-out skill asked the ring to fade the light over 30 s, held it while nobody was there, … |
| What you teach it (“don’t say hello in the morning”) is kept and honoured | 1.00 | #813 | seventh | Built |
| Forgets on request: that, everything, my face, and everything it came from | 0.90 | #1586 #1693 #2849 #2850 | fourth, twelve | 22 Oct 15:1x, up from 0.85: “forget everything” passed on lamp-c0a4 with two known faces (Twelve, #2849, closed): one thing forgotten alone, all of A’s gone across a restart, B’s and the room’s untouched. |
| Remembers what matters without being asked (“how was the wedding?”) | 0.70 | #1693 #2648 #2680 #2805 #2806 #2807 #2808 | friday, twelve | 22 Oct 17:3x, up from 0.65: a dated event’s follow-up is worded in memory’s note and marked when the lamp asks it (LAMP_MEMORY_EVENTS, Twelve’s #2894, carried as #2980, a8564b52e). |
| Keeps going when one part fails | 1.00 | — | — | Built: each part its own unit, restarted alone, health says how the lamp is |
| Change the Wi-Fi, name or time zone after setup, without a reset | 0.95 | #1982 #1995 #2038 #2851 #2852 #2853 | fourteen, nine | 22 Oct 16:1x, up from 0.9: the settings and Wi-Fi helpers take a request only from lamp-cloud’s unit (#2696), and install puts their units on every product lamp (Sixth’s #2926, carried as #2955). |
| Hand-overs from a home lamp: Harness installed, the person signed in | 0.80 | #751 #765 #798 #1854 #2702 #2838 #2839 #2840 #2841 #2842 | seventh, eighth | 22 Oct 09:10, up from 0.75: a task’s finish is told after the agent’s question, on lamp-75c9 (Eighth, #2543). |
| Updates cover the whole lamp, so V3, V4 and V5 update a shipped lamp in place: units, models, the updater itself, rollback, every part’s saved data read across versions | 0.55 | #1369 #1505 #2700 #2809 #2810 | migrate, tests | 22 Oct 12:4x, up from 0.5: three more parts’ kept files are read by every later build’s tests: controls (#2784), lamp-cloud’s rotated login (#2786) and records (#2787) (Migrate). |
| The company sees its lamps in homes: health, crash and support logs, opt-in | 0.95 | #1683 #1755 #2862 | jarvis, mobile, backend | 22 Oct 12:46, up from 0.9: the health report lists the product parts that have failed, under the same consent (Second, #2764). |
| The factory’s per-lamp steps: body poses, touch pads, the update key | 0.70 | #1682 #1717 #1744 #2817 #2818 | third, thirteen | 22 Oct 16:3x, up from 0.65: the factory steps start the body before the stand, so a whole run needs no person between steps (Fifth, #2869, ef1eb0cf2). |
| Reads your mood from your face or voice *(not a ship gate)* | 0.35 | #2796 #2797 #2798 #2799 #2800 | ten, fourth | 22 Oct 16:1x, up from 0.3: the eyes read a face’s expression only on the person’s kept yes, and the face mesh runs on its own worker (Sixth’s #2872, carried as #2953); |
| Notices your activity and posture | 1.00 | — | — | Built and tried on lamp-4ace 20 Oct 01:23: with LAMP_ACTIVITY on the eyes say what the person at the desk is doing on room/activity (away in an empty room); |
| Habits and wellbeing nudges beyond Stretch break, as skills | 1.00 | — | — | 20 Oct 11:46, built: all four tried on lamp-52e6 with the lead (row 12: rest your eyes, a sip of water, stand up, wind down), each passed with its skill done. |
| Text your lamp from your phone, in the Autonomous app | 0.70 | #1396 #1673 #1726 #1742 #2836 | ten, mobile, backend, fourth | 20 Oct 11:3x, up from 0.25: on lamp-0c89 from a phone, a texted reminder was set and said at the lamp 2 min later, and a moving skill was refused from a text; |
| Email, calendar and other connectors, through hand-overs | 0.70 | #751 #2740 #2820 #2821 | eighth, el | 22 Oct 18:0x, up from 0.6: an email ask handed over and told back, 6 of 6 on lamp-06c1 with the stand-in (El, #2820 closed), as the calendar was on 4ace. |
| Guard mode, an installable skill | 0.85 | #2562 #2827 #2828 | jarvis | 22 Oct 15:1x, up from 0.75: merged, each off unless set: it names only a known person who said yes to being named (LAMP_GUARD_NAMES, #2562, 645d2f554), and it reads a return or a thank-you as off (LAMP_GUARD_WORDS_V2, #2… |
| “Was anyone here while I was out?”, an installable skill | 1.00 | #1989 #2019 | nine, tests | 20 Oct 10:1x, built by the lead’s rule (merged and tried on lamp-4ace at 03:3x); |
| Music to relax, an installable skill | 0.45 | #1685 #2792 #2793 | second, sixth, first | 22 Oct 11:35, up from 0.4: while requested music plays, the lamp shows it is busy, never resting (Second, #2879). |
| Faces from a photo and “who is this?”, an installable skill | 0.85 | #2854 #2855 #2856 | mobile, ten | 22 Oct 15:1x, up from 0.8: a face from a photo is kept on its yes with the prints the lamp took of it, not the selfie’s alone, so it is named away from the middle too (Mobile, #2854, dda47c270). |
| Light shows and effects, a core skill | 1.00 | #1730 #2028 | twelve, nine | Built and merged (#1730): party, music and calm, under a minute, no flashing, stopping within 0.2 s when someone speaks. |
| “I’m unplugging you”: it folds safely first, a core skill | 1.00 | #1699 #2478 | fifth | 21 Oct 11:30, built: #1699 merged as #2478 after its rebase, and Prepare to unplug is in every lamp’s default skills. |
| Smoke and bad-air alerts, a core skill | 1.00 | — | — | 20 Oct 11:42, built: tried on lamp-52e6 with the lead (row 18): a smoke reading said once, the head lifted a little, an amber breath. |
| Every lamp passes an approved hardware test before it ships: stations, records, a 3–5 lamp pilot | 0.35 | #1700 #812 #1682 #1741 #2794 #2795 | first, ten, thirteen | 22 Oct 15:1x, up from 0.25: lamp-station judges whether a kept record is complete against a matrix version, every expected check passed or failed by its limits and none missing (050b74011), and compares failures and inco… |

## Quality rows

| Row | Now | Target | Where it is |
|---|---|---|---|
| Fast answers | 0.60 | 0.85 | 22 Oct 02:0x, down from 0.8, a correction: the 1.59 s and 1.61 s below were mostly the soft “Hm.” wait sound, not the answer. |
| Knows when you have finished | 0.45 | 0.85 | 22 Oct 19:3x, down from 0.6: in the owner’s 19:04 test 5 of 28 lines ended at a mid-sentence pause or a follow-up begun before the answer, and the first half was let go (a one-person join and #3026 are being built). |
| Cutting in | 0.50 | 0.75 | 20 Oct 03:45 on the cheaper voice, lamp-4ace: two lines played over a long story each made the lamp pause and then go on, and after two such pauses it no longer lets anyone cut in on that story, so a third line 30 s late… |
| Stop words | 0.50 | 0.75 | 21 Oct 22:5x, the speech-to-text path scored on the same two recordings, on lamp-4ace through the product’s own relay: it caught none of the ten “stop” lines. |
| Its own echo | 0.65 | 0.70 | Measured right today (#968): the canceller takes a median 54 dB off the lamp’s own voice on lamp-52e6, the same hardware; |
| Not spoken to | 0.55 | 0.90 | 22 Oct 19:3x, down from 0.65: in the owner’s 19:04 test 4 of their lines were held without the name, the office keeping the room busy (#2967 lets a quiet room take a turned face again). |
| Several people | 0.65 | 0.85 | 22 Oct 07:38, up from 0.6: both halves of the going-on rows on lamp-52e6 with Mac voices and screen faces (Seventh). |
| Sounds that are not speech | 0.80 | 0.80 | 18 Oct 17:48, the same 48 noises now judged from the lamp’s own state recorded during the run (the 17 Oct zeros came from an older recording, so they do not compare): answered 0 times, but 12 of 48 taken for a voice and … |
| Speaking up on its own | 0.80 | 0.85 | 18 Oct 15:39, everything the lamp said unasked since midnight (Jarvis’s readout, 14 sessions): 17 reminders and timers, a median 0.98 s after they were due (16 within 5 s, one held back by the link), and 17 pieces of tas… |
| Remembers you: your name and what you told it | 0.55 | 0.90 | 22 Oct 19:3x, down from 0.65: “what’s my name?” failed for the owner at 19:04 (their face record refused “remember my face” twice at 15:33; |
| A good companion | 0.65 | 0.90 | 22 Oct 17:4x, up from 0.6: in the owner’s try the weather was looked up for Hanoi and then the lamp’s place, tomorrow, and the time called for London: the functions the bound had left out are back (#2961, merged a0705aee… |
| Both microphones | 0.65 | 0.80 | 22 Oct 06:41, up from 0.6: step 4 passed on lamp-75c9 from the privacy test input (Nine, #694, the lead’s 05:5x ruling): after the switch closes, the room microphone stops within one 100 ms level period and gives nothing… |
| Body | 0.70 | 0.80 | 17 Oct: the guarded body (#1192) on lamp-4ace since 07:34: stands for a question, turns to the voice, gestures and folds, with no refused or bad-read stop; |
| Head and eye | 0.60 | 0.80 | 22 Oct 05:0x, up from 0.55: the arm also leans toward you during a real answer (lamp-c0a4, 21 Oct 21:4x, Thirteen: a played story request answered, the arm at its 45/60 lean while the lamp spoke, the face looked at throu… |
| The cheaper voice (Deepgram + DeepSeek + ElevenLabs) as good as Live | 0.45 | 0.90 | 22 Oct 19:3x, down from 0.5: the owner at 19:04, “terrible … missed most of my lines”: about 13 of 28 lines answered. |
| Light and all of it together | 0.80 | 0.80 | 20 Oct 01:2x–03:2x on lamp-4ace: the light-out, scenes and light-show trials all passed, the light put back each time; |
