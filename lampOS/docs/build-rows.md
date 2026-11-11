# Where V2 is: the build rows and the quality rows

Written by the first builder from the team's review page at 2026-11-11 12:37 (+07). The lead asked for it on #1711 (20 Oct, 16:43), so anyone can read where V2 is without waiting for the hourly post. It's a copy, updated a few times a day. The hourly figures on #1711, posted by the PM (Faye) since 4 Nov, are the live ones.

**Build 93.0% over 75 rows. Quality 69.4% over 17 rows.** For later (mood, and texting from the app): 46.8% over 4 rows, not in the build figure.

**How a build row is counted** (the lead's rule): 1.0 when its work is merged and tried on a lamp; less while parts are missing or untried. Build is the mean of the rows. Quality rows score how well a part works today, against a target; quality is the mean of the scores.

**Mood, for later:** the mood rows stay in V2 but out of the build figure, which counts the rows that ship. No day slot or new work goes into them until those reach 100% (the lead, 23 Oct on #1711). On 22 Oct the first builder had kept them in the figure as rows that aren't ship gates.

## Build rows

| Row | Score | Issues | Owners | Where it is |
|---|---|---|---|---|
| The voice never dies (SD stalls, power cuts, midnight) | 1.00 | #784 #762 #1178 | fifth, eighth | Midnight fix and stall recorder merged; |
| Speed: tool turns and comma pauses | 1.00 | #799 #782 | second, fourth |  |
| Who is talking, and when not to answer | 1.00 | #753 #792 #800 | fourth, second | #800 behind switches |
| Privacy switch closes both microphones | 1.00 | #795 | — |  |
| Faces: remember, hello by name | 0.95 | #759 #775 | first | 11 Nov 12:3x, up from 0.93: on stable 11716 (09:18) with LAMP_FACE_REFOUND on (#5916): on lamp-52e6 the lead turned away and stepped out, came back, asked his name, and the lamp knew him at once and kept his words as his… |
| Hand work to Claude Code or Codex, and its news | 1.00 | #751 #765 #798 #733 | seventh |  |
| Backup voice when Gemini is down | 1.00 | #778 #754 | sixth | The relay chat it uses has been down since 02:29 |
| Gives way to a hand; wakes after power | 1.00 | #780 #790 #702 #703 #701 | fifth | Arm-goal guard merged (#1192) |
| “Lamp, that was weird” marks a moment | 1.00 | #803 | sixth |  |
| The first five minutes | 0.95 | #841 #2843 #2844 | seventh, el | 25 Oct 10:5x, up from 0.9: on in the product (#3435, train 114). |
| Speaks first when it should (the Policy) | 1.00 | #752 | second |  |
| Each person’s memory kept apart | 1.00 | #764 | seventh |  |
| Sleep, and waking on its name | 1.00 | #749 #714 | fourth, second |  |
| Reminders, timers, memory across days | 1.00 | #748 #743 | — | 10 Nov 12:3x, back up from 0.95: #5498’s fix (a stale oldest reminder no longer silences the rest) has been on stable since 10686, and #5879 (a plain reminders question always reads the real reminders, `list_reminders`) … |
| Setup for a home, 5 s reset | 0.95 | #806 #1209 #2861 #3579 | first, el | 26 Oct 20:3x, up from 0.85: shown on lamp-52e6 with build 7071 (the lead’s agent, 20:10–20:28). |
| A safe shutdown from the app and a long press | 1.00 | #3697 #3698 #3713 #3735 | fifth, mobile | 2 Nov 16:0x, up from 0.85, built: on in the product since #4909, stable in 9528 (16:0x). |
| Touch strip and head pads | 0.95 | #711 #1362 #2445 #2460 #2857 | first, fifth | 27 Oct 15:1x: found on the tester’s 7071 lamps: a hand kept 3 s put the lamp to sleep unsaid, in onboarding’s touch and giving-way steps, and they reset it (#3747, #3749). |
| Two people at the lamp | 1.00 | #829 #1158 #1278 | fourth | All merged: day and night bars (#1158, 11:49) and a follow-up within 20 s told it may follow on (#1278, 13:57). |
| “Stop” over its own voice | 1.00 | #828 #1047 #1264 | fourth | Built: stop words heard over its voice (#1047, 09:11) and listening gaps in its pauses (#1264, 11:52), both off until measured. |
| Skills: say service, Reminders, Task news | 1.00 | #939 #1042 #1237 #1253 | second | All merged: the say service and the Policy gate, Reminders (with #1253’s fix), Task news as a skill (#1042, 11:44) |
| Skills: body and light shared between skills | 1.00 | #940 #1239 #1252 | seventh | All merged: the light side, the body’s side (#1239, 11:45) and the skills side (#1252, 12:32), behind their switches |
| Skills: creator, Stretch break, Stuffy air, Desk tutor | 1.00 | #941 #1066 #1011 #1260 | ten | All merged: Stuffy air (#1066), the reading pose (#1011, 08:58), Desk tutor’s nod and tilt (#1260, 10:23); |
| Two-minute self-test | 1.00 | #812 #983 #1243 #1245 | ten | All merged: every part checked, reported in health (#1243), run at first boot and every 6 hours (#1245), and the head’s small move (#983, 12:23) |
| One install for a new lamp | 1.00 | #811 | nine | Shown 19 Oct 15:1x on a second lamp, lamp-c0a4: build 3162 installed over V1 from a test-signed local channel, all 45 files equal to the build’s checksums, V1 stopped and kept, no reboot; |
| No internet at home | 1.00 | #830 #1242 #1256 #1247 | nine | 21 Oct 14:1x: its lamp runs passed on lamp-75c9 (Nine): the outage found with nobody speaking, the offline line once, “back online” after, and the unavailable face. |
| Everything is a skill, core and installable, no one-off code | 0.85 | #940 #938 #1667 #1696 #1698 #2643 #2654 #2674 #2822 #2823 #2824 #2825 #2826 | seventh, fifth, thirteen, fourth, fourteen | 24 Oct 08:4x, up from 0.72: this row’s note was stale. |
| An owner makes their own skill | 0.95 | #941 #1698 #2424 #2811 #2813 #2814 #2815 #2816 | sixth, ten, subdarren, blue, fourteen, seventh | 6 Nov 17:3x, up from 0.90: made by voice and typed in the app on 9adf at 10300, a file upload on 10395, each on product units. |
| A Skills Store to publish skills | 1.00 | #1698 #2801 #2802 | fourteen, mobile, backend3 | 31 Oct 11:1x, built: Take back ran too (Fourteen, 10:29, lamp-9adf on 9063): a made skill shared, approved, taken back by its owner and gone from the Store’s queue and downloads (Backend3). |
| Updates over the internet, by themselves | 0.90 | #737 #1812 | migrate, second | 6 Nov 17:3x, down from 0.95: lamp-06c1 sat on 10343 for five hours with a writer put back (#5539), freed only by hand; |
| A voice cheaper than Gemini Live, as fast and as good | 1.00 | #754 #1686 #1712 #1973 #2135 #2136 | sixth, fourth, first, backend, third | 20 Oct 10:1x, built, by the lead’s rule (it works and it is fast enough are separate): on the main line since #1973, on lamp-4ace, and 32 of 32 answered for the lead on lamp-52e6. |
| A person brings their own DeepSeek and Deepgram keys, in setup or the app | 0.80 | #2094 #2845 #2846 | fourteen, backend2, el | 6 Nov 17:3x, down from 0.90: LAMP_OWN_KEYS is off in the product, its last run was by hand on 21 Oct, and its fix waits on the broker team. |
| Lamp 2 in the Autonomous app | 1.00 | #1396 | mobile, backend | 20 Oct 11:4x, built: the app's Skills page and name and time zone (ecm-sds-mobile#8, #9) merged into the app's v3 after their tries on lamp-0c89. |
| Speaks English, Japanese, Chinese or Korean, the owner’s choice in setup or the app | 0.75 | #3975 #4473 #4474 | seventh, mobile | 6 Nov 17:3x, up from 0.70: language changed from the app on 52e6 at 10441, ja and ko walks played at 9424 (rows 7/11 and 9/11), kept through an update. |
| Talks with you naturally: turns, answers, web answers, the time | 0.92 | #694 | nine | 24 Oct 06:1x, up from 0.9: the two-turn fault’s fix is merged behind LAMP_SAME_SPEAKER_CARRIES, off (#3296, Sixth, train 62, df6830381). |
| Hears its name, a bare “Lamp” too | 1.00 | #694 #725 | nine | Built; |
| Eye contact: turns to you and to its name, looks away to think, nods | 1.00 | #715 | fifth | Built |
| Always alive: breathing, small drift, a glance at a bang | 1.00 | #710 #714 #715 | fifth, second | Built |
| Moves with its words while it speaks | 1.00 | #933 #1067 #1365 #2030 | thirteen | 20 Oct 09:4x, built: #1759 (with #1067’s sway) merged into lamp-voice after its try on lamp-52e6 with the lead, so every lamp nods and sways while it speaks, sparse as the lead chose. |
| Never tips itself over | 1.00 | #1072 #704 | hw-first | Built with V1’s limits; |
| One inner state (mood) every behaviour reads, and familiarity *(for later: not in the build figure)* | 0.72 | #713 #1670 #1728 #1749 #1772 #2829 #2830 #2831 | fourth, sixth, seventh | 22 Oct 22:0x, up from 0.7: a say’s record keeps who it told and the familiar level (#3056 merged, Seventh), so #2831’s last bar can be read on 52e6. |
| Music matched to your mood (for later) *(for later: not in the build figure)* | 0.00 | #3504 | — | 27 Oct 08:1x, new, from the lead’s 07:58 list: music to relax plays one 48 s piece. |
| Shows its feelings in body and light, as V1’s emotions did | 0.50 | #3504 #3691 #4020 | jarvis | 29 Oct 06:3x, up from 0: this row was stale. |
| Joys: sways to music, watches you leave, core skills | 0.70 | #1669 #1707 #1748 #2832 #2833 #2834 | sixth, fifth, nine | 6 Nov 17:3x, down from 0.80: music sway has nothing to read: LAMP_EARS_CLASSES is in no product unit; |
| Dance when asked | 1.00 | #999 | jarvis | Built (the Dance skill, V1’s dance clips) |
| Bedtime and Morning as a skill, with night caps on light and volume | 0.85 | #1672 #1732 #2803 #2804 | jarvis | 11 Nov 12:3x, up from 0.75: on for customers in stable 11716 with its night caps (#5771). |
| “What do you see?” and showing it a page | 1.00 | #836 #842 | seventh | Built |
| Seeing someone speak (is the face it looks at talking?) | 0.94 | #720 #1470 #2008 #2026 #2859 #2860 | fifth, sixth | 24 Oct 09:4x, up from 0.92: built and merged, all off unless set: with LAMP_EYES_SPEAKING_DROP_OUTLIER the eyes drop one misread look from a speaking window instead of voiding it (#3313, #3317), and with LAMP_EYES_SPEAKI… |
| The eye: looks toward you, blinks, a face for each state | 1.00 | #712 | first | Built (ring eye, asleep face) |
| Its light by voice and in the app, kept across a restart | 1.00 | #1472 #1484 #1464 #1468 | — | Built |
| Volume by voice and in the app | 1.00 | #1458 #1465 | — | Built |
| A light for booting and a “not feeling well” face for a failed part, built into the system | 0.95 | #2847 #2848 | third, el, tests | 26 Oct 08:0x, up from 0.9: the waking face is on in the product unit (#3518, #2848). |
| Light scenes (“reading light”) and pointing the head and light (“light my book”), core skills | 0.85 | #1679 #1730 | twelve | 6 Nov 21:5x, down from 1.0, as Build now counts what a customer gets (the lead’s agent for the lead, 21:34): built and tried on a lamp, but `light-scenes` and `point-light` are installed off on customer lamps (install::P… |
| “Look for my keys”, an installable skill | 0.90 | #1685 #1738 #2444 #2466 #2835 #2904 | first, thirteen | 6 Nov 17:3x, up from 0.85: installed from the Store through the app on 9adf at 10226, and it looked and answered. |
| The light dims and goes out when nobody is there, a core skill | 0.85 | #1677 #1721 #1747 | nine | 6 Nov 21:5x, down from 1.0, as Build now counts what a customer gets (the lead’s agent for the lead, 21:34): it is built and was tried on a lamp, but `light-out` is installed off on customer lamps (install::PRODUCT_SKILL… |
| What you teach it (“don’t say hello in the morning”) is kept and honoured | 1.00 | #813 | seventh | Built |
| Forgets on request: that, everything, my face, and everything it came from | 0.97 | #1586 #1693 #2849 #2850 | fourth, friday, twelve | 23 Oct 10:1x, up from 0.95: after “forget everything”, the room’s lines about that person are left out for their day (#3142, train 29). |
| Remembers what matters without being asked (“how was the wedding?”) | 0.90 | #1693 #2648 #2680 #2805 #2806 #2807 #2808 | friday, twelve, thirteen | 26 Oct 09:5x, up from 0.85: the product keeps what matters without being asked, dated things and one open thread, for the person seen saying it only, and “forget everything about me” reaches it (#3450, merged 08:4x: OBSE… |
| Keeps going when one part fails | 1.00 | — | — | Built: each part its own unit, restarted alone, health says how the lamp is |
| Change the Wi-Fi, name or time zone after setup, without a reset | 0.85 | #1982 #1995 #2038 #2851 #2852 #2853 | fourteen, mobile, migrate | 6 Nov 17:3x, down from 0.95: the name and time zone work; |
| Hand-overs from a home lamp: Harness installed, the person signed in | 0.95 | #751 #765 #798 #1854 #2702 #2838 #2839 #2840 #2841 #2842 | eighth, tests, subdarren, blue | 6 Nov 11:1x, up from 0.88, on stable 10441: lamp-c0a4 (#5228, SubDarren, 10:31–10:40) went from signed out to a spoken sign-in, the person approving the code in Harness, the daemon started, a hand-over to Claude Code by … |
| Updates cover the whole lamp, so V3, V4 and V5 update a shipped lamp in place: units, models, the updater itself, rollback, every part’s saved data read across versions | 1.00 | #1369 #1505 #2700 #2809 #2810 | migrate, tests, second | 9 Nov 10:2x, up from 0.85 (scored by the lead’s agent for the lead): #5476’s and #5539’s fixes are in 10686, 10749 and 10824, and lamp-52e6 played two real update rounds across builds with them, 10686 to 10749 (8 Nov) an… |
| The company sees its lamps in homes: health, crash and support logs, opt-in | 0.95 | #1683 #1755 #2862 | jarvis, mobile, backend, tests | 22 Oct 12:46, up from 0.9: the health report lists the product parts that have failed, under the same consent (Second, #2764). |
| The factory’s per-lamp steps: body poses, touch pads, the update key | 0.95 | #1682 #1717 #1744 #2817 #2818 #3578 | third, thirteen, first, fifth | 26 Oct 20:3x, up from 0.9: #3578’s step 4 on lamp-52e6 with 7071: the self-test passed, “the lamp is stopped, every part finished” for 23 units, then the reset and the reboot into setup, with nothing left behind. |
| Reads your mood from your face or voice *(for later: not in the build figure)* | 0.45 | #2796 #2797 #2798 #2799 #2800 | ten, fourth, sixth, fifth | 23 Oct 02:1x, up from 0.35: the product units carry the face reader, Blendshape alone (#2953, #2799 closed), and onboarding’s one ask whether the lamp may notice how a face seems, keeping only a yes, is merged (#3082, tr… |
| Notices your activity and posture | 1.00 | — | — | Built and tried on lamp-4ace 20 Oct 01:23: with LAMP_ACTIVITY on the eyes say what the person at the desk is doing on room/activity (away in an empty room); |
| Habits and wellbeing nudges beyond Stretch break, as skills | 1.00 | — | — | 20 Oct 11:46, built: all four tried on lamp-52e6 with the lead (row 12: rest your eyes, a sip of water, stand up, wind down), each passed with its skill done. |
| Text your lamp from your phone, in the Autonomous app *(for later: not in the build figure)* | 0.70 | #1396 #1673 #1726 #1742 #2836 | mobile, backend, fourth | 20 Oct 11:3x, up from 0.25: on lamp-0c89 from a phone, a texted reminder was set and said at the lamp 2 min later, and a moving skill was refused from a text; |
| Email, calendar and other connectors, through hand-overs | 0.85 | #751 #2740 #2820 #2821 | eighth, el, subdarren | 11 Nov 12:3x, up from 0.80: on stable 11716, a waiting mail, event or Drive write goes out on a natural “yes” (#6009); |
| Guard mode, an installable skill | 0.75 | #2562 #2827 #2828 | jarvis, nine | 6 Nov 17:3x, down from 0.85: it is in neither the product list nor the Store, its switches are off, and the 9adf run was on a drop-in. |
| “Was anyone here while I was out?”, an installable skill | 0.85 | #1989 #2019 | — | 6 Nov 22:3x, down, as Build now counts what a customer gets: `who-was-here` isn’t in the product’s install, and the Store lists it but doesn’t offer it (Backend2, 21:49), so a customer can’t get it. |
| Music to relax, an installable skill | 0.60 | #1685 #2792 #2793 | second, sixth, first, nine | 6 Nov 17:3x, down from 0.70: LAMP_REQUESTED_AUDIO is in no product unit and the skill isn’t in the product or the Store; |
| A bathroom-break reminder, an installable skill | 0.90 | #3504 | — | 9 Nov 12:3x, up from 0.5 (was stale): comfort-break is a core skill, on for customers through LAMP_NUDGES_PRESENT in stable 10824, and was played on lamp-52e6 on 7 Nov with a face at the desk (#5555, Jarvis). |
| A meal reminder, an installable skill | 0.90 | #3504 | — | 9 Nov 12:3x, up from 0.5 (was stale): meal-check is offered in the Skills Store and was played on lamp-52e6 on 7 Nov with a face at the desk, installed through the helper (#5555, Jarvis). |
| Posture: a kind word when you slouch, and a praise when you sit well, an installable skill | 0.90 | #3504 | — | 6 Nov 17:3x, up from 0.50: the product’s Sit back said its nudge after 600 s and its warm word on lamp-9adf at 10300, on stable since 10343; |
| Faces from a photo and “who is this?”, an installable skill | 0.80 | #2854 #2855 #2856 | mobile, ten, fourth | 6 Nov 17:3x, down from 0.90: LAMP_PHOTO_FACES is off in the product, with its acceptance run still owed. |
| Light shows and effects, a core skill | 0.85 | #1730 #2028 | — | 6 Nov 21:5x, down from 1.0, as Build now counts what a customer gets (the lead’s agent for the lead, 21:34): built and tried on a lamp, but `light-show` is installed off on customer lamps (install::PRODUCT_SKILLS_OFF). |
| “I’m unplugging you”: it folds safely first, a core skill | 0.85 | #1699 #2478 | fifth | 6 Nov 21:5x, down from 1.0, as Build now counts what a customer gets (the lead’s agent for the lead, 21:34): built and tried on a lamp, but the product doesn’t set `LAMP_UNPLUG`, so a customer’s lamp hides and ignores it… |
| Smoke and bad-air alerts, a core skill | 1.00 | — | — | 20 Oct 11:42, built: tried on lamp-52e6 with the lead (row 18): a smoke reading said once, the head lifted a little, an amber breath. |
| Every lamp passes an approved hardware test before it ships: stations, records, a 3–5 lamp pilot | 0.45 | #1700 #812 #1682 #1741 #2794 #2795 | first, ten, thirteen, hw-first | 23 Oct 10:1x, up from 0.4: the matrix’s version 1, version 0’s checks and limits approved for the pilot, is merged (#2461, train 29). |

## Quality rows

| Row | Now | Target | Where it is |
|---|---|---|---|
| Fast answers | 0.75 | 0.85 | 23 Oct 10:1x, up from 0.7: a real person (the lead, lamp-52e6, build G, 09:16–09:31): the first sound at 1.18–1.71 s on five of six lines, one tool follow-up at 7.8 s. |
| Knows when you have finished | 0.70 | 0.85 | 27 Oct 01:3x, unchanged: a bare “Lamp.”, a pause, then the request, right after a voice start, still splits and is held in a room not yet known to be quiet. |
| Cutting in | 0.55 | 0.75 | 31 Oct 16:1x, unchanged: on lamp-4ace’s product unit (c08b4c08f6), “Wait, hold on.” about 6 s into a story was handled 4 of 6 times. |
| Stop words | 0.70 | 0.75 | 9 Nov 10:2x, up from 0.6: stable 10824 (promoted 9 Nov 08:41) has the stop words heard over the lamp’s own voice (LAMP_STOP_WORDS_KEYTERM on, #5111 closed). |
| Its own echo | 0.65 | 0.70 | Measured right today (#968): the canceller takes a median 54 dB off the lamp’s own voice on lamp-52e6, the same hardware; |
| Not spoken to | 0.50 | 0.90 | 3 Nov 23:3x, down from 0.6: the core-verifier at 9868 held only 1 of 6 desk lines in a going-on thread (two called recall on talk between people), with #5034 reverted; |
| Several people | 0.70 | 0.85 | 23 Oct 11:2x, up from 0.65: a follow-up is asked only of the person who told it. |
| Sounds that are not speech | 0.80 | 0.80 | 18 Oct 17:48, the same 48 noises now judged from the lamp’s own state recorded during the run (the 17 Oct zeros came from an older recording, so they do not compare): answered 0 times, but 12 of 48 taken for a voice and … |
| Speaking up on its own | 0.85 | 0.85 | 9 Nov 10:2x, back up from 0.8: #5498’s fix (a stale oldest reminder no longer silences every later one, ec2be29dbc) is on stable since 10686, and on 10824 a one-minute timer rang and was said on lamp-52e6 (Fifth, 07:20) … |
| Onboarding, as a first-time person meets it | 0.75 | 0.90 | 10 Nov 20:3x, up from 0.65: on the promoted 11372 (lamp-52e6, 16:41–16:51) Seventh’s scene A passed: every step word heard and none made up (#5995’s strip now runs in the worker), the face question said word for word (#6… |
| Remembers you: your name and what you told it | 0.75 | 0.90 | 10 Nov 00:4x, up from 0.7: Friday’s batch on stock 10824 pooled 0.85 (from 0.80); |
| A good companion | 0.70 | 0.90 | 6 Nov 11:1x, down from 0.75, on stable 10441: the core-verifier counts one claimed action not done (lamp-52e6, 04:24, 10441’s voice: “I’ve kept the light on until ten.” with no call and no schedule item), so claimed-not-… |
| Both microphones | 0.60 | 0.80 | 6 Nov 17:3x, down from 0.65: tester lamps on 10226 had name lines with no words (220c 29/36, a63e 17/33), and 4ace’s lines came back empty 138 of 153 on 6 Nov; |
| Body | 0.65 | 0.80 | 4 Nov 08:3x, down from 0.7: in the night’s sleep on lamp-06c1 (9868), the body sat up for each of three room lines and stayed up about 80 s (Jarvis); |
| Head and eye | 0.65 | 0.80 | 23 Oct 11:2x, up from 0.6: head touch passed with the lead’s own hand on lamp-52e6: a tap, a stroke, a 3 s hold that put it to sleep, a tap that woke it, and nothing with hands away (#2857 closed). |
| The cheaper voice (Deepgram + DeepSeek + ElevenLabs) as good as Live | 0.65 | 0.90 | 6 Nov 17:3x, down from 0.70: installed 9928 on 52e6 in office hours heard 11/32 and 20/32 right, well below the 48/49 of its old run; |
| Light and all of it together | 0.85 | 0.80 | 3 Nov 23:3x, up from 0.8: the core-verifier on lamp-52e6 at stable 9868 (#4305, 22:30–23:05, one going-on thread): 19 of 19 action lines heard right called the right tool, 0 talk-only (5 of 11 and 4 talk-only on 9582 thi… |
