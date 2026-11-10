# A skill's own code, in a sandbox on the lamp (#6071)

The owner's decision on #6071 (option 2): a skill may carry a small program
run in a sandbox on the lamp, for what no step kind says. #6066's four stays
(10 Nov 19:44) hold: hardware safety; said back before it goes on, naming
outbound, camera and mic use; privacy; "can't yet", never "not allowed".
Nothing here is wired yet.

## The idea: the code computes a plan, it never acts

A run gets the trigger's event and the skill's small state, and returns the
`Call`s a static `[[do]]` makes (`crates/skills/src/lib.rs`, `enum Call`).
`lamp-skills` makes that plan as it makes steps today: each call held by
`Permissions::refuses`, then sent to the body's gaze and mood sockets, the
actions socket, the say socket or the bus. No second API, no second
permission check; a run that breaks a limit is dropped whole.

## The runtime and the language: Rhai, not WASM

Measured 10 Nov on the Mac (ARM64), release, stripped, a host with one
host function, over a 0.34 MB `println` binary. The Mac has no aarch64
Linux linker: the OrangePi 4 Pro (A76/A55) figures are to measure.

| | Added to a binary | Crates pulled | Compile/load | 1M-iteration loop |
|---|---|---|---|---|
| `wasmi` 2.0.0 (interpreter, fuel) | 2.24 MB (2.69 with `wat`) | 15 | 0.9 ms (WAT parse + validate) | 2.7 ms |
| `wasmtime` 50.0.1 (Cranelift JIT, fuel) | 11.9 MB | 132 | 6.3 ms | 0.8 ms |
| `rhai` 1.27.0 (embedded script) | 1.24 MB | 24 | 0.4 ms | 96 ms (6M operations) |

- **`wasmtime` is out:** 12 MB and 132 crates for speed a skill never
  needs, and a JIT writing executable memory on a CPU shared with voice.
- **`wasmi` is the better WASM, but WASM needs a language.** No Rust
  toolchain runs here; WAT is assembly (strings are pointers into linear
  memory), which a model writes badly and can't be told plainly what it got
  wrong; a small language compiled to WASM on the lamp is a compiler to keep.
- **`rhai` is the pick:** pure Rust (AGENTS.md: no native code), the
  smallest, and its limits are built in (`set_max_operations` as fuel,
  `on_progress` as a wall-time stop, call and expression depth, string,
  array and map sizes). `Engine::new_raw()` has no library: no files,
  network or clock, only what we register. Its syntax is close to Rust and
  JavaScript, which models write well, and a parse error names a line the
  repair brief can quote. 36 times slower than `wasmi` doesn't matter: a
  run is a few hundred operations. WASM stays a later door for Store
  developers who compile off the lamp: a `wasmi` backend returns the same plan.
- Built with `no_module`, `no_closure`, `no_custom_syntax`, and `eval`,
  `Fn`, `call` and `curry` disabled, so every function a script calls is
  named in its text. `unchecked` (no limits) is never on.
- The bare engine has operators, `if`, `while`, `loop`, maps and string
  `+`, but no ranges for `for`, no `len`. Any library is added function by
  function: Rhai's own core package has a blocking `sleep`, which no
  script gets.
- The spike, `crates/skill-code` (not in the image): `check` and `run`
  with these limits, `light` and `say`; its tests stop an endless loop by
  fuel and by wall time, refuse a doubling string, a growing array and
  endless recursion, and refuse an unknown call (`send_text`) before
  anything runs.

## The lamp API: one host function per step kind

Each records one `Call`, needs its step's grant, and has its inputs checked
as the step's are (a known clip, a sound in `sounds/`, a level 1 to 5).

| Host function | `Call` | Grant |
|---|---|---|
| `light(colour, level)`, `light_for(colour, level, secs)` | `Light` | `light` |
| `dim(steps)`, `light_out()` | `Dim`, `Out` | `light` |
| `say(text)` (as written), `note(text)` (in the voice's words) | `Words`, `Say` | `speak` |
| `sound(name)`, `hum()` | `Sound`, `Cue` | `speak` |
| `gesture(name)`, `play_move(name)`, `pose(mood, secs)`, `look_at_face()`, `stand(percent)` | `Gesture`, `Move`, `Mood`, `Gaze`, `Stand` | `body` |
| `look()` | `Look` | `camera` (only in a skill its tool starts, as today) |
| `read(service, what)` | `ServiceRead` | `services = { s = "read" }` |
| `after_say()` | `AfterSay` | none |

A run reads `event` (the trigger and its message's fields, from topics in
`[permissions] topics`), `now` (local hour, minute, weekday) and `state`,
kept per skill (at most 2 KiB), so "the third time today" can be counted.

It never gets a picture, words heard, memory, a face's name, a service's
content (`read` is said to the owner alone behind its gate; the code only
asks), a socket, a file or the network. A send or a service write is
**can't yet**; #6066's send step becomes `send(...)`, through the send
gate's read-back. Any call the lamp lacks is refused at the make as "the
lamp can't X yet" and journaled for #6066's list of step kinds.

## Limits, and the kill

| Limit | Value | How |
|---|---|---|
| Operations a run | 100,000 (1.6 ms on the Mac) | `set_max_operations` |
| Wall time a run | 50 ms | `on_progress` checks the clock and stops it |
| Memory | string 1 KiB, array 64, map 64, call depth 16, expression depth 32/16, variables 64; source at most 8 KiB | Rhai's limits; the worker's `MemoryMax=48M` |
| Calls a run | at most 16 in a plan, one say | checked on the plan |
| Runs | one a skill each 5 s, beyond its trigger's cooldown | `lamp-skills` |
| A hung worker | no answer in 200 ms | the run dropped, the worker restarted |

A breach ends the run and nothing from it is made. Three in a local day
take the skill off, said once: "I turned off NAME: its code ran too long."

## Where it runs

A separate long-lived process, **`lamp-skill-code`**, its own unit, asked
by `lamp-skills` over a Unix socket (`{code_sha256, source, event, now,
state}` in, `{calls, state, ops, ms}` or `{error}` out). It holds no socket
to the body, light, voice or bus: even an escape acts on nothing.

- `Slice=lampos-parts.slice`, `Nice=15`, `CPUWeight=20`, `CPUQuota=25%`,
  `MemoryMax=48M`, `MemorySwapMax=0`, `TasksMax=4`, `PrivateNetwork=yes`,
  `RestrictAddressFamilies=AF_UNIX`, `PrivateDevices=yes`,
  `ProtectSystem=strict`, `ProtectHome=yes`, `PrivateTmp=yes`,
  `NoNewPrivileges=yes`, `SystemCallFilter=@system-service`; its socket
  open to the lamp-skills user alone.
- The voice runs in `lampos-voice.slice` (`CPUWeight=1000`, `SCHED_RR` 20);
  a script at its limit on every trigger is at most a quarter of one core
  in the low slice. In-process would save a round trip (under 1 ms) but put
  the code in the process holding the body's and the say's sockets.

## The make, the say-back and the plain failure

With `LAMP_SKILL_CODE=on`, the brief (`made_skill::brief`) may add a third
block, ```` ```code.rhai ````, naming the host functions, for what steps
can't say. Then: **parse** in the worker (an error goes back once in the
repair brief with its line, as `lamp-skills pack`'s do); **uses**, the AST
walked for every called name, each a host function or the script's own,
any other "the lamp can't X yet", and the grants they need are what
`[permissions]` must give; a **dry run** on a sample event of its trigger,
nothing sent, its plan in limits and passing `Permissions::refuses`; then
kept and put on as today (`keep`, `put_on`, the manage helper by SHA-256).
A failure is one line: "I couldn't make that: its code didn't finish in
time." or "…: the lamp can't send a text yet."

`plainly` says the trigger as today, then the code by what it may use,
from the AST, never the model's description: "When you come back, it runs
its own code, which may set the light and speak." Camera, microphone (a
trigger on heard words) and outbound (a service read) are named each time.
It says what the code may do, not each thing it will do (open question 1).

## The Store and the switch

A package lists `code/main.rhai` with its SHA-256 in `lamp-skill.json`, so
the Store's signature covers it. The backend runs the same check (this
crate as a library: parse, uses, dry run on the trigger's fixtures) and
shows the reviewer the uses beside the code (at most 8 KiB). The lamp
checks again at install and load, whatever the signature.

`LAMP_SKILL_CODE`: `on` only, off by default, read by `lamp-skills`,
`lamp-skills-manage@` and `lamp-voice`. Off: no code block in the brief,
the worker not started, and a skill with code not loaded, said as "This
lamp can't run a skill's own code yet."; nothing else changes. It goes in
`docs/switches.md` and every census test.

## Latency targets (CLAUDE.md rule 7)

- Trigger to first output: at most 10 ms over the same skill's steps, p95
  on the OrangePi. The worker starts with `lamp-skills` (under 100 ms),
  never on a first trigger.
- The make's parse, uses and dry run: under 300 ms, in today's 30 s budget.
- A breach stops in 50 ms; a hung worker is dropped in 200 ms; voice turn
  latency is unchanged with one skill at its limit every 5 s.

## Rollout

1. This note; a spike, `crates/skill-code`, not in the product image.
2. The worker binary, unit and socket, behind the switch.
3. A `code` step in `lamp-skills`; then the make (brief, check, dry run, `plainly`).
4. An in-house lamp: the latency targets, a runaway script, voice
   unaffected; then TWO READS (say-back and privacy).
5. The Store's check and review. Turning it on is the first builder's call.

## Open questions for the first builder

1. Is "it may set the light and speak" enough of a say-back for code, when
   `plainly` today refuses anything it can't say step by step?
2. May `event` carry more than presence (a face count, a known face's yes
   or no, never a name)?
3. Is a 2 KiB state a skill keeps across runs acceptable?
