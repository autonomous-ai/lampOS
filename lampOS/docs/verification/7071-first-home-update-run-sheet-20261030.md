# A 7071 lamp's first home update, on a real lamp: run sheet — NOT RUN

**Not needed as written (the lead, #1711, 30 Oct 16:42):** the ten customer
lamps did not ship on 7071. The hardware team flashed 7071, then set each one up
through the app on Wi-Fi, so each took that day's stable over the air before it
was reset and packed. That was 7071 → stable on real lamps. Their first home
update is from the build each shipped on to stable. On which shipped builds
that differs from lamp-52e6's own move to 8899, see Second on #1711 (30 Oct
16:5x): builds 8237 to 8498 (updater 8, from before the voice's own guard
release `1c88f2dbd`) run the switch with their own, older order. The sheet below
still reads right for a lamp that is on 7071, if one is ever run.

For [#3689](https://github.com/autonomous-ai/lampOS/issues/3689) and the first
builder's 15:5x on #1711 (30 Oct), as it was then thought: the ten customer
lamps left on the 7071 image (`aa9ec9e27a`, updater level 5), and their first
update at home had never run on a real lamp. Tests showed the climb and the
round on emulated ARM64 (#3689, 29 Oct 02:22), and that 8899's signed envelope opens on 7071's own rules
(#3689, 30 Oct 14:55). The parts starting, the voice listening afterwards and
the health watch need a lamp. Second has no lamp. This sheet records no result.

Which lamp, and who runs it, is the lead's and the hardware team's call. Keep
the lamp's own name, network and any person out of what is posted back: post
role labels and the lines below.

## What the lamp should do

A 7071 lamp's own `lamp-update.service` already follows `lampos-stable`, with
`--apply on` and `LAMP_UPDATE_SELF=on`. No drop-in is needed. Stable was 8899 on
30 Oct 15:21; it may have moved by the run (8925, train 409, carries #4517), so
the first thing the run notes is the build stable names. Below, 8899 stands for it.

1. **Setup through the app**, as a customer does. `lamp-update serve` runs before
   and after setup (it is outside `lampos.target`), a round every 900 s.
2. **The climb (7071's updater).** 8899's manifest needs updater 8
   (`min_updater` 8). 7071 is updater 5, so its round installs only the
   `lamp-update` 8899 carries (5,252,824 bytes) beside its own and switches to
   it. The new updater starts with the same line and confirms itself, about
   20 s in (Tests' rig: 16 to 19 s).
3. **The round (8899's updater).** It fetches the build beside 7071's, about
   236 MB, then:
   - **holds while the setup walk is under way** (#4455): before the fetch and at
     the switch's quiet wait, for at most 8 rounds (`walk::MOST_HELD`), so up to
     about 2 h if the walk is left unfinished;
   - waits for a **quiet moment**: the lamp resting, nobody heard and nothing
     said for 45 s (`quiet::QUIET`);
   - stops the voice first and starts it last (#4431), switches every part, and
     watches each for 60 s. A part that restarts, changes process or fails in
     that time puts the whole build back.

## The run

1. Flash one lamp with the 7071 image (sha256 `15449d4f…`, as #3530's 4c4 used).
   Note the time, and the build stable names (the release
   `lampos-stable`'s `manifest.json`, `sequence`).
2. Set it up through the app. **Finish the walk** it starts, so the walk hold
   doesn't add up to 2 h.
3. Use it a little, as a person would: say its name, let it see a face, tell it
   one thing to remember, mute and unmute once.
4. Leave it quiet: nobody talking near it. Expect the switch within about
   15 to 30 min of setup, depending on where the 900 s round falls.
5. Then say "Lamp, what time is it?" and "Lamp, what do you remember about me?".

## What to send back

```sh
journalctl -u lamp-update --since -2h --no-pager \
  | grep -E "needs updater|switches from its build|confirmed: lamp-update|switching to build|switched to build|putting build|walk is under way|no quiet moment|unreachable|failed|refused|nothing is switched"
cat /run/lampos-bus/voice/mute/latest
journalctl -u lamp-voice -b --no-pager | grep -iE "guard|muted" | tail -5
systemctl show -p NRestarts lamp-voice.service lamp-controls.service
```

and whether each of the two questions was answered.

## Reading it

| Seen | Means |
| --- | --- |
| `build 8899 needs updater 8; lamp-update switches from its build … to …, which the build carries, …`, then `confirmed: lamp-update runs its new build` | the climb worked (7071's `Outcome::Climbs`, then the new updater's confirm) |
| `build 8899 needs updater 8 or newer, and this is updater 5; …` | **fail, no climb**: 7071's refusal (`TooOld::why`), written only when it did not climb. Post the whole line and `systemctl show -p Environment lamp-update` |
| `switching to build 8899`, then `switched to build 8899; every part is well` | **pass**, if the voice then answers both questions and is not muted |
| `putting build 8899 back: …` | the watch put the parts back. The lamp keeps 7071's parts and the climbed updater. It records 8899 as refused and never tries it again (`round.rs:917` at `1e42b1032d`); it takes the next promoted build. Post the line's `why` and the voice's last 40 journal lines before it. A speaker that was busy at the new voice's start points to the PulseAudio race that #4517 guards (in 8925 and later, not in 8899) |
| `the setup walk is under way` | held by the walk, as designed. Finish the walk and wait for the next round |
| `no quiet moment in … s` | someone was heard, or the lamp wasn't resting. Leave it quiet; the next round tries again |
| `unreachable` | the channel couldn't be reached. Nothing changed; the next round tries again |
| switched, but muted, or a question not answered | **fail**: the case #4431 was built for. Post the voice's last 40 journal lines before the switch |

## Afterwards

Nothing to put back: the lamp stays on the stable build, the one every customer
lamp is meant to reach. If it was put back, it stays on 7071's parts until the next
promote, as a customer's lamp would.
