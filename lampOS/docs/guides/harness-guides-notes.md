# Notes for the two Harness guides (#5413)

For the people who keep [harness-getting-started.md](harness-getting-started.md) and [harness-developer-quick-start.md](harness-developer-quick-start.md). The guides are written for customers and carry no sources, so the sources, the media plan and the conditions for publishing are here. Paths are under `lampOS/` on `lamp-voice` (39fddfe02) unless named otherwise.

## When the getting-started guide is true

The guide describes the lamp once a build carrying PR #5379 is on stable. Today the product keeps both pieces off:

- Harness installed at onboarding: `crates/update/src/harness_install/product.rs:42` (`PRODUCT_DEFAULT = false`), `docs/switches.md:454`.
- Sign-in from the app by an 8-digit code: `deploy/product/lamp-tasks.service:36` has no `--sign-in-by-code` and no app socket; `docs/switches.md:406`. The voice's own switches (`LAMP_TASKS_SIGN_IN`, `LAMP_TASKS_SIGN_IN_APP`) are not set in `deploy/product/lamp-voice.service`.

Link the guide from the app only for a build whose lamps have both.

## Where the Autonomous app shows the guide

1. Right after the lamp's setup. Suggested words: "Your lamp can hand work to the AI helpers on your computer. Set it up in three steps." Buttons: "Show me" and "Not now".
2. On the Harness card, when the lamp has no Harness yet. Suggested words: "Connect your computer so the lamp can hand it work." The card may also link to the Harness phone app's page, harness.autonomous.ai/app (the App Store and Google Play). The computer app does not come from the App Store.

The lamp itself says nothing new about Harness on its own. That is out of scope for #5413 and is the lead's call.

## Pictures and video, one plan per step

Take them from a real run on a lamp with the build above. No names, emails or real codes in any capture: blur the code.

1. **Install:** the download page with the button circled; then Harness open on its first screen.
2. **Sign in:** the sign-in screen with its three ways in; then the signed-in home screen.
3. **Connect:** a 20-second screen recording of the app showing the code, the code typed under Machines, Approve a device, the Approve press, and the yes in the app. Then a clip of someone speaking to the lamp and the lamp answering, for the first real ask.

## Sources for the getting-started guide

| Claim | Source |
|---|---|
| The lamp repeats the task and nothing starts without a spoken yes | `docs/tasks.md:151`, `:74` |
| "Claude Code has it, I'll tell you when it's done" | `docs/tasks.md:1735-1745` |
| The lamp tells you at a good moment, unasked | `docs/tasks.md:1735-1745`; `LAMP_TASK_NEWS=skill`, `deploy/product/lamp-voice.service:338` |
| A question from the helper is told; you answer at the computer | `docs/tasks.md:1750`, `:1777` |
| Only the named helper gets the work | `deploy/product/lamp-voice.service:310` (`LAMP_TASKS_NAMED_ONLY=on`), `docs/switches.md:409` |
| The lamp can't check the work | `docs/tasks.md:30-34` (completion is "unverified") |
| Work goes to computers, not phones | `docs/tasks.md:1030`, `docs/switches.md:534`; a phone is a `viewer` session and only a `computer` or `team` session takes work: Harness `backend/src/lib/harnessSession.ts:95,158`, `adapterWs.ts:199`, `ssoAuth.ts:226-228` (checked by Mobile, openharness `1347c2481`). Harness can also run cloud machines for an account (`backend/src/routes/mobile.ts:56-63`) |
| Install page and platforms | harness.autonomous.ai/desktop loads and offers the downloads (checked live by Mobile); macOS 12+, Windows 10/11 (unsigned), Ubuntu 22.04+: `website/src/app/desktop/DesktopDownload.tsx:28-35`, `website/src/lib/desktopPlatform.ts:40-63` |
| Ways to sign in to Harness | `desktop/lib/.../login_screen.dart:332-361`, `cli/src/cli.ts:144-152` |
| Machines, Approve a device, then an 8-digit code | `desktop/lib/widgets/in_window_menu_bar.dart:854`, `machine_picker_extras.dart:364`, `approve_device_code.dart:112-136` |
| Continue, then "Sign in <lamp>?"; Approve, or a 2-second hold on another network | `approve_device_code.dart:176,198,236-251` |
| The code can be typed at harness.autonomous.ai when signed in there | `website/src/lib/webApp.ts:2`, `swarm_screen.dart:3810` |
| The code can be typed in the Harness phone app | `mobile/lib/phone/settings_page.dart:504` |
| The lamp gets the code, then asks "Sign in as <email>?" and takes a yes or no; the app answers for an app-started sign-in | `cli/src/cli.ts:148-152,702-739`; `docs/tasks.md:1209`, `:1220-1228`; `docs/cloud.md:123` |
| Someone who overhears the code gains nothing | `docs/tasks.md:1209-1211` ("No session exists before the answer") |
| The code runs out; start again | `docs/tasks.md:1193-1195` (a code lives 10 minutes), `:1224-1228` (an unanswered question ends after two minutes). Times left out of the page on purpose |
| After a yes the lamp looks at once for the computers; the targets file is read again each minute | `docs/tasks.md:1237-1238`, `:986-988` |
| The lamp says plainly when Harness or a computer is missing | `docs/tasks.md:1015-1020` (`handovers_off`: `no_harness`, `no_computer`, `not_signed_in`) |
| The Harness window can be closed; the service keeps running; after a restart, Windows starts it again, a Mac or Linux needs Harness opened once (or `harness service install`) | Harness `cli/src/cli.ts:1721-1729`, `desktop/macos/Runner/AppDelegate.swift:55`, `desktop/lib/core/windows_status_tray.dart:11`, `cli/scripts/install.ps1:450`, `cli/src/harnessd/platform.ts:132-141` (checked by Mobile) |
| The Harness version the build installs, held there | 0.5.39: `crates/update/src/harness_install/product.rs:51`; `LAMP_HARNESS_HOLD_UPDATES` (`docs/switches.md:455`, off today). Not on the page |
| Sign-in is Harness's own; the lamp only passes the person's answer on | `docs/tasks.md:9-15` (Tasks implements no pairing, login or credential) |

### Left out because it is not proven

- Naming a computer ("on my office computer"): only a first computer is called `office` and other names need `LAMP_TASKS_MACHINE_NAMES`, which is off in the product (`docs/switches.md:407`, `:534`).
- Anything about a phone receiving work from the lamp.
- A dated or timed promise ("in a minute") beyond what the lamp's code reads: the page says "in a minute or two", from the once-a-minute read of the targets file.

## Sources for the developer quick start

| Claim | Source |
|---|---|
| Python 3.10 or later; SDK 0.2.0 | `sdk/python/pyproject.toml:6-10` |
| Not on PyPI or npm; install from a checkout | `sdk/python/README.md:21-28`, `sdk/js/README.md:22-28` (`sdk/README.md` says `pip install lampos`, which the two READMEs contradict; the page follows the READMEs) |
| `lamp fake`, its `LAMPOS_HOME`, every grant | `sdk/python/README.md:30-57` |
| `lamp pair CODE --pin PIN`; pin checked first; kept in `~/.config/lampos/` | `sdk/python/README.md:59-72` |
| The code comes with the pin and a 5-minute expiry | `docs/control.md:297-300`, `docs/develop.md:259-261` |
| The lamp says the first 8 characters of the pin aloud | `docs/cloud.md`, "The developer door" (`control.code`, the spoken pin check) |
| The app screen is not out yet; the keeper makes the code with `lamp-control code --grants body,light,speak` | `docs/develop.md:259-262` ("that app screen is a later piece"), `docs/control.md:236`; the lamp's side is merged (commit ae813eee0, `docs/cloud.md:157`, `:2145`) |
| lamp-control runs only with `LAMP_CONTROL=on`; no product unit passes it | `docs/control.md`, "Running the door"; no lamp-control unit in `deploy/product/`; PR #4526 is open |
| The owner can revoke at any time | `docs/cloud.md` (`control.revoke`), `docs/develop.md:276` |
| The person wins | `docs/develop.md`, "What takes control back" |
| What a program never gets | `crates/control/src/protocol.rs:118-135` (`Grant::shown`), `docs/develop.md` ("Never") |
| A motion asked while one plays is refused `busy` with `retry_ms` | `sdk/python/README.md`, "A motion asked while another still plays" |
| Closing the connection gives the light back | `sdk/python/README.md`, "Closing gives your light back at once" |
| Harness and the SDK are separate doors | `docs/tasks.md:9-15`; Mobile found nothing in the SDK or the example that calls a Harness API |

### How the example was run

On 5 Nov 2026, from a checkout of `origin/lamp-voice` at 39fddfe02: macOS, Python 3.13.7, a fresh virtual environment, `pip install ./sdk/python`, a temporary `LAMPOS_HOME`, `lamp fake` in one terminal and `python first_light.py` in another. Exit 0, output as on the page. The reviewer re-ran it on Python 3.12.3 with the same result. It ran against the fake lamp, not a real lamp. Not run: the JavaScript SDK, a real pairing, the app.

### Left out because it is not proven

- Any statement that the app's *Add a developer computer* screen is in a build customers get.
- Real-lamp pairing end to end (needs lamp-control on, PR #4526 not on stable).
