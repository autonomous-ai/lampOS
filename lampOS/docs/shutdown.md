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
| Refused | While `lamp-update` holds its lock (an update installs), or the lock cannot be checked, nothing is done: `{"refused": ...}`, exit 1. Otherwise the lock is held to the end, so no update starts under the shutdown. | taken without waiting |
| Say | The lamp says `shutdown::GOING_TO_SLEEP`, "Going to sleep now. Unplug me when my light turns green.", through the voice's say service (`LAMP_SAY_SOCKET`, `/run/lampos-say/say.sock`; speaking-first.md, "the say service") as `text` from the skill `shutdown` at priority 9, and waits for its `{"outcome":"spoken"}`, played whole. Whether it may be said now (muted, paused, busy) is the say service's own judgement; nothing is added here. Not there (`LAMP_SAY` off, the voice down), refused, dropped, expired or not spoken in time: said as "not said (...)", and the shutdown goes on. A dry run says it too, so that it can be heard. | 10 s (`SAY_WITHIN`) |
| Pause | `lamp-keeper pause`: each part that honours `lamp/pause` takes no new work, and the body goes to rest, folding if it stands. A product lamp has no keeper (lamp-52e6 on 7071): that is said as "no keeper on this lamp: the body rests as it stops", and no resume is asked; any other failed pause is said too. Either way the body's own stop folds it. | one command |
| Rest | The body's `body/joint_states`, fresh, every 100 ms: every joint still within 0.01 rad (about 6 counts) for 2 s (`STILL_FOR`, HAL-4.12's two seconds). | 15 s (`REST_WITHIN`): a standing lamp folds over its 6 s stand time and is then still for 2 s. After it the shutdown goes on and says so. |
| Stop | `systemctl stop lampos.target` with every part named and each read back `inactive` or `failed`, and the reset's starters and writers stopped with it (`reset::stop_and_wait`). The body folds on its stop and keeps its torque. Stopped, the pause is cleared, so a lamp started again without a power-up is not left paused. The voice is stopped on its own first (`VOICE_FIRST`): the target stops lamp-controls before it, and a voice that loses the privacy switch while it stops counts it a fault, so its restart guard muted lamp-52e6 at each next power-on (27 Oct). | each part's own `TimeoutStopSec` (the body's 25 s) |
| Let go | 0.4 s after the stop (`LET_GO_AFTER`, HAL-4.12), `lamp-body-measure let-go` as the body's user (`setpriv --reuid=lampos --regid=lampos --groups=dialout`; the servo port's owner refuses root): it takes the port and its lock as the body does, reads each joint, and only for a lamp that is down, the head within 20 counts of `lamp.env`'s `--home` and the arm within 20 of its `--folded-arm` (a seated lamp has none, and its arm is not judged), writes torque off to all five and reads each joint's torque back. One JSON line, each joint with where it was and its torque. A lamp found up keeps its torque: let go, it would fall. | the port's own read deadlines, well under a second |
| Light | The setup light's ring writer (`lamp_ring::linux::open_setup`, which the voice has let go with its stop) shows the whole ring in soft green, steady (`SetupLook::Unplug`), and is left with that frame on. | one frame |
| Sync | `sync`. | |
| Power off | `systemctl poweroff`; one that fails is forced once synced, as the reset forces its reboot. With `--dry-run`: not run, and `dry run: would run /usr/bin/systemctl poweroff` said instead. | |

Nothing that fails stops it short of the power: a step that fails is said, and
the next runs. A lamp that asked to be turned off and is left half stopped
helps nobody, and the system's own poweroff stops what is left in its order.
This is the reset's policy for a stop that fails (it reboots all the same). A
body that never shows rest is stopped after 15 s; its stop folds it, and the
let-go writes nothing to a lamp it finds up.

`--dry-run` does everything but the poweroff: the lamp says its line, is paused, stopped, let
go and lit, and stays so. `lamp-keeper start lamp` brings it back, or `systemctl restart lampos.target` on a lamp without the keeper. `systemctl stop lampos.target` warns that `lampos-setup-done.path` can start the target again: before a power-off that does not matter, and after a dry run the target may read active with its parts stopped until it is restarted.

## The switch

`LAMP_SHUTDOWN` (`on` exactly; off unless set; `switches.md`) is for the ways
a person asks, which the later changes add. The runner itself is not behind it:
it is only ever started by one of those, or by hand.

## What later changes add

1. The app's `system.shutdown` through `lamp-cloud`, on a root socket as the
   app's restart is asked (`lamp-keeper restart-asked`, keeper.md).
2. The button held, acting on release by hold time (HAL-5.7), with warning
   lights while it is held.
3. Voice: "Lamp, turn off", only with a spoken yes, never on a guess. The
   runner itself says it is going to sleep first (the Say step).

Each starts `lamp-setup shutdown` and is behind `LAMP_SHUTDOWN`.

## Not yet seen on a lamp

Host tests only (`lamp-setup`'s `shutdown`, `lamp-body`'s let-go against
simulated servos). On a lamp still to see: the rest proof on a standing lamp;
the let-go's per-joint read-back; whether the ring keeps its frame after the
poweroff (it does only while the LEDs keep power); whether the servos keep
power after the board's poweroff; and a clean boot with no filesystem repair.

Related: preparing to unplug without turning off (unplug.md), the reset
(setup.md, "The reset"), the pause (keeper.md).
