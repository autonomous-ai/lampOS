# Turning the lamp off safely (#3697)

Until now the only way to turn a lamp off was to pull the plug, which can
corrupt the SD card. V1 had a shutdown from the app, from a held button and by
voice; `v1-verdicts.md` says how to rebuild it: the buttons are needed
(GO-2.20), a destructive action happens on release by hold time (HAL-5.7), the
servos are parked before the power goes (HAL-5.18), and the park is V1's order,
"move to rest over 2 s, wait 0.4 s, then torque off, reporting each joint"
(HAL-4.12).

This page is the lamp's side of it. The first change builds the runner and the
body's let-go; the ways a person asks come after it (below).

## The runner

`lamp-setup` owns it, beside the reset: the reset already stops the lamp with
every part named and read back stopped (`reset::stop_and_wait`), hands itself to
a unit of its own so that stopping the lamp cannot stop it, holds the setup
light's ring writer, and builds against `lamp-update`'s lock. The keeper owns
`lamp/pause`, which the runner asks of it (`lamp-keeper pause`), and the body
owns the servo port, which the runner asks of it (`lamp-body-measure let-go`).

```sh
lamp-setup shutdown [--dry-run]       # hands shutdown-now to lamp-setup-shutdown, a unit of its own
lamp-setup shutdown-now [--dry-run]   # the shutdown itself, as root
```

`shutdown` runs `systemd-run --no-block --collect --unit=lamp-setup-shutdown
--property=Type=oneshot lamp-setup shutdown-now`, outside `lampos.target`, as the
reset's `lamp-setup-wipe` is started.

`shutdown-now`, as root, in order, one journal line a step (`lamp-setup:
shutdown: ...`) and one JSON line of the steps at the end:

| Step | What | Bound |
|---|---|---|
| Refused | While `lamp-update` holds its lock (an update installs), or the lock cannot be checked, nothing is done: `{"refused": ...}`, exit 1. Otherwise the lock is held to the end, so no update starts under the shutdown. | waited for up to 5 s (`LOCK_WAIT`), as the change helper lets it go; the helper's own take doesn't wait |
| Say | The lamp says `shutdown::GOING_TO_SLEEP`, "Going to sleep now. Unplug me when my light turns green.", through the voice's say service (`LAMP_SAY_SOCKET`, `/run/lampos-say/say.sock`; speaking-first.md, "the say service"), from the skill `shutdown` at priority 9. First as `text` with `"cache": true` (speaking-first.md, "A cached line"): played from the voice's speech cache, the line rendered once in the lamp's own speech voice and kept on the lamp, keyed by the SHA-256 of voice, model, language and these words (the text is `lamp_msgs::GOING_TO_SLEEP`, one source for both), and rendered by the voice at its start, so it needs no model and no network at the shutdown: on lamp-52e6 a `text` say answered `dropped: no_audio_answer` after 3 s, a voice restarted 2 min before, and the lamp powered off silently. The skill `shutdown` is root's alone, and its say is the lamp's last words (speaking-first.md, "The shutdown's last words"): taken before any other, held by no quiet, rarity, listening or speaking gate, the lamp's own speech cut for it, so a shutdown asked right after a turn or while the lamp speaks is still said; only the keeper's pause and the privacy switch keep it silent (`the lamp is muted`). Not played (no cache: `the speech cache is off`; never rendered and offline), the same words follow as `text` for the voice's model, in the time left. Either way it waits for `{"outcome":"spoken"}`, played whole. Neither: said as "not said (the cached line: ...; the words: ...)", and the shutdown goes on. A dry run says it too, so that it can be heard. | 10 s (`SAY_WITHIN`), the cached line and the words together |
| Pause | `lamp-keeper pause`: each part that honours `lamp/pause` takes no new work, and the body goes to rest, folding if it stands. A product lamp has no keeper (lamp-52e6 on 7071): that is said as "no keeper on this lamp: the body rests as it stops", and no resume is asked; any other failed pause is said too. Either way the body's own stop folds it. | one command |
| Rest | The body's `body/joint_states`, fresh, every 100 ms: every joint still within 0.01 rad (about 6 counts) for 2 s (`STILL_FOR`, HAL-4.12's two seconds). | 15 s (`REST_WITHIN`): a standing lamp folds over its 6 s stand time and is then still for 2 s. After it the shutdown goes on and says so. |
| Stop | `systemctl stop lampos.target` with every part named and each read back `inactive` or `failed`, and the reset's starters and writers stopped with it (`reset::stop_and_wait`). The body folds on its stop and keeps its torque. Stopped, the pause is cleared, so a lamp started again without a power-up is not left paused. The voice is stopped on its own first, with `lampos-setup-done.path`, which would otherwise start the target, and with it the voice, again while it stops (lamp-52e6, 28 Oct: the voice back for 6 s between the two stops) (`VOICE_FIRST`; a dry run starts the path again with the rest): a voice that loses the privacy switch while it stops counts it a fault, so its restart guard muted lamp-52e6 at each next power-on (27 Oct). Every other stop of the target is voice-first by the units themselves: the product's `lamp-voice.service` is `After=lamp-controls.service` and lamp-controls is no longer after the voice, so `systemctl stop lampos.target`, the reset's `reset::stop_and_wait` and the keeper's `stop lamp` and `restart` (the app's restart) stop the voice before Controls. `VOICE_FIRST` stays for a lamp whose units are older. | each part's own `TimeoutStopSec` (the body's 25 s) |
| Let go | 0.4 s after the stop (`LET_GO_AFTER`, HAL-4.12), `lamp-body-measure let-go` as the body's user (`setpriv --reuid=lampos --regid=lampos --groups=dialout`; the servo port's owner refuses root): it takes the port and its lock as the body does, reads each joint, and only for a lamp that is down, the head within 20 counts of `lamp.env`'s `--home` and the arm within 20 of its `--folded-arm` (a seated lamp has none, and its arm is not judged), writes torque off to all five and reads each joint's torque back. One JSON line, each joint with where it was and its torque. A lamp found up keeps its torque: let go, it would fall. | the port's own read deadlines, well under a second |
| Light | Only after a let-go whose report says so, `"let_go": {"let_go": true, …}` under the port's report: the setup light's ring writer (`lamp_ring::linux::open_setup`, which the voice has let go with its stop) shows the whole ring in soft green, steady (`SetupLook::Unplug`), and is left with that frame on. A lamp not let go (found up, `"let_go": false`, or a let-go that failed) is never lit green, since green is the "Unplug me" the lamp just said and an arm held up by its torque would fall: `not_lit` "not let go: keep it plugged in" (`NOT_LET_GO`; `a_lamp_not_let_go_never_turns_the_ring_green`). | one frame |
| Sync | `sync`. | |
| Power off | `systemctl poweroff`; one that fails is forced once synced, as the reset forces its reboot. With `--dry-run`: not run, and `dry run: would run /usr/bin/systemctl poweroff` said instead. | |

Nothing that fails stops it short of the power: a step that fails is said, and
the next runs. A lamp that asked to be turned off and is left half stopped
helps nobody, and the system's own poweroff stops what is left in its order.
This is the reset's policy for a stop that fails (it reboots all the same). A
body that never shows rest is stopped after 15 s; its stop folds it, and the
let-go writes nothing to a lamp it finds up.

`--dry-run` does everything but the poweroff: the lamp says its line, is paused, stopped, let
go and lit, and then starts again every `lamp*` unit that was active before its stop and is not after (`systemctl list-units --state=active lamp*` read before and after, then one `systemctl start`; an accepted connection's `@` instance is left to its socket), said as `dry run: started again ...`. The stop ends more than `lampos.target`'s parts: the reset's writers and starters (`reset::WRITERS`, `reset::STARTERS`), `lamp-update.service` among them (WantedBy `multi-user.target`), which `systemctl restart lampos.target` never brought back after a dry run on a real lamp. A real power-off reads and starts nothing. `systemctl stop lampos.target` warns that `lampos-setup-done.path` can start the target again: before a power-off that does not matter.

## The switch

`LAMP_SHUTDOWN` (`on` exactly; off unless set; `switches.md`) is for the ways
a person asks: `lamp-cloud` reads it for the app (below); the others come with
later changes. The runner itself is not behind it: it is only ever started by
one of those, or by hand.

## The ways a person asks

1. **The app** (Mobile's off button, #3698): `system.shutdown` to
   `lamp-cloud`, which sends `{"shutdown":"lamp"}` to setup's change helper
   (`lamp-setup change` on `/run/lampos-setup-change/change.sock`, root's,
   on every lamp; setup.md, "Changed after setup"). The helper takes
   lamp-update's lock without waiting: held by an update (or not checkable),
   it refuses with why ("an update is being installed: try again once it is
   done"). Otherwise it lets the lock go, answers
   `{"done":{"shutting_down":true}}`, so the app hears `success` before the
   lamp goes down, and starts `lamp-setup shutdown` (the runner above, which
   takes the lock again itself, waiting up to 5 s for the helper to let it
   go, `shutdown::LOCK_WAIT`, and refuses, logged, if an update holds it past
   that). `lamp-cloud` keeps the yes for 60 s: an app that asks again within
   it is answered the same and the helper is asked once; after it, a lamp
   still on is asked again (cloud.md, "The lamp turned off from the app").
   No keeper is needed: a product lamp has none.

Still to come:

2. The button held, acting on release by hold time (HAL-5.7), with warning
   lights while it is held.
3. Voice: "Lamp, turn off", only with a spoken yes, never on a guess. The
   runner itself says it is going to sleep first (the Say step).

Each starts `lamp-setup shutdown` and is behind `LAMP_SHUTDOWN`.

## Not yet seen on a lamp

Host tests only (`lamp-setup`'s `shutdown` and its change helper's answer
under a held update lock, `lamp-body`'s let-go against simulated servos,
`lamp-cloud`'s `system.shutdown` against a stand-in helper answering with the
helper's own code). On a lamp still to see: the app's `success` before the
lamp goes down, and the helper's `systemd-run` from its sandboxed unit; the
rest proof on a standing lamp;
the let-go's per-joint read-back; whether the ring keeps its frame after the
poweroff (it does only while the LEDs keep power); whether the servos keep
power after the board's poweroff; and a clean boot with no filesystem repair.

Related: preparing to unplug without turning off (unplug.md), the reset
(setup.md, "The reset"), the pause (keeper.md).
