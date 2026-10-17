# Switches

Every setting that turns a behaviour on in `lamp-voice`, `lamp-body`, `lamp-eyes`,
`lamp-expression`, `lamp-skills`, `lamp-tasks` and `lamp-controls`: what it does,
what lamp-4ace and the product set, the pull request that added it with the lamp
check it named, and the product default proposed for it. Made for the owner's
question of 17 October, "why off?", and for switching lamp-4ace on group by
group: each switch that passes its check becomes the default in code before
1 November (the first builder, #705, 07:30).

**As of** `lamp-voice` `8c3df0449` (17 October 2026, about 08:50 +07), and the
product column as #1332 (17 October, 15:41) sets it, with `LAMP_SPOKEN_TO`
from #1378. The code is the source of
truth; this page is a census of it, to be kept up as switches are added, turned
on or removed.

## How to read it

- **Values and default:** what the code accepts, and what happens when the
  setting is absent. Almost every switch is off unless set.
- **lamp-4ace (repo unit)** and **Product unit:** what
  `lampOS/deploy/lamp-4ace/` and `lampOS/deploy/product/` set, reading only
  lines that aren't commented out. lamp-4ace may also run drop-ins that aren't
  in the repo (see "What the census found").
- **Added by:** the merged pull request that brought the name in, or the commit
  for the ones added to `lamp-voice` directly before pull requests were the rule.
- **Lamp check it named:** what that pull request or its doc said must be seen
  on a lamp, and whether a doc records it **shown**. Where nothing is recorded,
  it says so; no lamp result here is assumed.
- **Proposed product default:** "on (already)" where both units already set it;
  "on" where the evidence is in and it should be switched on; "off until …"
  naming the check that would turn it on; "off" for a diagnostic or a switch
  turned down.
- Paths, sockets and test hooks (`LAMP_*_SOCKET`, `LAMP_*_DIR`, `LAMP_*_FILE`,
  `LAMP_*_TEST_*`, `LAMP_BUS`, `LAMP_BUS_ROOT`, `LAMP_CLOCK`) and tuning numbers
  (speeds, poses, reaches) are left out: they configure, they don't turn a
  behaviour on.

## The count

111 switches (`LAMP_SERVICE_DOWN_LINE` added with #830 (C), off; `LAMP_EYES_SPEAKING` added with #1385, off; `LAMP_QUIET_PACE` and `--call-wait-ms` added with #1332, both set in the product unit, counted as on):

| | Voice: hearing, turns, ring | Voice: tasks, memory, schedule, skills, looks | Voice: its own flags | Other parts | All |
|---|---|---|---|---|---|
| On already, or right at its default | 16 | 10 | 8 | 16 | **50** |
| **Proposed to turn on now** | 0 | 1 | 0 | 1 | **2** |
| Off until its named lamp check | 27 | 13 | 0 | 6 | **46** |
| Stays off (diagnostic, turned down, or not needed) | 5 | 1 | 2 | 2 | **10** |
| A setting, not a behaviour | 1 | 2 | 0 | 0 | **3** |

**Proposed to turn on now**, with its newest lamp-4ace record a pass:
- `LAMP_LOOK_FULL=on` **with** `lamp-eyes --full-look`, the two together: it
  works end to end on lamp-4ace and has answered there since 16 Oct 03:17
  (#842), with things held up by a person shown on lamp-52e6.

`LAMP_POLICY_HOLDS_ACTIONS` and `LAMP_TASKS_WHO` were proposed here at first
on earlier passes; each has a newer lamp-4ace record that fails (the reviewer
on #1238), so each is "off until" its scenes pass. So was `LAMP_SPOKEN_TO`,
until the battery of 17 October showed the product deaf in a busy room
without it (#828, 19:22): it is on in the product unit since, and its two
failing scenes are for Sunday's two-people table (#829).

**The quickest wins among "off until"** are the switches whose check needs no
motion, since lamp-4ace's motors stay still until Monday: `LAMP_STAFF_NOTE`
(the in-time/late count), `LAMP_SHORT_SOUNDS=quiet-room-checked`,
`LAMP_ASKER_FIRST=record`, `LAMP_STOP_TOLD`, `LAMP_WAIT_KEEPS_ANSWER`,
`LAMP_TASK_NEWS_AT_PAUSE`, `lamp-eyes --leave-still-faces` (one live night) and
`lamp-expression --offline-face`.

## The voice: hearing, turns, the ring and the backup voice

| Switch | Values and default | lamp-4ace (repo unit) | Product unit | Added by | Lamp check it named | Proposed product default | Why (one line) |
|---|---|---|---|---|---|---|---|
| LAMP_ADDRESSED | `on`; off. Judges each request "said to the lamp from close"; needs LAMP_ATTENTION and LAMP_BUS | — | — | #792 | "Most likely wrong on lamp-4ace": sideways seat (#763), fewer than two looks; attention.md: "not on any lamp" | off until played on a lamp facing its seat | #792 names its likely faults and gives no lamp result; attention.md says not on any lamp |
| LAMP_ASKER_FIRST | `on`/`record`; off. Softer speech waits up to 5 s while the asker's answer is awaited | — | record (#1332) | #1092 | Plan: `record` on lamp-4ace first to set the 6 dB margin; group-talk.md: "6 dB is not yet measured" | off until a `record` run on lamp-4ace sets the margin | #1092 and group-talk.md say the margin is unmeasured; `record` is the step named |
| LAMP_ATTENTION | `on`/`busy`/`busy-near`; off. Busy-room rule holding answers not for the lamp | busy-near | busy-near | commit 01fc900cd (direct) | attention.md: busy-near on lamp-4ace since 18:46 on 14 Oct (build 41d09542f), 19.5–26.5 dB at desk | busy-near (already) | Shown on lamp-4ace 14 Oct (attention.md "A name heard close to the lamp"); many later switches need it |
| LAMP_BUS_AUDIO | `on`; off. Puts the voice mic's audio on the bus (needs LAMP_BUS) | on | on | commit 6e1392926 (direct) | none named; name.md and selftest.md need it for lamp-name and the mic check | on (already) | lamp-name (in product) hears the name only from this topic (name.md) |
| LAMP_CUT_INS_NOT_TALK | `on` only; off. A line said while the lamp's own answer could be heard, and left unanswered, is not counted toward a busy room (it still keeps a busy room busy) | — | — | #828 (17 Oct) | none yet: the first builder's ten cut-ins on and off, and a busy office talking over the lamp (attention.md, "Cut-ins are not talk") | off until both are played | Unplayed; it also stops counting office talk said over the lamp |
| LAMP_CUT_KEEPS_NAME | `on`; off. A request continuing an early-ended one keeps its name/call | — | on (#1332) | #809 | "Most likely wrong": second voice inside a pause; the run at 400/550 ms; attention.md: "not on any lamp" | off until played on lamp-4ace | #809 waits on a played run; #918's 15 Oct records show 2 of 4 unsigned plays were such cuts |
| LAMP_EXPRESSION_BODY | `on`; never otherwise. Body adds Expression's requests to its pose (lamp-body) | on (lamp-body) | on (lamp-body) | commit fc93f8608 (direct) | expression.md: used on lamp-4ace since 2026-10-14 10:05 | on (already) | Running on lamp-4ace since 14 Oct (expression.md); the lean and rest glance need it |
| LAMP_EXPRESSION_OUT | `on`; nothing otherwise. Voice publishes voice/timing, activity, playout for Expression | on | on | commit 40e9b1c00 (direct) | expression.md: playout said ahead since 14 Oct 13:00, measured on lamp-4ace | on (already) | Expression's faces depend on it; in use on lamp-4ace since 14 Oct |
| LAMP_FAINT_FLOOR_MAX_DB | dBFS -47 to -20; unset = floor fixed at --faint-floor-db (-28) | — | — | commit 96bc22266 (direct) | lamp-4ace RUNBOOK: "off (watching)"; `room_floor` / `room_alone_dbfs` records to be watched first | unset until lamp-4ace's room_floor records support a ceiling | RUNBOOK lists it off (watching); group-talk.md says watch a room before letting the floor go |
| LAMP_FAINT_HOLD_MS | ms 0–1000; unset/0 = each block judged alone. Keeps trailing desk speech | 400 | 400 | commit de6ae60ed (direct) | not-speech.md: shown on lamp-4ace at 400 ms, 0 of 7 short phrases thrown out | 400 (already) | not-speech.md result on lamp-4ace; quiet lines at a tenth of the volume still not admitted |
| LAMP_FIRST_WORDS | `kept`; off. A yield's request starts at first words under the lamp's voice (≤1.6 s) | — | kept (#1332) | #936 | "Not shown: nothing was played"; a played minute of cut-ins (cutins.sh) and stories asked over. On 17 Oct #1246 measured on lamp-4ace that neither microphone carries a person's word said over the lamp's own speech, which this check depends on | off until the played cut-in minute | #936 measured only on replayed recordings; no lamp run in PR or docs |
| LAMP_FOLLOWS_ON | `on`; off. A line begun within 20 s of the lamp's words finishing, not over them, is told it may follow on (not in a busy room; the Live route only) | — | — | #829 (17 Oct) | none yet: the first builder's `s-0857`, on and off (conversation-thread.md) | off until `s-0857` passes with it | Unplayed; the model still judges whether the line follows on |
| LAMP_HEAD_TAP_CUE_DBFS | dBFS -60 to -20, else start refused; default -35. Level of the tap "hm?" | — | — | #849 | Hum level by ear on a lamp (#849 "Next"); attention.md: -35 "not yet measured" | unset (-35) until tuned by ear on a lamp | For tuning only; default untested against the lamp's speech (attention.md) |
| LAMP_HEAD_TOUCH | `on`; off. Head tap is a call / stops speech; hold 3 s = sleep | — | — | #849 | Tap-to-speaker under 150 ms, hum by ear, head_gap_ms recording; attention.md "not on any lamp" | off until tap latency is measured on a lamp | #849 says lamp latency unmeasured; #1176 keeps product pats in the old engine |
| LAMP_KERNEL_CLOCK | `on`; off. Says the clock is not set rather than tell a wrong time | — | on | #884 | offline.md "What was not shown": nothing run with the network cut. **Found 17 Oct (Ten on #705, the reviewer read the same):** the provider worker is spawned with `env_clear()` and this switch isn't passed on (`process.rs`), so the worker's `the_time` and session note never see the clock unset; Fourth passes it in #1158 | on (already in product; lamp-4ace's unit lacks it) | No battery clock (#830); it changes anything only while the clock is unset |
| LAMP_MARK_WEIRD | `on`; off. "That was weird" keeps the last minute for review, 7 days | — | — | #855 | Shown on lamp-52e6 15 Oct (mark written, listed); 7-day sweep host only | off | A builder review tool that keeps session records on the lamp; #855 itself is a builder check |
| LAMP_NAME_IN_PASSING | `ignored`; off. "the lamp" spoken about is no call in a busy room | — | ignored (#1332) | #864 | #864: "Nothing was run on a lamp"; attention.md: not on any lamp | off until played on lamp-4ace with LAMP_SPOKEN_TO | Model declined both played cases itself (14, 15 Oct); #918 run still answered 1 of 2 in-passing lines |
| LAMP_NAME_NOTE | `on`; never otherwise. Tells the model the lamp's name was heard | on | on | commit 7a07ebd9e (direct) | attention.md: played on lamp-4ace 14 Oct 07:28, note arrives and does no harm | on (already) | Shown on lamp-4ace 14 Oct (attention.md table) |
| LAMP_NAME_NOTE_BUSY | `face`; off. In a busy room the name note waits for a turned face | face | face | commit 0ca828c67 (direct) | attention.md: on lamp-4ace since 11:52 14 Oct | face (already) | From the 14 Oct meeting: 4 false name hearings in 20 min made the lamp answer the table |
| LAMP_NAME_SIGNAL | `on`; off. Measurement: model signals hearing its name | — | — | commit aabb3ac39 (direct) | attention.md: tried, heard 6 of 7 calls but answered 1 of 7; "the switch is off" | off | A measurement switch attention.md turned down: neither function kind was acceptable |
| LAMP_OFFLINE_VOICE | `on`; off. Offline line once per outage, amber breath, "back online" | on (#830, for the probe's check) | on (#830, with `lamp-health --online on`; no reach probe, as the product has no second route) | #884 | offline.md: not run with network cut; silent until recordings via --failure-notice-dir (none in units) | on: the product says no link once and keeps going (#830); silent until recordings | No unit provisions --failure-notice-dir, so it would be silent anyway; offline.md shows nothing run |
| LAMP_OTHER_LANGUAGE | `sorry`; off. Other-script/number-only turns in a conversation are asked again | — | sorry (#1332) | #930 | lamp-4ace 06:42 cherry-pick: no wrong answer in 10 cut-ins, but no "Sorry?" followed | off until a replay shows "Sorry?" said | #930 left why no "Sorry?" came for the next replay's trace |
| LAMP_PRIVACY_CONTROLS | `on` exactly (Controls, Voice, Ears together); off. Hardware slide closes both mics | — | — | #795 | privacy-switch.md handoff steps (off minute, slide mute LOW/HIGH, ring red); "no lamp was used" | off until the first builder's lamp handoff passes | #795: polarity, ALSA device, 50 ms budget unshown; promotion is the first builder's call |
| LAMP_REACH_PROBE | `on` (with LAMP_OFFLINE_VOICE); off. The link down says no internet only on `lamp-health --reach-probe on`'s verdict, so a relay outage with the internet up gets no offline line or breath | on (#830, with `lamp-health --online on --reach-probe on`) | — (not without a second route: a relay outage would drop speech in silence unless LAMP_SERVICE_DOWN_LINE=on, #830) | #1247 | offline.md: a minute with the Wi-Fi off (offline, the line once) and a minute with only the relay unreachable (online from `reach`, no line); not run, Monday on lamp-4ace with a person at the router (#830) | off until that check, and only where LAMP_VOICE_SECOND_CONFIG is set | Without the second route, speech while the link is down and the probe says online is dropped without a word (offline.md) |
| LAMP_SERVICE_DOWN_LINE | `on` (with LAMP_OFFLINE_VOICE and LAMP_REACH_PROBE); off. A link down while the internet answers, with nothing else to answer, gets the service's line once and the offline breath instead of silence | — | — | #830 (C) | Only the relay blocked for a minute (the owner's step 8b), if it can be done; silent until its recording | off until the recording is approved and the relay-only minute is played | Ends the silent drop of a relay outage on a lamp with the probe and no second route |
| LAMP_RESTORED_MUTE | `on`; off. Tells the restart guard's mute apart; head tap reopens it | — | — | #997 | kill -9 while listening, then one head tap opens it; a held mute stays; not documented as shown | off until that check, together with LAMP_HEAD_TOUCH | Needs LAMP_HEAD_TOUCH (controls.md); fixes the 16 Oct 12:20 deaf lamp seen on lamp-4ace |
| LAMP_RING_ASLEEP | `on`; off. Faint warm breathing ring while muted/asleep, not dark | — | on (#1332) | #1025 | Goes on with the stop fix (#1024), sleep by a hand on the head, photographed; no result in docs | off until the photographed sleep check | Rule-8 friendly (a dark ring reads as broken) but no lamp result recorded |
| LAMP_RING_EYE | `on`/`strong`; off. Ring looks toward the person and blinks at rest | on | strong (#1332) | commit d3e9cc3d2 (direct) | ring-choreography.md: lean "not yet seen on a lamp"; the owner found it too subtle 15 Oct, hence `strong` | on (already) | Running on lamp-4ace; the product unit sets `strong` since #1332 |
| LAMP_SAY | `on`; off. Say socket for other parts (skills) to speak through the voice | on | on | #954 | #954 steps: begun/spoken, dropped on speech-over, expired under pause; not recorded as shown | on (already) | Welcome back (lamp-skills, in product) speaks only through it (install.md) |
| LAMP_SAY_GROUP | a group name; unset = socket private to the voice's user | orangepi | lampos | #954 | Same as LAMP_SAY (socket as `orangepi` on lamp-4ace) | lampos (already) | lamp-skills runs in group lampos in product; orangepi is lamp-4ace's user |
| LAMP_SAY_SOUNDS | absolute dir; unset = /var/lib/lampOS/skills. Skill sounds only from under it | — | — | #1144 | #1144: "nothing has run on a lamp, and no sound has been heard" | unset (default matches install's skills folder) | The default is the product's skills folder; a path, not a behaviour switch |
| LAMP_SECOND_DOWN_LINE | `on`; off. Backup voice says "I can't think right now" when chat never starts | — | — | #1195 | none named (from the relay outage at 02:29, seen on lamp-52e6) | off until heard on a lamp with the relay's chat down | #1195 has host tests only; said unasked it must not read as a broken machine (second-voice.md) |
| LAMP_SECOND_LOOK | `on`; nothing otherwise. Tells the model its name was heard after it stayed silent | on | on | commit 407c55f26 (direct) | Shown on lamp-4ace 14 Oct from 06:04: the second look taken in 2 of 4 rounds, the first words about 5.3 s after (attention.md) | on (already) | In both units; one turn at most, and only to the session that heard it |
| LAMP_SECOND_WAIT_CUE | `on`; off. Backup voice makes one soft "Hm." if its first round is slow | — | — | #1086 | "Draft until lamp-52e6": five questions with Gemini down, how often/when the sound played | off until that lamp-52e6 run | No result of the named run in the PR or second-voice.md (which notes 9 of 10 turns waited past 1.5 s on lamp-4ace) |
| LAMP_SHORT_SOUNDS | `quiet-room`/`quiet-room-checked`; off. Short sounds still go to the model in a quiet room | quiet-room | quiet-room-checked (#1332) | commit 3e3de2d7a (direct) | not-speech.md: 13 of 36 short words lost on lamp-4ace 14 Oct; #800: a room sound answered under quiet-room | quiet-room-checked once played (quiet-room meanwhile) | not-speech.md #800 shows quiet-room answers room sounds; the checked form exists but has no lamp result |
| LAMP_SPEAK_FIRST | `on`; otherwise moments only recorded. The voice's own hello on arrival | — (commented out) | — (commented out) | commit 4985caf77 (direct) | RUNBOOK: on since 15 Oct, never due in 0 of 680 sessions; now replaced by Welcome back | off | Both units leave it commented so only the Welcome back skill greets (install.md, skills.md) |
| LAMP_SPOKEN_TO | `on`, or `day-and-night` (by day the name also needs a sure hearing, a face or its words, #1158); off. In every room an answer plays only when the lamp was spoken to | — | on (#828, 17 Oct) | #918 | Passed on lamp-4ace 16 Oct 04:38 on the pre-merge `211f32c74` (budget line 0 of 8, named 6 of 6, day.sh 12 of 12). Failing then: 06:06 on `1dc286181` the budget line was answered 3 of 4 (#918, 06:15), and 15:39 "Yes, give me five minutes.", said to someone else, set a timer (#829, attention.md "To another"); the rules added for both (`TO_ANOTHER`, `AFTER_QUESTION`, `TALK_ENDS_CONVERSATION`) are for Sunday's two-people table. **Without it, deaf**: on 17 Oct on `460a1a1aa` with `busy-near`, the battery answered 6 of 12 day lines without it (19:02) and 11 of 12 with it (19:26–19:40, as at 16:11); the cut-ins cost nothing (12 against 11 of 20). In a busy room a reply to the lamp's news is taken only when it asks about its work, so two colleagues talking a minute or two after the news stay held (#1378) | on | Without it a busy room holds every line nobody calls the lamp in, its name too with no face, and a held line starts no conversation, so it stays deaf (#828, 19:22) |
| LAMP_STALLS | `on`; off. Logs loop stalls ≥50 ms with getrusage to /run/lampos-voice | — | on (#1332) | #1179 | Next: on in lamp-4ace's drop-in, a scene across 00:00 (#1178) | off | A diagnostic for #1178, meant for lamp-4ace (serve-runtime.md) |
| LAMP_QUIET_PACE | `on`; off. The voice's idle loops look at their inputs less often while nobody talks and the lamp is silent, and at once again when a turn begins | — (lamp-4ace's drop-in sets it) | on (#1332) | #1261 | lamp-4ace A/B on 17 Oct, same build: the coordinator 16.3% → 14.4% of a core (the first builder, 11:46); a turn goes back to the fast pace at once (Twelve's review) | on (#1332) | About 2% of a core with hearing unchanged; the lamp runs hot (83–92 °C) |
| LAMP_START_OFFLINE | `on`; off. Voice started without internet keeps listening past the 25 s budget | — | on (#1332) | #884 | Stop Wi-Fi, restart the voice, it stays up logging "listening with no link"; lamp-4ace's installed unit has run it since 16 Oct, and the line never printed because that day's cut left the link up (#1242, offline.md) | off until the Wi-Fi-off restart check | offline.md: "Not yet shown on a host with its network cut or on a lamp" |
| LAMP_STOP_TOLD | `on`; off. First request after a hand/head stop tells the model it was stopped | — | on (#1332) | #1148 | Code and not-speech.md: "off until it is played on lamp-4ace" | off until played on lamp-4ace | Its own doc says it waits on that lamp check (from 16 Oct 16:32, story restarted 4 of 6 times) |
| LAMP_STOP_WORDS | `heard`; off. Stop words heard in a yield end the reply (relay speech-to-text) | — | — | #970 | Count yield outcomes against played cut-ins; privacy accepted for lamp-4ace only (#828, 16 Oct). On 17 Oct #1246 measured on lamp-4ace that neither microphone carries a person's word over the lamp's own speech, which this depends on | off | stop-word.md: privacy "to be decided again before home lamps"; speech goes to the relay during yields |
| LAMP_TRANSCRIPTION_LANGUAGE | a language tag (`en-US`); unset, none; not a tag, none and said. The service's transcript of what it hears gets that language hint (`inputAudioTranscription.languageCodes`); the reply's language is not set by it (not measured under the hint) | — | — | #926 (17 Oct) | Accepted by the service (a made-up field there is refused, 1007); no change on clear English. An accented clip not yet run | off until the accented clip shows Latin with it where none gives Devanagari, and 05:39's cut-ins replayed with it are still held or answered in English | Twelve runs `transcript_probe` on the clip |
| LAMP_VOICE_LINK | `on` (with bus); off. Publishes voice/link for lamp/online | — | — | #884 | lamp/online false 5 s after the link drops (with lamp-health --online on); not run | off until that check and lamp-health --online on in product | Only a signal; nothing in product reads it yet; offline.md shows nothing run |
| LAMP_WAIT_KEEPS_ANSWER | `on`, or `words`; off. A short sound does not cut off an answer still to come; with `words` (needs LAMP_VOICE_SECOND_CONFIG) one whose words say something takes the turn, and a lone stop said before the answer (#1352) stops it | — | on (#1332) | #1130; `words` #1123 step 2 | lamp-4ace steps 1–4: cough within 2 s, real follow-up, #1123 count before/after; not documented done. `words`: "No." and "Wait." five times each and five coughs in the gap, counting `wait_words` and `stop_said` (group-talk.md; a lone "Wait." now stops the answer, "Wait, what about…" takes the turn); not run | off until those lamp-4ace steps | #1130 "Most likely wrong": the 1 s, and a short "No."/"Wait." let go; `words` sends a waited sound to the relay, a privacy line still to decide |
| LAMP_SAME_VOICE_MODEL | absolute model path; unset = no check. The same-voice check beside LAMP_SPOKEN_TO (a line in another voice counts as talk) | — | — | #1030 | none recorded on a lamp | off until played with LAMP_SPOKEN_TO | It works only beside LAMP_SPOKEN_TO, which is itself off until its scenes pass |
| LAMP_VOICE_SECOND_CONFIG | absolute config path; unset = no backup voice. The second voice (the relay's chat) when Gemini can't answer | — | — | #778 | Runs on lamp-52e6 (#1154, #1195, the relay log); **on lamp-4ace on 16 Oct** (second-voice.md): 9 of 10 backup turns waited past 1.5 s, first events 1.6 to 20.5 s after the words, and it answered "Me." and half a question | off until a lamp-4ace run where its turns answer within #1154's bounds (3 s, 5 s for a look) and the relay's day log (the sixth, from 17 Oct) shows them reachable | Without it LAMP_SECOND_WAIT_CUE and LAMP_SECOND_DOWN_LINE do nothing; the relay was down 25–30 min on 17 Oct (the sixth's log) |
| LAMP_SELFTEST_SPEAKER | `on`; off. The speaker part of the self-test, from the echo of the lamp's own last speaking turn (#968; it plays nothing) | — | on (#1332) | #895 (changed by #968) | #968: shown on lamp-52e6 (43.4 dB median echo); not on lamp-4ace | off until its lamp-4ace run | Without it the self-test's speaker is `not_checked`, never a false fail |
| LAMP_FIRST_OUTPUT | absolute directory; unset = nothing kept. Keeps what was heard for each request, on the lamp | /var/lib/lampOS/voice/first-output | /var/lib/lampOS/voice/first-output | commit 8ae7b8868 (direct) | none of its own: a record, read by builders | set (already); worth a privacy line for a home | It keeps words heard in a home on the lamp, which `docs/service-privacy.md` (the nearest doc) should name |
| `lamp-voice --controls-mute` | absolute record path; off. The software mute (the privacy path) | — | — | commit 5f9914314 (direct) | Ran on lamp-4ace on 16 Oct: a crash at 12:20 brought the voice up muted, and nothing at the lamp could open it (controls.md, #997) | off until LAMP_HEAD_TOUCH and LAMP_RESTORED_MUTE are checked | LAMP_HEAD_TOUCH's 3 s sleep and LAMP_RESTORED_MUTE need it (`hand_controls.rs`, controls.md); head touch also needs the product pins' `"legacy_head": true` gone (#1176) |

## The voice: tasks, memory, schedule, skills and looks

| Switch | Values and default | lamp-4ace (repo unit) | Product unit | Added by | Lamp check it named | Proposed product default | Why (one line) |
|---|---|---|---|---|---|---|---|
| LAMP_AIR_FACTS | `on`; off unless set. Tells the model, once per 3 h, that the air has been stuffy for 10 min | on | on | commit 56f25ed9d (direct) | none named in a PR; air.md: on lamp-4ace since 14 Oct 18:46, first told 19:45 with a window suggestion (shown) | on (already) | air.md records it shown on lamp-4ace 14 Oct; one quiet remark, only while a person is already talking |
| LAMP_HELLO_ASKS_POLICY | `on`; off. A due voice hello waits up to 2 s for the Policy's `lamp-policy --hello` answer, then goes on without it | — | — | #883 | Shadow run on lamp-4ace: read `hello_due`/`hello_policy` waited_ms after the 11:00 and 16:00 sessions; speaking-first.md: no real person has yet made a hello due (not shown) | off | The product's hello is the Welcome back skill and the voice's own is off (install.md); without `--hello` in the policy unit it only makes a hello 2 s late |
| LAMP_LIGHT_REQUESTS | `on`; off. Ring takes lapsing light requests from skills, the person's light returning by itself | — | on (#1332: the voice's unit and `lamp-skills`') | #993 | Publish a 10 s green `light/request`: ring eases green, back to the person's light, "make your light blue" ends it; no record of it shown | off until the #993 ring check is played (on both lamp-voice and lamp-skills) | Without it skills give the light back by hand (skills.md); #993 left the run to the first builder and no doc records it |
| LAMP_LOOK_FULL | `on`; off. "Let me look" asks the eyes for one larger, settled still | — | — | #842 | Shown: lamp-52e6 15 Oct (6 of 10 objects named, against 4 of 10); **on lamp-4ace it works end to end and has answered there since 16 Oct 03:17** (#842, 03:23 and 08:58); things held up by a person shown on lamp-52e6 (`docs/verification/full-look-held-lamp-52e6-20261016`) | on, with `lamp-eyes --full-look` in the same units | Shown on both lamps, held objects included; it needs the eyes' flag beside it, which no unit sets yet |
| LAMP_LOOK_FULL_SIDE | integer 320 to 4096 px; unset = 1024 across / 64 KiB. Still's side up to 184 KiB, for handwriting | — | — | #987 | none named (PR body is only "Part of #941") | off (unset) until a handwriting look is shown | Only useful with LAMP_LOOK_FULL and a camera run large (eyes.md #941); no lamp result recorded |
| LAMP_MEMORY_DAYS | `on` only; off. The voice uses the Days memory client across days | on | on | #743 | The voice switch's own lamp-4ace minute: #743, 15 Oct 02:49 (memory-days.md's 14 Oct 14:50 is the memory part's `--memory-days`) | on (already) | In both units, shown on lamp-4ace |
| LAMP_MEMORY_RULES | `on` only, with Days memory; off. Keeps a person's corrections as rules for later sessions | — | on (#1332) | #822 | Shown on lamp-52e6 twice 15 Oct (rules kept, told, taken back by number); not shown: Policy reading rules, seventh rule on a lamp | off until its lamp-4ace minute | Shown only on lamp-52e6 with synthesized speech (#822); the Policy half of #813 not shown |
| LAMP_NAMES | `checked` only; off. Name kept only if said/spelled; one name per face | — | checked (#1332) | #1057 | #1057 "Not shown": a played name in a busy room, model following the note; "For lamp-4ace: LAMP_NAMES=checked" | off until played on lamp-4ace | Fixes a real 16 Oct lamp-4ace failure (two names for one face), but #1057 says nothing shown on a lamp yet |
| LAMP_ONBOARDING | `on` plus absolute LAMP_ONBOARDING_FILE; off. Walks a new person through seat, name, face, preferences | — | — | #858 | A lamp-52e6 run with a person: seconds per step, redone/skipped steps, framing, setup_step reliability; onboarding.md "Not shown" | off until the lamp-52e6 run with a person | onboarding.md and #858 both list the person run as not shown |
| LAMP_PERSON_NAME | absolute path of the name typed at setup (`/etc/lampOS/person/name`); unset = no name. Told to the model as the name its person gave, and the voice's hello by it when no face is known; passed to the provider and second workers | — | /etc/lampOS/person/name | #1323 | Sunday's reinstall with the owner (#806 sheet, step 6): Welcome back and the first hello use the typed name, and the journal shows its length, never the name | on (in product with #1323; lamp-4ace's unit lacks it) | Setup asks for the name so the lamp can use it (the first builder on #1323) |
| LAMP_POLICY_HOLDS_ACTIONS | `on` only; off. A call that changes the lamp waits up to 2 s for the Policy's decision; held on `stay_quiet` without the name, run past the 2 s | — | — | #908 | Played on lamp-4ace 16 Oct 06:51 on the pre-merge `1ab26f986`, with LAMP_SPOKEN_TO on (nothing ran unnamed; "Hey Lamp, make your light a little brighter" ran in 2 ms; #908, 06:54); merged build from 07:25. **Later, failing both ways**: 11:20 "Set a timer for 1 minute", said to the lamp, was held and no timer set (#1054's rule still holds that turn, letting 17 of 33 timers said to the lamp through); 15:39 a timer said to someone else ran, let through by #1054 (attention.md) | off until both the 11:20 and the 15:39 scenes pass, with `lamp-policy --act attention` in the policy unit | Both later records fail; without `--act attention` every held call waits 2 s and then runs (`policy_holds.rs`), so on its own it only makes calls late |
| LAMP_PROGRESS_NEWS | `on`; off. Voice tells a fresh Policy `say_now` progress step as news | — | — | #765 | tasks.md "Playing progress news on lamp-4ace": a few-minute task, expect `say_now` then one spoken sentence; PR says nothing run on a lamp | off until that play is done, with `lamp-policy --act progress` | #765 "nothing was run on a lamp"; tasks.md most likely wrong: the Policy may `wait` on everything |
| LAMP_ROOM_FACTS | `on` only; off. Model told when the room goes quiet or busy | on | on | commit f7598521b (direct) | none named in a PR; room_told.rs and attention.md cite lamp-4ace scenes 13 to 16 Oct | on (already) | On in both units; the rule behind lamp-4ace's busy-office fixes (attention.md) |
| LAMP_SCHEDULE_SPEAK | `on` or `skill` (waits up to 10 s for a skill's ask); off. Speaks due reminders | on | on | #747 | schedule.md's Task 31 lamp qualification (seven spoken cases); for `skill`, #1038's reminder-in-two-minutes with/without lamp-skills | on (already); `skill` off until #1038's lamp check | Installed on lamp-4ace (install.md); the `skill` value's 10 s wait is #1038's "most likely wrong", no lamp result recorded |
| LAMP_SCHEDULE_TOOLS | `on` only; off. Permits the six Schedule tools (voice and actions) | on (voice, actions) | on (voice, actions) | #747 | schedule.md's Task 31 lamp qualification: off-state minute, scheduling checks, seven spoken cases | on (already) | On lamp-4ace and listed by install.md as product default |
| LAMP_SESSION_FACT | `on` only; off. Tells each new session nobody is talking to the lamp | on | on | commit 0b042f8fd (direct) | none named; RUNBOOK: on since 13 Oct 17:29, "unmeasured" | on (already) | On since 13 Oct; session_fact.rs: its effect on a session's first hearing still unmeasured |
| LAMP_SKILLS_ASK | `policy` (Policy's gate) or `skip` (act unasked); unset = nothing self-started runs (lamp-skills) | skip | skip | #950 | #950: nothing ran on a lamp; device run the first builder's on lamp-4ace 17 Oct | skip (already) until `lamp-policy --gate` is in the product unit and #1155 lands, then policy | skills.md: with `policy` a hello at the first lull is refused; product greets in a busy room only while it skips |
| LAMP_STAFF_NOTE | `on`; off. A request whose heard words name one of the staff gets a hand-over note | — | on (#1332) | #1173 | lamp-4ace with TASKS_WHO/ASKED: Codex haiku, Claude Code poem, code X; count "in time" vs "after its input was closed"; not shown | off until the first builder's in-time/late count on lamp-4ace | tasks.md "Not shown on a lamp yet"; most likely wrong: transcript arriving after the request's end |
| LAMP_TASKS_ASKED | `on` only; off. Work asked of the staff is always handed over; an unclear name asked | — | on (#1332) | #859 | Shown on lamp-4ace 15 Oct 21:22–21:59; `handovers.sh` on 16 Oct: a pass at 03:37 (#903, 03:40), a fail at 06:16 (#854, 06:21), hand-overs 3 of 3 at 06:53–07:07 (#854, 07:09); **newest, failing:** 16 Oct about 21:05 and 22:51, a Claude Code poem and a Codex haiku asked for by name, done by the lamp itself (tasks.md, #1173) | off until `handovers.sh` passes whole on the merged build (told 4/4, offered 0, handed 3/3) | Its runs went both ways on 16 Oct; tasks.md's "Not shown yet" for #903 is stale (these runs) |
| LAMP_TASKS_NEEDS | file path; unset = a missing part is told as not running. Light "not set up" words | — | /usr/local/bin/harness | #1063 | none named on a lamp (#1063 host tests and gate only) | /usr/local/bin/harness (already) | A home lamp without Harness gets plain words (tasks.md, #811); lamp-4ace has Harness so does not need it |
| LAMP_TASKS_TARGET | alias string; default `office`. The tasks target the work goes to | — | — | commit c2ac775f5 (direct) | none of its own; LAMP_TASKS_WHO's lamp-4ace minute used `office-claude` | the lamp's own alias where its `targets.json` names differ; unset (`office`) otherwise | A per-lamp name, not a behaviour |
| LAMP_TASKS_TOLD | file path; default /var/lib/lampOS/voice/tasks-told. Record of task news already told | — | — | commit 55e92e3d8 (direct) | none named | unset (default path) | A storage location, not a behaviour switch (tasks.md, setup.md) |
| LAMP_TASKS_WHO | `on` only; off. The person may name Claude Code, Codex or OpenCode | — | on (#1332) | #769 | Its own minute on lamp-4ace: #769, 15 Oct 17:47–17:49, with `LAMP_TASKS_TARGET=office-claude`. **Newer, with it on and failing:** 16 Oct about 21:05 and 22:51 (stack36, with LAMP_TASKS_ASKED), a Claude Code poem and a Codex haiku, each asked for by name, were done by the lamp itself with no `start_agent_task` (tasks.md) | off until LAMP_STAFF_NOTE's count shows named hand-overs landing on lamp-4ace | Its newest records with it on fail; the miss is the model's (what LAMP_TASKS_ASKED and LAMP_STAFF_NOTE are for), but switching it on alone doesn't make a named hand-over land |
| LAMP_TASK_NEWS | `skill`; otherwise as before. Finished/stopped/asking news waits up to 10 s for a skill's ask | — | — | #1038 | #1038: lamp-tasks --progress, LAMP_SAY, Task news skill folder; news said once via the skill; no record shown | off until #1038's task-news-through-the-skill check is played, after #1237 and #1042 send `task` | Needs the Task news skill (#1042); until a news ask names its task (#1237), a look or dim can go with the wrong task when two settle together (the Second) |
| LAMP_TASK_NEWS_AT_PAUSE | `on`; off. News at the first 1 s pause instead of after 15 s of quiet; since #1223 tasks finished together are told in one turn, at most three | — | on (#1332) | #1161 | lamp-4ace: talk while a task runs; news at the first ~1 s pause, 3 s after the lamp's words, under 20 s; not shown | off until #1161's lamp-4ace check, which also covers #1217, #1223 and #1231 (one play: news at the first 1 s pause; `since_turn_ms` about 3,000 after an unanswered reply; two tasks within a minute told in one turn) | #1161 names rule 8 (news landing between two people talking) as most likely wrong; no lamp result recorded |
| LAMP_TOOLS | `on` only; off. Model may call not_for_me instead of speaking | on | on | commit 995240456 (direct) | none named | on (already) | On in both units since the function-call routing landed (tools.md) |
| LAMP_TOOL_LEAD_IN | `on` only; off. Says the read-back/start of answer before an agent-task call | — | on (#1332) | #810 | lamp-4ace: provider-check --tool-turns with/without, then played hand-over and `turn-times`; 16 Oct 00:03 run failed (since mended), not re-shown | off until the #799 turn-times measurement on lamp-4ace | tools.md: 16 Oct run lost all functions (description over 1,024 B); #799 stays open until measured |

## The voice's own flags (`lamp-voice serve`)

`lamp-voice`'s command line in its unit, beside its environment. Here the code's
default and the units often differ: both units already pass the lamp's choice, so
"make it the code default" is the proposal (the first builder's 07:30: a switch
that passes becomes the default in code). Missing from the first version of this
page; the Second's 07:45 on #1238.

| Switch | Values and default | lamp-4ace (repo unit) | Product unit | Added by | Lamp check it named | Proposed product default | Why (one line) |
|---|---|---|---|---|---|---|---|
| `--turn-taking` | `vad-only` (default) / `v1-baseline` / `duplex`. `duplex`: a person heard over the lamp makes it yield | duplex | duplex | commit 7dde135af (direct) | Runs on lamp-4ace in every scene since 13 Oct (duplex-listening.md) | duplex (already); make it the code default | Both units pass it; `vad-only` is the experiment's first form |
| `--endpoint` | `silence` (default) / `smart-turn` / `smart-turn-fast`. The end-of-turn model judges each pause | smart-turn-fast | smart-turn-fast | commit 6ff31ca1d (direct; `smart-turn-fast` 86e5c275a) | endpoint-pilot.md: the fast model's verdict in 148 ms on lamp-4ace, before the silence window ends the turn | smart-turn-fast (already); make it the code default | Both units pass it; without the model every pause is the window's |
| `--noise-suppression` | `on` (default) / `off`. The capture's software noise suppression | off | off | commit 64529dee4 (direct) | Off on lamp-4ace since the voice came in | off (already); make it the code default | Both units turn it off; the code default disagrees with every lamp |
| `--reference-clock` | `fixed` (default) / `capture`. The echo reference follows the two USB clocks' difference | capture | capture | commit 55d7af98e (direct) | Runs on lamp-4ace since 13 Oct | capture (already); make it the code default | `fixed` is a margin a long session uses up (options.rs) |
| `--reference-recovery` | `off` (default) / `bounded`. Re-centres a reference whose margin was used up | off (lamp-4ace's drop-in passes `bounded` since 17 Oct morning) | bounded (#1332) | commit 55d7af98e (direct) | lamp-cb94 ran an hour clean on `bounded`, 17 Oct 07:21–08:23 UTC: no restart, no untrusted reference (Twelve, #1298); with `off` its voice ended on `ReferenceLate` at 04:53 (#1178) | bounded (#1332) | A held-up speaker no longer ends the capture worker and the voice (#1298) |
| `--echo-reference` | `as-given` (default) / `steady`. `steady` delays the reference to where the echo is | — | — | commit 8a494575f (direct, 14 Oct) | Played on lamp-4ace 14 Oct 18:00–19:35, six runs: with `steady` the lamp paused for nothing 35 times in 52 trials and gave up its own story 6 times, so it stays off there (steady-reference.md) | off (`as-given`) | Shown worse on the lamp; its own doc calls it an experiment, never a default |
| `--provider-continuation` | `off` (default) / `kept-start`. A next request's start kept back while an answer may still come | kept-start | kept-start | commit 2021c1bb9 (direct) | Runs on lamp-4ace since 13 Oct | kept-start (already); make it the code default | Both units pass it; `--early-end-ms` and the cut-off note need it |
| `--controls-stop` | flag; off. A hand on the button or the head stops the lamp's speech | set | set | commit 5f9914314 (direct, 14 Oct) | On lamp-4ace since 14 Oct (controls.md) | on (already) | Both units pass it; a stop by hand is the lamp's quickest control |
| `--early-end-ms` | 100–1000 ms with `kept-start`; off. Sends the end early after that much quiet, plays only when sure | — | — | commit af9ff59c5 (direct, 14 Oct) | Played on lamp-4ace at 200 ms: worse, so switched off (the Second's work log, row 19) | off | The end-of-turn model (`--endpoint smart-turn-fast`) took its place |
| `--call-wait-ms` | milliseconds; off. An utterance that is only the lamp's name ("Hey Lamp,") waits that long for the rest instead of ending | — (lamp-4ace's drop-in passes 2000) | 2000 (#1332) | #891 | Runs on lamp-4ace with 2000 through 17 Oct. On #828 at 13:03 it was cleared as the cut-in regression's cause, 6 of 10 yields with the wait on against 7 of 10 off; at 12:4x, on five cut-ins, the yields were slower with it on (1.6–2.0 s against 0.5–0.9 s) | 2000 (#1332) | A person who says the name and pauses is waited for, not answered with "Yes?" |

## The other parts: body, eyes, expression, tasks and controls

Body flags all say "keep as is until #1192's guard is on the lamp": on 17 Oct a
body exit wrote a bad elbow goal on lamp-4ace, #1192 now refuses such goals, and
its lamp-4ace run is Monday's, with a person there.

| Switch | Values and default | lamp-4ace (repo unit) | Product unit | Added by | Lamp check it named | Proposed product default | Why (one line) |
|---|---|---|---|---|---|---|---|
| `lamp-body --idle` | `on`/`off`; on. A lamp standing at rest shifts a little, every 40 s to 3 min at random | on | on | commit 44d8fea57 (direct; as the crate `motor`) | Ran on lamp-4ace (every 18 s before the random waits); body.md counts it as passing its minute, shown | on (already); keep as is until #1192's guard is on the lamp | Folded it never shifts, and since 16 Oct the waits are random, so it reads as alive, not on a clock (body.md "With --idle on") |
| `lamp-body --wake` | `on`/`off`; on. Brings a joint found off home back slowly, never away from home | on | on | commit ce40ff5ce (direct) | Shown on lamp-52e6 (base woke 311 and 283 counts); lamp-4ace's own pressed-head case not tried | on (already); keep as is until #1192's guard is on the lamp | Without it a lamp found off home refuses to start and has no body (body.md "Waking") |
| `lamp-body --rest-glance` | `on`/`off`; on. A resting head takes Expression's small lift, e.g. at a sound | on | on | #718 | Shown on lamp-52e6 with hand-published requests (head rose 41 counts), and on lamp-4ace folded, 14 Oct 18:40–18:44 (head lifted 49 counts; body.md) | on (already); keep as is until #1192's guard is on the lamp | A resting lamp that lifts its head at a sudden sound and lets it go follows rule 8 (body.md) |
| `lamp-body --look-at-name` | `on`/`off`; on. When its name is heard it rises, looks up, glances once and lets go | on | on | #724 | Shown on lamp-52e6 (name heard 4 of 6, glance 135–204 ms); with a person in view not yet shown | on (already); keep as is until #1192's guard is on the lamp | Rule 8's own example: it glances to one side and the other and lets it go, at most once in 10 s |
| `lamp-body --give-way` | `return`/`stay`/`off`; `return` since 16 Oct 11:21 (`bd7440c4a`, #857). A pushed joint gives way and stops pushing a stalled goal | — (no flag; lamp-4ace's repo unit names a body build older than that default) | — (no flag; the default) | commit 396c31642 (direct) | Shown by hand on lamp-52e6 on 15 Oct (give-way-hand verification); no record of it on lamp-4ace | return (default); keep as is until #1192's guard is on the lamp | It stops a held joint from cooking against its goal and feeds Controls' push signal (controls.md) |
| `lamp-body --limp` | `on`/`off`; on while giving way. A hand forcing a joint makes the lamp slump, as in V1 | — (default on) | — (default on) | #780 | The merged slump shown by hand on lamp-52e6, runs 7–9: "behaves as V1 does" (`docs/verification/v1-limp-lamp-52e6-20261015/`) | on (default); keep as is until #1192's guard is on the lamp | Shown with the merged build; what the person asked for (V1's slump) |
| `lamp-body --overload` | `release` or not given; off. Torque off on all five at sustained 80% load, retaken 120 s later | — | — | commit e14d22b89 (direct) | none named: body.md says "Not yet tried on any lamp" | off until a lamp check | Only simulated servos so far; a slump nobody has seen should not ship by default |
| `lamp-body --torque` | `on`/`off`; off. Takes hold of a joint found with torque off before starting | — | — (a lamp's `lamp.env` may pass it) | commit f694d6ed4 (direct, 14 Oct) | none named on lamp-4ace; lamp-52e6 on 14 Oct showed stale goals 18–39 counts off | off; keep as is until #1192's guard is on the lamp | It writes torque to a joint holding an old goal, the same class of risk as the 17 Oct elbow goal |
| `lamp-body --moves` | absolute folder; none. Loads NAME.json moves, played by name as gestures | — | — | #994 | Add --moves, check the start log, send a gesture on a standing lamp; not yet shown | off (no folder ships) | No move files ship, and who may ask for a move is not yet decided (#994) |
| `lamp-eyes --full-look` | `WxH` with `--full-look-file`; off. One larger still when asked, after the head settles | — | — | #842 | Works end to end on lamp-4ace, answering since 16 Oct 03:17 (#842); held objects shown on lamp-52e6 | on, with LAMP_LOOK_FULL in the voice's units | Shown on both lamps; the two go on together |
| `lamp-eyes --steady-rate` | `on`/`off`; off. Looks 2/s instead of 4/s while one face sits still | — | — | #1114 | Only an offline cost estimate (about 21% vs 11% of a core on lamp-4ace); lamp-52e6 still to measure | off until measured on a lamp | It halves the eyes' CPU but slows gaze correction (1.5 s vs 0.75 s), not yet seen on a lamp |
| `lamp-eyes --leave-still-faces` | `on`/`off`; off. Leaves out a small, faint face at a fixed spot (a false face) | — | — | #766 | Host replay of lamp-4ace's night: 55 of 55 false faces left out, 0 people; not yet live | off now; on after one live night on lamp-4ace | The replay is strong, but a run on a lamp is still owed before it becomes the default |
| `lamp-eyes` LAMP_EYES_SPEAKING | `on`; off. With `LAMP_BUS=on`, each face on `humans/faces/tracked` gets `speaking` (yes/no/unknown, score) from YuNet's five marks; nothing reads it yet | — | — | #720 | Offline only: synthetic faces at 60 px between the eyes and 0.3 px of jitter, looked at every 250 to 350 ms (still, nodding, nodding at a 20° tilt, turning, closer, talking, talking while moving) right in at least 90% of looks; at --steady-rate's 500 ms under half (eyes.md, "Whether a face is speaking") | off until YuNet's corner jitter on a still face at 0.5 and 1 m is measured on a lamp | It holds only for a near face (60 px, about 0.45 m) with steady marks; at 1 m (about 27 px) it does not, on the synthetic model; no reader exists yet |
| `lamp-expression --enable` | flag; off (shadow). Publishes body and light requests instead of only planning | set | set | commit 6226dae33 (direct) | On lamp-4ace with the alive catalogue since 14 Oct, shown | on (already) | Without it Expression only plans, and nothing moves or lights |
| `lamp-expression --head-touch` | flag; off. Leans into a held hand, sleeps on a 3 s hold, wakes on a tap | — | — | #849 | None shown: ARM64 binaries listed for a lamp run, no result recorded | off until shown, with the voice's LAMP_HEAD_TOUCH | Product Controls publish head touches (`--touch`), but nothing has been seen on a lamp |
| `lamp-expression --kernel-clock` | flag; off. Time of day only once the kernel says the clock is set | — | set | #884 | Read-only on lamp-75c9 (chrony keeps the clock); nothing run with the network cut | on (already) | No battery clock, so without it a stale boot clock can bring a false night (install.md) |
| `lamp-expression --offline-face` | flag; off. Listening drawn as not ready while lamp/online says offline | — | — | #978 | none named | off now; on after its minute on a lamp | The light shouldn't say "go ahead" when the lamp can't answer, but no lamp has shown it |
| `lamp-tasks --enable` | flag; off. Serves tasks at all, otherwise "Tasks is off" | set | set | commit 189b744fa (direct) | Shown on lamp-4ace 14 Oct 22:24–22:39 (prepare 35 ms, confirm 38 ms) | on (already) | Handing work to Harness by voice was shown end to end on lamp-4ace |
| `lamp-tasks --watch` / `--no-watch` | watch on unless `--no-watch` (`--watch` changes nothing). Reads each task handed over | `--watch` | `--watch` | commits 45a1e4d2c, fd6c1ae7a (direct) | Shown in the first end-to-end run on lamp-4ace 15 Oct 00:49–00:56 | on (the code default; `--watch` in the units is redundant) | Already the code's default |
| `lamp-tasks --progress` | flag; off. Puts each task's progress on the bus as tasks/status | — | set | #751 | Five lamp-4ace steps named; measured only on the Mac (tasks.md) | on (already in product) | #1156's wake and the Policy's progress need it; its cost on a lamp is not measured on its own (lamp-75c9's "about 1 point" is `--push-wait`'s) |
| `lamp-tasks --push-wait` | flag; off. Holds Harness's wait, so a task's end is known in seconds | — | set | #1157 | Shown on lamp-75c9: idle to settled 8.3–10.2 s vs 38.9–41.2 s without | on (already in product) | Four times faster for about 1 point of CPU and a held ~130 MB Node process |
| `lamp-tasks --close-told` | flag; off. Closes the harness a task started once its answer is told, after a read finds it still idle on that turn | — | — | #1390 | Tests (engine, service, fixture CLI; the voice's news heard whole against a real `lamp-tasks`, #1425). Real Harness 0.4.18, 18 Oct: on lamp-4ace the voice told task 147 and nothing closed, as the news path said nothing (#1425 is that fix); on a laptop, `harness close` refused a finished session as `WORKING` while `show` and `wait` said idle, still open on #1390 | off until a run on a lamp paired with a computer | Without it, every hand-over leaves an idle Claude Code session on the person's computer (52 on the office Mac, 17 Oct) |
| `lamp-tasks --harness-as-person` | flag; off. Calls Harness as the person's own call, without agent markers | set | set | commit b73dfe6fa (direct) | Shown on lamp-4ace 14 Oct 22:24 | on (already) | Otherwise `harness new` is refused unless an account-wide agent setting is on; the owner chose this |
| `lamp-skills --tools-dir` | absolute folder; unset = no skill tools. Skills' own tools offered to the voice | — | set | #984 | skills.md: "not yet run on a lamp" | on (already in product); lamp-4ace's unit lacks it | The product passes it; nothing recorded on a lamp |
| `lamp-controls --enable` | flag; off (shadow). Gestures act, not only watched | set | set | commit 1c765da78 (direct) | Watching only on lamp-4ace from 14 Oct 11:03, then enabled; shown | on (already) | Without it the button and pads say `enabled: false` and nothing acts |
| `lamp-controls --touch` | flag; off. The touch path: head and strip pads, and the reset button held 5 s, on controls/touch | — | set | #728 | Per-pad thresholds, faulty E4/E8, latency listed; not yet shown | on (already in product) | The product needs it for the 5 s reset hold (#1167); without it the reset did nothing |

## What the census found

Gaps between the docs, the units and the code, for whoever owns each:

- **lamp-4ace's repo unit sets less than the docs say lamp-4ace runs.**
  `docs/tasks.md` says `LAMP_TASKS_WHO` and `LAMP_TASKS_ASKED` stay on on
  lamp-4ace (15 Oct 21:30), `docs/attention.md` records
  `LAMP_POLICY_HOLDS_ACTIONS` taken there on the merged build (16 Oct 07:25),
  #997 and #1148 describe head taps (`LAMP_HEAD_TOUCH`) and the 12:20 mute
  (`--controls-mute`), #842 the full look (LAMP_LOOK_FULL with `lamp-eyes
  --full-look`, since 16 Oct 03:17), #1242 `LAMP_START_OFFLINE`, and
  second-voice.md the backup voice's config.
  `deploy/lamp-4ace/lamp-voice.service` sets none of them,
  so they live in drop-ins on the lamp that the repo doesn't have. Before
  switching groups on, the lamp's own drop-ins should be read and copied here.
- **No repo policy unit passes `--act attention`, `--act progress`, `--hello` or
  `--gate`.** So from the repo units alone `LAMP_PROGRESS_NEWS` does nothing,
  `LAMP_SKILLS_ASK=policy` refuses every self-started run at once (so Welcome
  back would never greet; skills.md), and `LAMP_POLICY_HOLDS_ACTIONS` and
  `LAMP_HELLO_ASKS_POLICY` only make what they hold 2 s late: each waits for a
  decision that never comes, then goes on (`policy_holds.rs`).
- **lamp-tasks:** the product unit passes `--progress --push-wait`; the
  lamp-4ace unit doesn't (reverted in `4b8922bbc`, its build being older than
  `--push-wait`), while `docs/tasks.md` says lamp-4ace runs the same. Both
  units pass `--watch`, which changes nothing.
- **Head touch:** product Controls run `--touch`, but product Expression has no
  `--head-touch` and the voice no `LAMP_HEAD_TOUCH`, so on a product lamp the
  head pads reach only the old engine's pat. lamp-4ace's repo units have none
  of the three.
- **`LAMP_SHORT_SOUNDS=quiet-room`** is in both units, the form #800 showed
  answering a room sound; `quiet-room-checked`, which fixes it, was in neither
  until #1332 put it in the product unit.
- **Stale text:** `deploy/lamp-4ace/RUNBOOK.md` says `LAMP_ROOM_FACTS` is "off;
  not yet heard" while the unit sets it on; the "Switch" section of
  `docs/attention.md` says `busy-near` isn't on lamp-4ace while the same file
  records it on since 14 Oct 18:46; `docs/tasks.md`'s "Not shown yet" for
  #903 (`handovers.sh` ran on lamp-4ace on 16 Oct) and its "Nothing was run
  on a lamp" for #769 (its 15 Oct 17:47 minute); test comments in `lamp-body.rs` say
  `--wake`, `--rest-glance` and `--look-at-name` are off unless asked for, while
  the code defaults them on; the units' comments say the Policy's gate "is not
  built yet (#939)" while `docs/skills.md` describes it built (#958).
- **Five of the voice's own flags default in code to what no lamp runs** (`--turn-taking`, `--endpoint`, `--noise-suppression`, `--reference-clock` and `--provider-continuation`): both units pass the lamp's choice, so a lamp started without the unit's line gets the experiment's first form.
- **A product body's motion flags are partly per lamp:** `BODY_POSES` in a
  lamp's `/etc/lampOS/body/lamp.env` can carry flags, and lamp-06c1's passes
  `--torque on`.
- **Small ones:** `LAMP_SCHEDULE_SPEAK` is defined twice (`reminders.rs` and
  `tools.rs`); `LAMP_ONBOARDING` has no doc comment; #987
  (`LAMP_LOOK_FULL_SIDE`) names no lamp check.
- **`LAMP_SCHEDULE_SPEAK=skill` never speaks a reminder** (the reviewer on
  #1238, a probe test on #1038, 08:28; the Second's fix is #1253): the coordinator takes `skill` as
  speaking (`reminders.rs`), but only `on` passes `--schedule-speak` to the
  provider worker (`tools.rs`), so the worker defers every due reminder as
  disabled (`provider_worker.rs`, `reminder_policy.rs`). It came in with
  #1038, and agrees with that row's "`skill` off".
- **`LAMP_KERNEL_CLOCK` never reached the provider worker:** it is spawned
  with `env_clear()` and the switch isn't passed on (`process.rs`), so the
  worker's `the_time` and session note never saw the clock unset (Ten on
  #705, 17 Oct). Fourth passes it in #1158.
- **#1332 brought the product voice unit up to lamp-4ace's** (the first
  builder, 17 October 15:41, for Sunday's reinstall and every product lamp):
  20 switches lamp-4ace ran all through 16 and 17 October, `--reference-recovery
  bounded` (after lamp-cb94's crash, #1298) and `--call-wait-ms 2000`. Their
  "Product unit" cells say "(#1332)". For several of them, this page's
  proposal was "off until" a named lamp check that no doc records yet
  (`LAMP_STAFF_NOTE`, `LAMP_TASKS_WHO`, `LAMP_LIGHT_REQUESTS`,
  `LAMP_FIRST_WORDS` among them). The first builder chose to ship them as
  lamp-4ace runs them, and the proposals stay as written, for each one's
  check. Left out of the product: what needs the second voice's
  configuration, the office's own, head touch and `--controls-mute`.
  **Six more were left out at #1332's review** (Nine's P2, `a9c9153ac`),
  because the product's other parts don't pass the flags they need:
  `LAMP_POLICY_HOLDS_ACTIONS`, `LAMP_HELLO_ASKS_POLICY` and
  `LAMP_PROGRESS_NEWS` need the policy unit's `--act attention`, `--hello`
  and `--act progress`, which it doesn't pass, so they would only make what
  they hold 2 s late or do nothing; `LAMP_LOOK_FULL` needs the eyes'
  `--full-look` and its file; `LAMP_RESTORED_MUTE` needs `--controls-mute`.
  `LAMP_MARK_WEIRD` is left out because a reset doesn't wipe its marks.
