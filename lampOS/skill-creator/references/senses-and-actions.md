# What a skill can sense and do

Senses are topics on the lamp's bus; actions are services. Only what is
listed here exists. The source of truth is the lamp's own `lampOS/docs/msgs.md`
(each message's fields) and `lampOS/docs/bus.md` (who publishes what); if
they disagree with this file, they are right.

## Senses (topics)

| Topic | What it says | Good for |
|---|---|---|
| `humans/faces/tracked` | The faces in view: a passing id, where, whether turned to the lamp. Said a few times a second while the camera is open | Someone arrives, is there, leaves; looks at the lamp |
| `voice/heard`, `voice/said` | What a person said, what the lamp said (text) | Rarely: a trigger matches the whole text exactly, so it cannot hear a request in someone's own words or a number in it. For "when I ask, do X", use a `tool` (skill-folder.md); a spoken timer or reminder ("focus for 25 minutes") is the voice's own (`set_timer`), not a skill's |
| `voice/state` | Resting, listening, thinking, speaking | Keeping out of a conversation |
| `room/state` | Unknown, quiet, busy | Doing nothing in a busy room |
| `room/presence` | Whether anyone is at the lamp: `someone` (a face seen or a voice heard within 20 s) and `nobody_s`, how long nobody has been there. Once a second while the eyes look (`LAMP_PRESENCE=on`), nothing while they don't: no value is not known, never nobody | Something for when nobody has been there a while (`nobody_s` above a line), as Light out does |
| `name/heard` | The lamp's name was heard, how sure | |
| `controls/gesture`, `controls/state` | A touch gesture on the head or base; button holds | Answering a touch |
| `ears/level`, `ears/sudden` | The room's loudness every 100 ms; a sudden sound | Quiet hours, a bang |
| `air/state` | Every 5 s: `co2_ppm`, `pm2_5` (and `pm1_0`, `pm4_0`, `pm10`), `humidity`, `temperature` (inside the base, warmed by the lamp: not the room's), and `feel`, the lamp's own judgement from the CO2: `fresh` (under 800 ppm), `fine` (to 1,200), `stuffy` (to 2,000), `very_stuffy`, `unknown`. Any of the numbers can be missing | Stuffy air: trigger on `feel`, the lamp's own word, rather than a number of your own |
| `schedule/due` | How many reminders and timers are due (`pending`), and whether Schedule really serves them (`enabled`; false in shadow mode). Not which one or what kind: a skill cannot tell its own timer from any other (#939) | Answering any due reminder or timer |
| `tasks/status` | How work handed to Claude Code or Codex is going | Task news |
| `body/joint_states`, `body/hand` | Where each joint is; a hand pushing the lamp | Not moving against a hand |
| `light/state` | The light the person asked for | |
| `mood/state` | The lamp's mood | |
| `lamp/pause` | The lamp is paused: do nothing. Said only when it is paused or resumed, so after a restart there is no value | Not usable in a `[[while]]` yet: with no value the condition fails and the skill never acts. The pause waits for the Policy's gate (#939) |
| `policy/decisions` | What the Policy decided about speaking first | |

## Actions (services)

| Service | What it does |
|---|---|
| **say** (the voice) | The lamp says something, as a written turn through the voice's own gates (the say service, #954; the voice runs it with `LAMP_SAY=on`). Give it a note for the model ("They are back. Greet them in one short line."), not a script |
| **light** (the actions part) | A colour for a while, or dimmer by a step: a short change on top of the person's own light, which is given back after (skill-folder.md). Its level is not set yet |
| **look** (the voice) | Only in a skill with a `tool` and no trigger: the voice's full look at what is in front of the lamp, for the model's answer |
| **body: gaze** | Look at a face, or a direction |
| **body: gesture** | One of the body's six gestures: `nod`, `headshake`, `greeting`, `laugh`, `shy`, `stretching`. There is no droop or bow yet: those come as move files |
| **body: pose** | `reading`, held for a while: the head down over the desk and the arm leaning forward (#1011). The only pose a skill may hold |
| **body: moves** | Not yet: move files wait for #940, and the runtime refuses `body.move` |
| **schedule** | Set, list, cancel reminders and timers |
| **tasks** | Hand work to Claude Code or Codex |
| **memory** | What the lamp remembers |
| **eyes: who** | Whose face is at the lamp, if known |

The camera's picture (`camera/image`) is not published yet: a skill cannot
look at the picture itself, only at the faces the eyes report.

## Names in tests

A skill's steps, and its tests, name seven calls: `body.gaze`, `body.gesture`, `body.move`,
`body.mood`, `light`, `say` and `look`. Schedule, tasks and memory are services the lamp has, but
a skill's steps cannot call them yet.

## The light's colours

`yellow`, `red`, `green`, `blue`, `cyan`, `purple`, `orange`, `pink`,
`white`, `warm`; levels 1 (dimmest) to 5.

## Never

- Never write to `light/command` (that is the person's light) or to a device.
- Never call a service the lamp does not have; never invent a topic.
- Never put a number for the model to compare in a `say` note.
