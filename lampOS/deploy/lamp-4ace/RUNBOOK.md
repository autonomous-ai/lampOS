# lamp-4ace (office prototype): how to run it

A copy of the runbook kept beside the deploy scripts, with the lamp's address, user and key file left out.

Everything below is run from this iMac. The lamp is `USER@LAMP_ADDRESS`; the key is `~/.ssh/LAMP_KEY`.
Short form used below: `L` = `/usr/bin/ssh -i ~/.ssh/LAMP_KEY -o IdentitiesOnly=yes -o UserKnownHostsFile=~/.ssh/LAMP_KNOWN_HOSTS USER@LAMP_ADDRESS`

## What runs on the lamp

| Unit | What it is | Starts at boot |
|---|---|---|
| `lamp-voice.service` | Voice and the light ring (`lamp-voice serve`). Restarts itself 4 s after a failure. | yes |
| `lamp-body.service` | All motion (`lamp-body`): stands the lamp up when it is spoken to and folds it down a minute and a half after the last word, moves the head for listening, thinking and speaking, turns it to a face the eyes report, and makes gestures (nod, head shake, greeting, laugh, shy, stretch). A head joint that meets resistance gives way for two seconds and goes on; a shoulder or elbow that strains, lags or reaches 62 °C sends the lamp back down for a minute. On a load that persists or repeats, a new servo flag, heat or a silent wire it stops itself and stays stopped until started again. | no |
| `lamp-eyes.service` | The head camera (`lamp-eyes`): finds a face and tells the body where it is. Opens the camera only while the voice service's microphone is permitted. Stores no picture and sends none anywhere. | no |
| `lamp-cmedia-speaker.service` | Sets the speaker level. From V1; V2 needs it. | yes |

V1 is gone from the lamp. Its files are backed up in `~/lamp-4ace-v1-backup-20261012/`.

## Talking to it

Start with its name: "Lamp" or "Hey Lamp". Since 2026-10-13 16:05 it answers only when called, or when you are already in a conversation with it (it said something a moment ago and you carry on). In an office it otherwise joins in with everyone. English only.
Speak at an ordinary voice from the desk: sound quieter than about -28 dBFS at its microphone (someone at the next desk) is not taken for speech.
It can look things up (weather, prices, news): those answers take three to five seconds.
The only mute is the physical privacy switch: closed means it cannot hear, and the ring goes dark. To silence it from here: `L 'sudo systemctl stop lamp-voice.service'` (and `start` to bring it back).

What the ring shows:

| Ring | Meaning |
|---|---|
| Slow amber breath | Awake and able to hear |
| Cool blue-white, brighter as you speak | It is hearing you |
| Amber point circling | You stopped; it is about to answer (or deciding it was not for it) |
| Warm glow moving with its voice | It is speaking |
| Closes for a moment, then breathes | It let that one go: not for it, or no answer |
| Dark | Privacy switch closed, or the voice service is down |

## Common things

- Is it up: `L 'systemctl is-active lamp-voice.service; systemctl show lamp-voice.service --property=NRestarts'`
- Restart voice: `L 'sudo systemctl restart lamp-voice.service'` (listening again in about 3 s)
- Last log lines: `L 'sudo journalctl -u lamp-voice.service -n 30 --no-pager -o cat'`
- Is the body moving: `L 'systemctl is-active lamp-body.service'` (`failed` means it stopped itself; why: `~/lamp-voice-data/tools/body_report.sh`)
- Start the body: `~/lamp-4ace-v2-deploy-20261012/body_start.sh` Stop it: `~/lamp-4ace-v2-deploy-20261012/body_stop.sh` (a standing lamp folds down first; allow ten seconds)
- What the body is doing: `L 'sudo journalctl -u lamp-body.service -n 5 --no-pager -o cat'`. Each line has `stand` (0 folded to 1 standing), the shoulder's and elbow's goals, positions, loads and `warmth` in °C; `gesture NAME` when it makes one; `face at ...` when the eyes report one.
- Start the eyes: `~/lamp-4ace-v2-deploy-20261012/eyes_start.sh` Stop them: `eyes_stop.sh`. What they see, every ten seconds: `L 'sudo journalctl -u lamp-eyes.service -n 3 --no-pager -o cat'` (frames, looks, how many had a face).
- After a restart of the lamp: voice comes back by itself; the body and the eyes do not. Start the body, then the eyes.
- Read the servos without moving anything: `L 'cd ~/.local/share/lampOS-voice && ./motor-inspect-50e8de4e/lamp-body-inspect --device /dev/ttyACM0 --unit-id lamp-4ace --lock-directory ~/.local/share/lampOS-voice/motor-locks --status-faults report'` (only while the body is stopped: one owner of the servo port at a time)

## Prepared private-memory configuration (#3064)

The repository's Voice unit prepares `LAMP_SECOND_SCOPED_CONTEXT=on`,
`LAMP_CAMERA_FOLLOWS_EYES=on` and `LAMP_EYES_SPEAKING=on`; Eyes prepares
`LAMP_EYES_SPEAKING=on` and `--steady-rate off`. The exact retained Eyes
pin's emulated CLI accepts all 16 unit flags, including the on/off cadence
syntax ([receipt](../../docs/verification/common-checking-20261023/README.md)). These are source settings,
not a claim that the running lamp or its drop-ins have changed. Compatible
qualified Voice/Eyes artifacts and exact executable pins must be selected
before installation; a flag added to source does not upgrade an old binary.
The new known-person lamp run, with its exact builds/settings and counts/times,
remains outstanding. No lamp installation accompanies this source preparation.

Both context modes retain the request's final positive speaker proof for name
and history. Scoped context also gives private Days reads their lease; a name
or history reply with it off does not demonstrate Days recall. Regular Eyes
cadence avoids starving the five-look still-face speaking window in source;
its unit contract must pass with the rest of the current-source gate. Its extra CPU/temperature and effect on voice
responsiveness are unmeasured on this setup.
No FACE_NAME, Photo or repair setting is enabled. Product and binary defaults
are unchanged, and the historical records below are not this gate's evidence.

## Switches added on 2026-10-13

All in `lamp-voice.service` in this folder unless said otherwise; install a change with `./deploy.sh releases/<current sha>/lamp-voice <label>` (a silent restart of about five seconds; the conversation is kept across it).

| What | Switch | Now |
|---|---|---|
| Answer only when called | the line beginning "You share an office" in `persona.txt`; the looser line is in `persona.txt.before-office-rule-20261013` | on |
| How loud counts as speech | `--faint-floor-db N` (-47 to -20) | -28 (was -40) |
| Let that level follow the room | `Environment=LAMP_FAINT_FLOOR_MAX_DB=N`: the level then moves between `--faint-floor-db` and N with the talk that is not for the lamp. Without it the level is fixed, and the record's `room_floor` rows still say where the room alone would put it | off (watching) |
| Web search | `"search": true` in `/etc/lampOS/voice/provider.json` (`~/lamp-voice-data/tools/set_provider.sh search false` turns it off) | on |
| Say hello to someone who sits down | the Welcome back skill (`lamp-skills.service`); the voice's own hello and `LAMP_SPEAK_FIRST` were removed (#2825). `lamp-policy.service` keeps `--hello --act hello`: nothing publishes `voice/arrival` any more, but `--gate`, which answers Welcome back's ask, needs both | Welcome back since 16 Oct 10:15 (#967) |
| Take a look when asked ("Let me look") | `Environment=LAMP_EYES_LOOK_FILE=...` here, `--sight-file` in `lamp-eyes.service`, and the "You can see, when you look" line in the character (`lampOS/deploy/character/lamp.txt`) | on since 2026-10-15 |
| Keep the conversation across a restart | `Environment=LAMP_THREAD_FILE=/run/lampos-voice/thread.json` | on |
| Tell each new Gemini session that nobody is talking to the lamp | `Environment=LAMP_SESSION_FACT=on` (the journal then says "told the new session" each time) | on since 17:29 on 2026-10-13; unmeasured |
| Answer without being called when the room is quiet, only when called when it is busy | `Environment=LAMP_ROOM_FACTS=on`, with the line beginning "Now and then you are told how the room is" in `persona.txt` (the journal says "told the session" at each change) | off; not yet heard |
| Priority over other work on the lamp | `CPUSchedulingPolicy=rr`, `CPUSchedulingPriority=20` | on |
| English only | the "Always speak English" line in `persona.txt`; the two-language file is `persona.txt.bilingual-20261013` | on |

What each one is and what has been shown of it: `lampOS/docs/` in the repository (`speaking-first.md`, `not-speech.md`, `group-talk.md`, `character.md`, `conversation-thread.md`, `serve-runtime.md`).

## Settings and where they are

| Setting | Where | Now |
|---|---|---|
| Voice options | `lamp-voice.service` in this folder (source of truth; `deploy.sh` installs it) | mic2, duplex, 600 ms end-of-speech wait, 350 ms playout start |
| Ring brightness | `--ring-channel-ceiling N` in that unit, 0 to 120 | 24 |
| Model, voice, language, timeouts | `/etc/lampOS/voice/provider.json` on the lamp | gemini-3.8-live, Kore, en-US, thinking budget 128 (with thinking off the lamp answers speech not meant for it), no reply after 4 s is "not for me" |
| Character | `persona.txt` in this folder (installed to `/etc/lampOS/voice/persona.txt` by `deploy.sh`) | the long version with three rules added on 2026-10-12 and 13: what to do when interrupted, what is and is not meant for it, and that its name can sound like "Helen". Why each is there and how to check them: `lampOS/docs/character.md`; the same text is in the repo as `lampOS/deploy/character/lamp.txt`. Earlier versions are kept here as `persona.txt.before-*`; `persona-short.txt` is a shorter one not in use |
| Body reach and speed | `--reach` and `--speed` in `lamp-body.service` in this folder (`body_deploy.sh BINARY 1950,2043,2015` installs it; stop the body first), within caps fixed in the binary (`lampOS/docs/body.md`) | pitch 150 up and 15 down (150 down once standing), roll 1 (off), yaw 250 counts; 120 counts/s; servo speed 1000 so gestures are not held back |
| Standing | `--stand PITCH,ROLL,YAW,SHOULDER,ELBOW --folded-arm S,E --stand-share N --stand-seconds N --awake-seconds N` in that unit | the old software's idle pose `1772,2043,2015,2657,2602`, folded arm `2048,2047`, all of the way, 6 s for the way, stays up 90 s after the last word. To bring a lamp up carefully, set `--stand-share` to 10, 30, 60 in turn |
| Eye contact | `--gaze-socket /run/lampos-body/gaze.sock --gaze YAW,PITCH` in that unit; `lamp-eyes.service` in this folder (`eyes_deploy.sh BINARY MODEL`); `LAMP_EYES_PERMIT_SOCKET` in `lamp-voice.service` | `--gaze 727,553`, measured on this lamp on 2026-10-13 (`lampOS/docs/body.md`, "Looking at a face"). Not yet seen with a person in front of it |
| Gestures | chosen by the voice service from the first words of its answer; nothing to set | nod, headshake, greeting, laugh, shy, stretching (`lampOS/docs/body.md`, "Gestures") |

To change a voice option: edit `lamp-voice.service` here, then `./deploy.sh releases/<current sha>/lamp-voice <label>`.
To deploy a new build: `./deploy.sh PATH/TO/lamp-voice <commit>`. Each build is kept under `releases/<sha256>/` here and `/opt/lampOS/releases/<sha256>/` on the lamp.
To go back: `./deploy.sh releases/<older sha>/lamp-voice <label>`; older unit files are kept here as `lamp-voice.service.<note>`, older configs on the lamp as `/etc/lampOS/voice/provider-<note>.json`.

## Do not

- Do not reboot or power-cycle the lamp without a hand on it: what the servos do at power-up has not been checked on this unit.
- Do not send goals to the servos from anything but `lamp-body`. It is what keeps the head off the base, sits the lamp down when a joint strains, and leaves a folded arm resting. (The elbow's overload flag from 2026-10-12 is gone: it cleared once the elbow was told to stop pushing.)
- Do not run two things that open `/dev/ttyACM0` at once, or two that open `/dev/video0`.
- Do not run played tests while people are working in the office: they are loud, and on 2026-10-13 a day of them was a nuisance to everyone. Silent checks: `lamp-voice provider-check CONFIG --written "NOTE"` on the lamp prints what the model would say and plays nothing.
- Do not leave long test runs going unwatched: on 2026-10-13 a night of them spent the Gemini Live quota and the lamp was silent for 100 minutes. After any long run check `NRestarts` and that it still answers.

## Test tools on this iMac

- `~/lamp-voice-data/tools/scenario.py NAME "Voice|Text|seconds to wait" ...` plays lines from the iMac speakers and prints what the lamp heard, said and when.
- `~/lamp-voice-data/tools/soak.sh ROUNDS LABEL` plays rounds of questions for a long run.
- `~/lamp-voice-data/tools/checks.sh LABEL` runs every played check in one go (about 20 minutes) and prints what to expect beside what happened. Run it after changing the character file or the voice build.
- Added 2026-10-13: `plain.sh LABEL` (seven ordinary questions, prints how soon it answers), `lookup.sh LABEL` (six things only a web search can answer), `sounds2.sh LABEL` (short sounds it should not answer), `hold.sh LABEL` (a staged busy room), `busy.sh LABEL` (six questions by name for a room with other talk), `look.sh LABEL` and `hello.sh` (taking a look; saying hello first), `people.sh LABEL` (what people asked it at the lamp that morning).
- `~/lamp-voice-data/tools/quiet10.sh LABEL` plays ten lines, three of them not for the lamp. `cutins.sh LABEL` plays ten story requests and speaks over each answer. `stories.sh LABEL` plays long stories with nobody speaking over them.
- Photos of the lamp: `/usr/bin/open -n -W -a ~/Applications/LampCamera.app --args OUT.jpg FaceTime`.
- Gestures by hand: with the lamp standing, `L 'python3 -c "import socket; s=socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM); s.sendto(b\"{\\\"gesture\\\":\\\"nod\\\"}\", \"/run/lampos-body/mood.sock\")"'`.
- Recordings, prompts and scene outputs are under `~/lamp-voice-data/` (see `INDEX.md`).
