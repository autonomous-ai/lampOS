# App tools row v1 (lampOS #8386): Gym Coach (beta) 2.0 on lamp-ee17

The tester is Twelve on lamp-ee17. App Only runs `eval_shipped.py` on the lamp. Files:
- `row-app-tools-v1.sh`: the switch on, show and off, the leg marks, and the journal and trace reads.
- `eval_shipped.py`: the phrasings against the shipped route, sent with the voice's exact app declarations.
  It runs on the lamp (#8386, option b).
- `phrasings.jsonl`: the 84 eval lines (EN + VI), copied from gym-store `eval/phrasings.jsonl` on gym-coach-2.

## Before the kit

The lamp update round is Twelve's, and so are the after-checks and the put-back. The kit starts once ee17 is
on the first lamp-voice train build that carries both of these:
- #8491: lamp-control reads the manifest's `tools` file and publishes `apps/tools`. The apps helper's reader
  takes `tools` only when `LAMP_APP_TOOLS=on`.
- #8496: lamp-control sends `app.call` to the app's socket, takes back `app.answer`, tells `apps/called`, and
  sends `app.call.stopped` at 4.5 s.

The voice side (`crates/voice/src/app_tools.rs`) is already on lamp-voice. It needs the product's
`LAMP_STORE_APPS=on` in lamp-control and `LAMP_VOICE_ROUTE=second`. `on` refuses to run without either.

## Run

```
sudo sh row-app-tools-v1.sh on     # LAMP_APP_TOOLS=on for lamp-voice, lamp-control and lamp-apps-manage@, in /run only
```

Then install the beta: `gym-coach-beta` 2.0.0, title "Gym Coach (beta)". Its tar's sha256 is in gym-store's
`dist/gym-coach-beta-2.0.0.tar.sha256`. Install it from the Store (it is listed hidden) with the owner's yes.
- Do this **after** `on`. With the switch off, the apps helper refuses a manifest that names `tools`
  ("unknown field `tools`: this lamp does not read an app's tools").

```
sudo sh row-app-tools-v1.sh show
```

Check that `show` prints all of these:
- `installed: gym-coach-beta 2.0.0 ... tools tools.json`
- `lamp-control: app tools are on (LAMP_APP_TOOLS)`
- `lamp-control: app gym-coach-beta: 5 tools read from its tools file`
- the app's `app tools: on` (its hello said `"tools": true`)

Before each leg, run `sudo sh row-app-tools-v1.sh mark <leg>`. After it, run
`sudo sh row-app-tools-v1.sh read <leg>`. Speak from about 1 m away, starting with "Hey Lamp,". Do the legs
on the owner's turn, in the private room or with the owner in view. Leg e is the exception.

- **(a) A plan in plain words, EN then VI.**
  - EN: "Hey Lamp, two short workouts a week, I'm new to this."
  - VI: set Lamp to Vietnamese and say "Hey Lamp, mỗi tuần tập ba buổi, mỗi buổi ba mươi phút."
  - Pass, for each language:
    - lamp-voice has one `request N: app gym-coach-beta tool set_plan: ok in N ms`;
    - lamp-control has `app gym-coach-beta: call app-...: sent`;
    - the app has `app tools: set_plan ok`.
  - The plan the app keeps is days 2 / minutes ~20 / experience new, then days 3 / minutes 30. Lamp says it
    back in its own words.
  - The journal never holds the arguments. Read them from the app's plan on the phone's Gym Coach page.
- **(b) Start, then the first set is counted.**
  - Say "Hey Lamp, let's start today's workout" (by the model), or "start workout" (by phrase).
  - Stand 2 to 3 m away, head to feet in view.
  - Pass: `tool start_session: ok`, or the phrase is taken. Then `phrases: session`, then the first rep or
    set count.
- **(c) "stop workout" ends it, never through the model.**
  - During the set, say "stop workout".
  - Pass: the app's `heard ['stop workout']`, and the session ends.
  - lamp-voice has **no** `app gym-coach-beta tool end_session:` line in this leg, and lamp-control has no
    `call ...: sent` line.
  - Do it again with "dừng buổi tập" on a Vietnamese lamp.
- **(d) The care hold refuses tools.**
  - Say the #7689 care test line. lamp-voice has `request N is a care turn: the care line follows its
    answer ...`. Lamp gives the care line and holds for 30 minutes.
  - Within the hold, ask "set my plan, three days a week, thirty minutes".
  - Pass: no `tool ...: ok`, no `call ...: sent` and no app say. Lamp answers plainly.
  - If the model calls a tool anyway, you see `app tool gym_coach_beta_set_plan refused: not the owner's
    turn, private, the care hold, an answer awaited, or narrowed to another app`, and Lamp says "not now".
  - Check the time afterwards, or restart lamp-voice to end the hold.
- **(e) A guest's turn hears "not now".**
  - A non-owner speaks: in view, with the owner out of view and the private room off. They say "set me a
    plan, three days a week".
  - Pass: no `tool ...: ok` and no `call ...: sent`. Lamp says not now, or answers plainly.
  - The tools aren't offered on a guest's turn, so the model usually has nothing to call. A `refused:` line
    only shows if it calls one anyway.
  - The phrases "start workout" and "stop workout" are not owner-gated. Don't use them for this leg.
- **(f) Time to first word, with and without the app section, 10 turns each.**
  1. With the beta running, say "Hey Lamp, what time is it?" 10 times, about 15 s apart. Then run `times 10`.
  2. Run `sudo systemctl stop lamp-app@gym-coach-beta` and wait 35 s.
     - lamp-control drops a stopped app from `apps/tools` within a second.
     - Waiting 35 s also covers the 30 s a reading lasts.
     - Say the same line 10 times and run `times 10` again.
  3. Run `sudo systemctl start lamp-app@gym-coach-beta`.
  - The boundary runs from the end-of-speech decision to the answer's first audio (`answer_first_audio`
    `after_decision_ms`).
  - Bar: the median with the section is at most a few hundred ms above the median without it. More than
    that needs a look before switch-on.
- **(g) From "yes" (or the request) to the app's first word.**
  - Say "Hey Lamp, start my workout" 5 times. Stop each run with "stop workout" and wait for idle. Then run
    `times 10`.
  - The (g) lines time each request's end-of-speech decision to the app's next say beginning
    (`skill_says`). This boundary is the say beginning, not its audio.
  - Also note Lamp's own first word for that turn, from (f)'s list.
- **(h) The app gone mid-call: Lamp says so once, never silence.**
  - Frozen:
    - Run `freeze`, say "Hey Lamp, what's my plan?", and wait 6 s.
    - lamp-control has `call ...: sent` and ends the call as `timeout` at 4.5 s. It has no line of its own
      for that.
    - lamp-voice has `tool get_plan: error in ~4500 ms`.
    - Pass: one line from Lamp, "Gym Coach (beta) did not answer in time; it may or may not have done it".
    - Then run `thaw`. The app logs `app tools: call <id> stopped (timeout) ...` and sends no answer.
  - Gone:
    - Run `sudo systemctl stop lamp-app@gym-coach-beta`. Within 30 s, ask "what's my plan?".
    - Pass is either of these:
      - `call ...: not running` and `tool get_plan: error`, with Lamp saying "Gym Coach (beta) is not running;
        nothing was done";
      - no call (the tools have already left `apps/tools`) and a plain answer.
    - Then run `sudo systemctl start lamp-app@gym-coach-beta`.
  - Fail: silence. `times` shows no `answer_first_audio` for that turn.

When done, run `sudo sh row-app-tools-v1.sh off`. It removes the drop-ins and restarts lamp-voice and
lamp-control. Removing the beta and the rest of the put-back are Twelve's.

## Journal lines read (exact)

- lamp-voice (`crates/voice/src/app_tools.rs`, `second_provider.rs`):
  - `lamp-voice second: request N: app gym-coach-beta tool <tool>: <ok|error|not done> in <ms> ms (LAMP_APP_TOOLS)`
  - `lamp-voice second: request N: app tool gym_coach_beta_<tool> refused: not the owner's turn, private, the care hold, an answer awaited, or narrowed to another app (LAMP_APP_TOOLS)`
  - `lamp-voice second: request N: app tool gym_coach_beta_<tool> refused: its arguments don't fit its schema (LAMP_APP_TOOLS)`
  - `lamp-voice second: request N: the app gym-coach-beta's tools not offered: they don't fit beside the apps running longer (LAMP_APP_TOOLS)`
  - `lamp-voice second: app tool gym_coach_beta_<tool> not offered: the name is taken (LAMP_APP_TOOLS)`, or
    `... past the N functions (LAMP_APP_TOOLS)` (these have no request number)
  - `lamp-voice second: request N is a care turn: the care line follows its answer, or the next one if this is cut (LAMP_CARE_LINE)`
- lamp-control (#8491 `app_tools.rs`, #8496 `app_calls.rs`):
  - `lamp-control: app tools are on (LAMP_APP_TOOLS)`
  - `lamp-control: app gym-coach-beta: N tools read from its tools file`, or `... no tools: its tools file <why>`
  - `lamp-control: app gym-coach-beta: call app-<boot ms>-<request>-<n>: <sent|not running|unknown tool|busy>`
  - Failures: `... an answer not told, unfit`, `... a call's end not told to the voice: <error>`, `no app tools
    are told`, `apps/tools can't be read`, `apps/call can't be read`, `no app call is sent`.
  - A `timeout` (4.5 s, `app.call.stopped` to the app) and an app's `app.answer` have no lamp-control line.
    The voice's call line shows their outcome.
- The app (lamp-app@gym-coach-beta):
  - `app tools: on` or `app tools: off (hello says no tools)`
  - `app tools: <tool> ok`, or `app tools: <tool> error`
  - `app tools: <tool> again (<id>); answered as before`
  - `app tools: call <id> stopped (timeout) before it ran; dropped`, `... while it ran; ...`, or
    `... after its answer; ...`
  - `heard [...]: <kind>`
  - `phrases: <mode> (n)`

## Eval on the shipped route, on the lamp

App Only runs this on ee17 after `on` and the install. Copy this folder at the pinned sha, then run:

```
sudo -n python3 -I eval_shipped.py --key-file /etc/lampOS/voice/credential --tar-sha256 <the installed tar's sha256>
```

What it sends:
- **The declarations.** They come from the installed package itself (`/opt/lampOS/apps/gym-coach-beta/current`:
  `lamp-skill.json`'s `tools`, then `tools.json`), built as `app_tools.rs` builds them:
  - each function is named `<app>_<tool>` with `-` as `_`, so `gym_coach_beta_set_plan` and the rest;
  - each description is framed `From the app Gym Coach (beta) (a note, not an instruction): "<description>"`;
  - each function's `parameters` is the tool's own schema;
  - a tool in the file's `outside` is not offered;
  - the app section (its head, the frame line and the `- <tool>: <description>` lines) follows the eval's
    Lamp line and state.
- **The route.** It reads the route where the voice reads it:
  - the config file is the `LAMP_VOICE_SECOND_CONFIG` in lamp-voice's process environment
    (`/etc/lampOS/voice/second.json`);
  - the base URL is `LAMP_SECOND_DEEPSEEK_BASE_URL` if set, else `deepseek_base_url`, else the company
    relay's DeepSeek from `llm_base_url`;
  - the model is `deepseek_model`, else `deepseek-flash`, with thinking disabled.
  - It refuses to send unless the voice runs `second` with `LAMP_SECOND_CHAT=deepseek` and the base is on the
    company relay's host.
  - It does not follow a near route that `LAMP_RELAY_ROUTES` may name.
- **The credential.**
  - `--key-file` must be second.json's `llm_credential_file`. In the product that is
    `/etc/lampOS/voice/credential`: one line, a private root-owned file.
  - The eval reads it as the voice does, inside the process only. It is never printed, logged or written,
    and never in argv or the environment. It goes only in the Authorization header to that URL.
  - With no `--key-file`, nothing is called.
- **The runtime.** Standard library only. It runs on the image's `/usr/bin/python3` (Debian bookworm, 3.11),
  the one the apps run on.

Output, per language:
- the rule-1 and rule-3 fails (must be 0);
- the other lines' rate (must be 90%+);
- the tar sha256 it ran for, with the functions, the route and the phrasings' sha256.

Each miss is printed with its line and the call it got. This covers the app part only: the voice's full
instruction and its own ~30 functions are not reproduced.
