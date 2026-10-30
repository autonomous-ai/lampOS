# Where V2 is: the build rows and the quality rows

Written by the first builder from the team's review page at 2026-10-30 11:04 (+07). The lead asked for it on #1711 (20 Oct, 16:43), so anyone can read where V2 is without waiting for the hourly post. It's a copy, updated a few times a day. The hourly figures on #1722 are the live ones.

**Build 90.8% over 76 rows. Quality 68.4% over 17 rows.** Mood, for later: 39.0% over 3 rows, not in the build figure.

**How a build row is counted** (the lead's rule): 1.0 when its work is merged and tried on a lamp; less while parts are missing or untried. Build is the mean of the rows. Quality rows score how well a part works today, against a target; quality is the mean of the scores.

**Mood, for later:** the mood rows stay in V2 but out of the build figure, which counts the rows that ship. No day slot or new work goes into them until those reach 100% (the lead, 23 Oct on #1711). On 22 Oct the first builder had kept them in the figure as rows that aren't ship gates.

## Build rows

| Row | Score | Issues | Owners | Where it is |
|---|---|---|---|---|
| The voice never dies (SD stalls, power cuts, midnight) | 1.00 | #784 #762 #1178 | fifth, eighth | Midnight fix and stall recorder merged; |
| Speed: tool turns and comma pauses | 1.00 | #799 #782 | second, fourth |  |
| Who is talking, and when not to answer | 1.00 | #753 #792 #800 | fourth, second | #800 behind switches |
| Privacy switch closes both microphones | 1.00 | #795 | — |  |
| Faces: remember, hello by name | 0.93 | #759 #775 | first | 24 Oct 09:4x, up from 0.92: with LAMP_REMEMBER_SPEAKER (off), a request to remember a face needs one face positively read as speaking it, and missing or conflicting evidence refuses (#3318, Third, merged 9ed44f88c). |
| Hand work to Claude Code or Codex, and its news | 1.00 | #751 #765 #798 #733 | seventh |  |
| Backup voice when Gemini is down | 1.00 | #778 #754 | sixth | The relay chat it uses has been down since 02:29 |
| Gives way to a hand; wakes after power | 1.00 | #780 #790 #702 #703 #701 | fifth | Arm-goal guard merged (#1192) |
| “Lamp, that was weird” marks a moment | 1.00 | #803 | sixth |  |
| The first five minutes | 0.95 | #841 #2843 #2844 | seventh, el | 25 Oct 10:5x, up from 0.9: on in the product (#3435, train 114). |
| Speaks first when it should (the Policy) | 1.00 | #752 | second |  |
| Each person’s memory kept apart | 1.00 | #764 | seventh |  |
| Sleep, and waking on its name | 1.00 | #749 #714 | fourth, second |  |
| Reminders, timers, memory across days | 1.00 | #748 #743 | — |  |
| Setup for a home, 5 s reset | 0.95 | #806 #1209 #2861 #3579 | first, el | 26 Oct 20:3x, up from 0.85: shown on lamp-52e6 with build 7071 (the lead’s agent, 20:10–20:28). |
| A safe shutdown from the app and a long press | 0.70 | #3697 #3698 #3713 #3735 | fifth, mobile | 29 Oct 07:0x, up from 0.5: the app’s path through setup’s helper landed (#3735, train 324, stable in 8389: answered first, refused during an update), and the voice restarted under a shutdown is fixed (#3982, closed). |
| Touch strip and head pads | 0.95 | #711 #1362 #2445 #2460 #2857 | first, fifth | 27 Oct 15:1x: found on the tester’s 7071 lamps: a hand kept 3 s put the lamp to sleep unsaid, in onboarding’s touch and giving-way steps, and they reset it (#3747, #3749). |
| Two people at the lamp | 1.00 | #829 #1158 #1278 | fourth | All merged: day and night bars (#1158, 11:49) and a follow-up within 20 s told it may follow on (#1278, 13:57). |
| “Stop” over its own voice | 1.00 | #828 #1047 #1264 | fourth | Built: stop words heard over its voice (#1047, 09:11) and listening gaps in its pauses (#1264, 11:52), both off until measured. |
| Skills: say service, Reminders, Task news | 1.00 | #939 #1042 #1237 #1253 | second | All merged: the say service and the Policy gate, Reminders (with #1253’s fix), Task news as a skill (#1042, 11:44) |
| Skills: body and light shared between skills | 1.00 | #940 #1239 #1252 | seventh | All merged: the light side, the body’s side (#1239, 11:45) and the skills side (#1252, 12:32), behind their switches |
| Skills: creator, Stretch break, Stuffy air, Desk tutor | 1.00 | #941 #1066 #1011 #1260 | ten | All merged: Stuffy air (#1066), the reading pose (#1011, 08:58), Desk tutor’s nod and tilt (#1260, 10:23); |
| Two-minute self-test | 1.00 | #812 #983 #1243 #1245 | ten | All merged: every part checked, reported in health (#1243), run at first boot and every 6 hours (#1245), and the head’s small move (#983, 12:23) |
| One install for a new lamp | 1.00 | #811 | nine | Shown 19 Oct 15:1x on a second lamp, lamp-c0a4: build 3162 installed over V1 from a test-signed local channel, all 45 files equal to the build’s checksums, V1 stopped and kept, no reboot; |
| No internet at home | 1.00 | #830 #1242 #1256 #1247 | nine | 21 Oct 14:1x: its lamp runs passed on lamp-75c9 (Nine): the outage found with nobody speaking, the offline line once, “back online” after, and the unavailable face. |
| Everything is a skill, core and installable, no one-off code | 0.85 | #940 #938 #1667 #1696 #1698 #2643 #2654 #2674 #2822 #2823 #2824 #2825 #2826 | seventh, fifth, thirteen, fourth, ten | 24 Oct 08:4x, up from 0.72: this row’s note was stale. |
| An owner makes their own skill | 0.90 | #941 #1698 #2424 #2811 #2813 #2814 #2815 #2816 | sixth, ten, blue, fourteen, seventh | 30 Oct 11:0x, up from 0.83: the app’s make-a-skill screen is done (#2814 closed). |
| A Skills Store to publish skills | 0.90 | #1698 #2801 #2802 | fourteen, mobile, backend3 | 30 Oct 11:0x, up from 0.6: the Store’s install from the app ran on a real lamp. |
| Updates over the internet, by themselves | 0.95 | #737 #1812 | migrate, second | 30 Oct 05:3x, unchanged: tonight found that a lamp on the 7071 image reaches stable in one jump, climbing to the new updater first. |
| A voice cheaper than Gemini Live, as fast and as good | 1.00 | #754 #1686 #1712 #1973 #2135 #2136 | sixth, fourth, first, backend, third | 20 Oct 10:1x, built, by the lead’s rule (it works and it is fast enough are separate): on the main line since #1973, on lamp-4ace, and 32 of 32 answered for the lead on lamp-52e6. |
| A person brings their own DeepSeek and Deepgram keys, in setup or the app | 0.90 | #2094 #2845 #2846 | el, fourteen | 22 Oct 18:0x, up from 0.8: setup’s own-keys step passed on a freshly reset lamp-06c1 (El, #2845 closed). |
| Lamp 2 in the Autonomous app | 1.00 | #1396 | mobile, backend | 20 Oct 11:4x, built: the app's Skills page and name and time zone (ecm-sds-mobile#8, #9) merged into the app's v3 after their tries on lamp-0c89. |
| Speaks English, Japanese, Chinese or Korean, the owner’s choice in setup or the app | 0.00 | #3975 #4473 #4474 | seventh, mobile | 30 Oct 09:4x, new, from the lead’s proposal (#3975), approved today: the owner picks the language in setup or the app at any time, and the voice, onboarding, fixed lines and skills follow it. |
| Talks with you naturally: turns, answers, web answers, the time | 0.92 | #694 | second, nine | 24 Oct 06:1x, up from 0.9: the two-turn fault’s fix is merged behind LAMP_SAME_SPEAKER_CARRIES, off (#3296, Sixth, train 62, df6830381). |
| Hears its name, a bare “Lamp” too | 1.00 | #694 #725 | second, nine | Built; |
| Eye contact: turns to you and to its name, looks away to think, nods | 1.00 | #715 | fifth | Built |
| Always alive: breathing, small drift, a glance at a bang | 1.00 | #710 #714 #715 | fifth, second | Built |
| Moves with its words while it speaks | 1.00 | #933 #1067 #1365 #2030 | thirteen | 20 Oct 09:4x, built: #1759 (with #1067’s sway) merged into lamp-voice after its try on lamp-52e6 with the lead, so every lamp nods and sways while it speaks, sparse as the lead chose. |
| Never tips itself over | 1.00 | #1072 #704 | — | Built with V1’s limits; |
| One inner state (mood) every behaviour reads, and familiarity *(mood, for later: not in the build figure)* | 0.72 | #713 #1670 #1728 #1749 #1772 #2829 #2830 #2831 | fourth, sixth, jarvis, seventh | 22 Oct 22:0x, up from 0.7: a say’s record keeps who it told and the familiar level (#3056 merged, Seventh), so #2831’s last bar can be read on 52e6. |
| Music matched to your mood (for later) *(mood, for later: not in the build figure)* | 0.00 | #3504 | — | 27 Oct 08:1x, new, from the lead’s 07:58 list: music to relax plays one 48 s piece. |
| Shows its feelings in body and light, as V1’s emotions did | 0.50 | #3504 #3691 #4020 | jarvis | 29 Oct 06:3x, up from 0: this row was stale. |
| Joys: sways to music, watches you leave, core skills | 0.80 | #1669 #1707 #1748 #2832 #2833 #2834 | sixth, fifth, nine | 23 Oct 16:1x, up from 0.7: music sway and watching you leave are on in the product units, with watch-you-leave by place only and no face print (#3219, a21f45e56). |
| Dance when asked | 1.00 | #999 | jarvis | Built (the Dance skill, V1’s dance clips) |
| Bedtime and Morning as a skill, with night caps on light and volume | 0.85 | #1672 #1732 #2803 #2804 | jarvis | 23 Oct 09:2x, up from 0.65: a full real night through the skills on lamp-06c1 met its done-when (#2803), the night caps on light and volume held across restarts and silence (#2804), and night sleep now runs through the s… |
| “What do you see?” and showing it a page | 1.00 | #836 #842 | seventh | Built |
| Seeing someone speak (is the face it looks at talking?) | 0.94 | #720 #1470 #2008 #2026 #2859 #2860 | fifth, sixth | 24 Oct 09:4x, up from 0.92: built and merged, all off unless set: with LAMP_EYES_SPEAKING_DROP_OUTLIER the eyes drop one misread look from a speaking window instead of voiding it (#3313, #3317), and with LAMP_EYES_SPEAKI… |
| The eye: looks toward you, blinks, a face for each state | 1.00 | #712 | first | Built (ring eye, asleep face) |
| Its light by voice and in the app, kept across a restart | 1.00 | #1472 #1484 #1464 #1468 | — | Built |
| Volume by voice and in the app | 1.00 | #1458 #1465 | — | Built |
| A light for booting and a “not feeling well” face for a failed part, built into the system | 0.95 | #2847 #2848 | third, el, tests | 26 Oct 08:0x, up from 0.9: the waking face is on in the product unit (#3518, #2848). |
| Light scenes (“reading light”) and pointing the head and light (“light my book”), core skills | 1.00 | #1679 #1730 | twelve | 20 Oct 12:07, built: tried on lamp-52e6 with the lead, row 5 (reading tipped the head 148 counts down and leaned the arm in, focus too) and row 4 (pointing the light toward a named side). |
| “Look for my keys”, an installable skill | 0.85 | #1685 #1738 #2444 #2466 #2835 #2904 | first, thirteen | 22 Oct 17:5x, up from 0.8: a push on the lamp ends its search at once, a real push and a test input each ending it on lamp-c0a4 (Thirteen, #2983 carried as #2990, fe49ed3c4). |
| The light dims and goes out when nobody is there, a core skill | 1.00 | #1677 #1721 #1747 | nine | Built and tried on lamp-4ace 20 Oct 01:23 (trials.sh with Tests’ #1999 stages, lamp-skills at 782e6ef34): with nobody there the light-out skill asked the ring to fade the light over 30 s, held it while nobody was there, … |
| What you teach it (“don’t say hello in the morning”) is kept and honoured | 1.00 | #813 | seventh | Built |
| Forgets on request: that, everything, my face, and everything it came from | 0.97 | #1586 #1693 #2849 #2850 | fourth, friday, twelve | 23 Oct 10:1x, up from 0.95: after “forget everything”, the room’s lines about that person are left out for their day (#3142, train 29). |
| Remembers what matters without being asked (“how was the wedding?”) | 0.90 | #1693 #2648 #2680 #2805 #2806 #2807 #2808 | friday, twelve, thirteen | 26 Oct 09:5x, up from 0.85: the product keeps what matters without being asked, dated things and one open thread, for the person seen saying it only, and “forget everything about me” reaches it (#3450, merged 08:4x: OBSE… |
| Keeps going when one part fails | 1.00 | — | — | Built: each part its own unit, restarted alone, health says how the lamp is |
| Change the Wi-Fi, name or time zone after setup, without a reset | 0.95 | #1982 #1995 #2038 #2851 #2852 #2853 | fourteen, mobile, migrate | 27 Oct 12:2x: on 7071 a Wi-Fi change after setup is off in the product (LAMP_SETTINGS_WIFI, pending broker TLS; |
| Hand-overs from a home lamp: Harness installed, the person signed in | 0.88 | #751 #765 #798 #1854 #2702 #2838 #2839 #2840 #2841 #2842 | eighth, tests, blue | 23 Oct 16:1x, up from 0.85: a bare project name now takes every hand-over, not only the first (#3217), and each hand-over’s outcome is one journal line without the ask (#3234). |
| Updates cover the whole lamp, so V3, V4 and V5 update a shipped lamp in place: units, models, the updater itself, rollback, every part’s saved data read across versions | 0.83 | #1369 #1505 #2700 #2809 #2810 | migrate, tests, second | 27 Oct 15:1x, up from 0.78: the product’s own units are carried now (#3466, promoted in 7403); |
| The company sees its lamps in homes: health, crash and support logs, opt-in | 0.95 | #1683 #1755 #2862 | jarvis, mobile, backend, tests | 22 Oct 12:46, up from 0.9: the health report lists the product parts that have failed, under the same consent (Second, #2764). |
| The factory’s per-lamp steps: body poses, touch pads, the update key | 0.95 | #1682 #1717 #1744 #2817 #2818 #3578 | third, thirteen, mobile, fifth | 26 Oct 20:3x, up from 0.9: #3578’s step 4 on lamp-52e6 with 7071: the self-test passed, “the lamp is stopped, every part finished” for 23 units, then the reset and the reboot into setup, with nothing left behind. |
| Reads your mood from your face or voice *(mood, for later: not in the build figure)* | 0.45 | #2796 #2797 #2798 #2799 #2800 | ten, fourth, sixth, fifth | 23 Oct 02:1x, up from 0.35: the product units carry the face reader, Blendshape alone (#2953, #2799 closed), and onboarding’s one ask whether the lamp may notice how a face seems, keeping only a yes, is merged (#3082, tr… |
| Notices your activity and posture | 1.00 | — | — | Built and tried on lamp-4ace 20 Oct 01:23: with LAMP_ACTIVITY on the eyes say what the person at the desk is doing on room/activity (away in an empty room); |
| Habits and wellbeing nudges beyond Stretch break, as skills | 1.00 | — | — | 20 Oct 11:46, built: all four tried on lamp-52e6 with the lead (row 12: rest your eyes, a sip of water, stand up, wind down), each passed with its skill done. |
| Text your lamp from your phone, in the Autonomous app | 0.70 | #1396 #1673 #1726 #1742 #2836 | mobile, backend, fourth | 20 Oct 11:3x, up from 0.25: on lamp-0c89 from a phone, a texted reminder was set and said at the lamp 2 min later, and a moving skill was refused from a text; |
| Email, calendar and other connectors, through hand-overs | 0.70 | #751 #2740 #2820 #2821 | eighth, el | 22 Oct 18:0x, up from 0.6: an email ask handed over and told back, 6 of 6 on lamp-06c1 with the stand-in (El, #2820 closed), as the calendar was on 4ace. |
| Guard mode, an installable skill | 0.85 | #2562 #2827 #2828 | jarvis, nine | 22 Oct 15:1x, up from 0.75: merged, each off unless set: it names only a known person who said yes to being named (LAMP_GUARD_NAMES, #2562, 645d2f554), and it reads a return or a thank-you as off (LAMP_GUARD_WORDS_V2, #2… |
| “Was anyone here while I was out?”, an installable skill | 1.00 | #1989 #2019 | — | 20 Oct 10:1x, built by the lead’s rule (merged and tried on lamp-4ace at 03:3x); |
| Music to relax, an installable skill | 0.70 | #1685 #2792 #2793 | second, sixth, first, nine | 26 Oct 10:3x, up from 0.45: tried on lamp-4ace with 6963 (#2793 steps 1, 2 and 4 PASS): asked aloud, the lamp confirms and the 48 s piece starts 10 ms after its words end, “Lamp, stop.” stops it within about 0.1 s, it pl… |
| A bathroom-break reminder, an installable skill | 0.50 | #3504 | — | 29 Oct 00:2x, up from 0: this row was stale (Third, 00:19). |
| A meal reminder, an installable skill | 0.50 | #3504 | — | 29 Oct 00:2x, up from 0: this row was stale (Third, 00:19). |
| Posture: a kind word when you slouch, and a praise when you sit well, an installable skill | 0.50 | #3504 | — | 29 Oct 00:2x, up from 0: this row was stale (Third, 00:19). |
| Faces from a photo and “who is this?”, an installable skill | 0.90 | #2854 #2855 #2856 | mobile, ten, fourth | 24 Oct 00:3x, up from 0.88: the eyes’ photo yes counts only a frame dated strictly after it, by one tested conversion, and ends when the voice closes its connection (#3279, train 51). |
| Light shows and effects, a core skill | 1.00 | #1730 #2028 | — | Built and merged (#1730): party, music and calm, under a minute, no flashing, stopping within 0.2 s when someone speaks. |
| “I’m unplugging you”: it folds safely first, a core skill | 1.00 | #1699 #2478 | fifth | 21 Oct 11:30, built: #1699 merged as #2478 after its rebase, and Prepare to unplug is in every lamp’s default skills. |
| Smoke and bad-air alerts, a core skill | 1.00 | — | — | 20 Oct 11:42, built: tried on lamp-52e6 with the lead (row 18): a smoke reading said once, the head lifted a little, an amber breath. |
| Every lamp passes an approved hardware test before it ships: stations, records, a 3–5 lamp pilot | 0.45 | #1700 #812 #1682 #1741 #2794 #2795 | first, ten, thirteen, hw-first | 23 Oct 10:1x, up from 0.4: the matrix’s version 1, version 0’s checks and limits approved for the pilot, is merged (#2461, train 29). |

## Quality rows

| Row | Now | Target | Where it is |
|---|---|---|---|
| Fast answers | 0.75 | 0.85 | 23 Oct 10:1x, up from 0.7: a real person (the lead, lamp-52e6, build G, 09:16–09:31): the first sound at 1.18–1.71 s on five of six lines, one tool follow-up at 7.8 s. |
| Knows when you have finished | 0.70 | 0.85 | 27 Oct 01:3x, unchanged: a bare “Lamp.”, a pause, then the request, right after a voice start, still splits and is held in a room not yet known to be quiet. |
| Cutting in | 0.55 | 0.75 | 25 Oct 19:0x, up from 0.5: on lamp-52e6 with the ship candidate’s own product switches (Seventh, played ElevenLabs lines, an empty room), every real cut-in stopped the story, 4 of 4, with no false stop; |
| Stop words | 0.55 | 0.75 | 24 Oct 02:0x, up from 0.5: in a quiet room on lamp-52e6, “Stop” during an answer stopped the lamp 10 of 10 times with the stop words on, against 7 of 10 off, with no false stops (Sixth, #2863, made voices from the Mac). |
| Its own echo | 0.65 | 0.70 | Measured right today (#968): the canceller takes a median 54 dB off the lamp’s own voice on lamp-52e6, the same hardware; |
| Not spoken to | 0.70 | 0.90 | 26 Oct 14:5x, up from 0.6: on 6999 (lamp-voice d5dc2e87f, #3517’s LAMP_NAME_STAYS_IN_ITS_LINE on) the busy scene’s desk talk was left alone 6 of 6, against 4 of 6 on 6919 and 6963: a name misheard in one colleague’s line… |
| Several people | 0.70 | 0.85 | 23 Oct 11:2x, up from 0.65: a follow-up is asked only of the person who told it. |
| Sounds that are not speech | 0.80 | 0.80 | 18 Oct 17:48, the same 48 noises now judged from the lamp’s own state recorded during the run (the 17 Oct zeros came from an older recording, so they do not compare): answered 0 times, but 12 of 48 taken for a voice and … |
| Speaking up on its own | 0.82 | 0.85 | 26 Oct 14:5x, up from 0.8: on 6999 every reminder set was said: one cut off by people talking is offered again and said (#3523, Third), and the next one triggers the reminders skill again, so no 10 s wait. |
| Onboarding, as a first-time person meets it | 0.60 | 0.90 | 28 Oct 17:0x, up from 0.5: the tester’s run on 29e7 at 8187 passed every step but how you sound’s sentence. |
| Remembers you: your name and what you told it | 0.65 | 0.90 | 28 Oct 05:3x, back up from 0.6: stable 7887 has Friday’s fast forget (#3871): on lamp-4ace a targeted forget answered in 375 ms and the lamp said “Gone”; |
| A good companion | 0.70 | 0.90 | 25 Oct 14:4x, up from 0.55: two played rounds on lamp-4ace with the product’s hearing (ElevenLabs voices from the iMac, no person): 27 of 27 conversation lines answered without the name, in three back-and-forths of up to… |
| Both microphones | 0.65 | 0.80 | 22 Oct 06:41, up from 0.6: step 4 passed on lamp-75c9 from the privacy test input (Nine, #694, the lead’s 05:5x ruling): after the switch closes, the room microphone stops within one 100 ms level period and gives nothing… |
| Body | 0.70 | 0.80 | 25 Oct 17:0x, up from 0.65: the livelier postures and nods (#3387) are in the product since train 109 (04:25), and on the ship candidate (6786) lamp-4ace stood for a played question, turned and tilted its head a little b… |
| Head and eye | 0.65 | 0.80 | 23 Oct 11:2x, up from 0.6: head touch passed with the lead’s own hand on lamp-52e6: a tap, a stroke, a 3 s hold that put it to sleep, a tap that woke it, and nothing with hands away (#2857 closed). |
| The cheaper voice (Deepgram + DeepSeek + ElevenLabs) as good as Live | 0.70 | 0.90 | 25 Oct 20:0x, up from 0.65: the ship build (6819) on the exact product unit: 48 of 49 lines meant for the lamp (the one miss played over a 10 s answer by the kit), the owner’s 22 of 22, desk talk 5 of 6 left alone, first… |
| Light and all of it together | 0.75 | 0.80 | 24 Oct 16:1x, down from 0.8: the not-feeling-well light (pale white) came on 18 times in three hours on lamp-4ace because its case at 40 °C was judged an error (#3367 makes that 46, off at 50). |
