# What the lamp's voice can do itself, and what each door carries

Facts about the lamp's functions and contracts, for labelling the routing
asks. Paths are in `lampOS/` on `origin/subdarren/task-progress`. This page
describes what exists. It does not say how today's model routes any ask.

## Setup the asks assume

- The second route, the product's voice path: DeepSeek chat with the lamp's own functions (`deploy/product/lamp-voice.service:111`, `:113`).
- `LAMP_HARD_ASKS_TO_AGENT=on` in lamp-voice and lamp-tasks, which offers
  `ask_agent` (`docs/switches.md:399`).
- One language per lamp (`crates/voice/src/language.rs:1`; it supports five, and these asks use English and Vietnamese).
- Every request already carries the time and the date here
  (`docs/switches.md:542`).

## The voice's own functions, by `acts` class

| `acts` | Functions | Notes |
|---|---|---|
| `light` | `light_on`, `light_off`, `light_colour`, `light_brighter`, `light_dimmer`, `light_warmer`, `light_cooler` | `crates/actions/src/lib.rs:208` |
| `volume` | `volume_up`, `volume_down`, `volume_mute`, `volume_unmute` | The lamp's own speaker. `crates/actions/src/lib.rs:190` |
| `timer` | `set_timer` | `crates/actions/src/schedule.rs:94` |
| `reminder` | `set_reminder`, `repeat_reminder`, `list_reminders`, `cancel_reminder`, `snooze_reminder` | There is no alarm function: "wake me at…" is `set_reminder`. `crates/actions/src/schedule.rs:95` |
| `memory` | `remember`, `recall`, `forget` | `remember` keeps what the person asks it to remember or keep in mind for later, as one short sentence per call (`crates/voice/src/remember.rs:3633`, `:3674`); `recall` reads kept things back by voice (`:3479`). Needs `LAMP_MEMORY_SOCKET` (`lamp-voice.service:482`). A keep about a person needs a known face in view (`lamp-voice.service:485`). `crates/voice/src/remember.rs:80` |
| `time` | `the_time` | Needed only for the time in **another place**; the time and date here are in the request. `crates/voice/src/tools.rs:661` |
| `weather` | `weather` | Any place, today, tomorrow or a weekday. `crates/voice/src/second_provider/net/weather.rs:30` |
| `search` | `web_search`, `fetch_page` | One search waits at most 3 s; `fetch_page` reads 2,000 characters of a page. `crates/voice/src/second_provider/net/search.rs:25`, `:29`, `:34` |
| `skill` | `use_skill` with `skill: "lamp_help"` | Says what the lamp can do. `skills/faq/skill.toml:16` |

The voice has no function for music, phone calls, messages, shopping, files
or code.

## The two doors to the lamp's agent

| Door | Function | What it carries |
|---|---|---|
| `ask` | `ask_agent` | The agent works the ask out and its answer is **said** by the lamp: at most three short plain sentences, no list, link, markdown or code (`crates/tasks/src/voice_ask.rs:294`), cut at 640 bytes (`crates/voice/src/say.rs:207`, `crates/voice/src/agent_asks.rs:418`). It writes no file. |
| `task` | `start_agent_task` | The agent does or makes something: research written up, code, files, several steps (`crates/voice/src/agent_tasks.rs:2066`). |

- One ask runs at a time; the next is refused until its answer is told
  (`crates/voice/src/agent_asks.rs:93`). An ask may take up to 120 s
  (`crates/tasks/src/voice_ask.rs:97`).
- A hand-over is never for something refused to keep someone safe or
  private (`crates/voice/src/agent_asks.rs:243`).
