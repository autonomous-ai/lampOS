# Controls: a hand can ask the lamp to stop

Task 33 adds an opt-in touch path, described below. The later sections retain
the installed Task 7 path and its evidence. Without `--touch` or the separately
selected Task 755 privacy source, that path's schema, topics, recognition and
output remain unchanged. Physical privacy can be selected independently in
either mode.

## Touch and slides, Task 33

### What exists and what we take

The MPR121's electrical rules come from NXP's
[Rev. 4 datasheet](https://www.nxp.com/docs/en/data-sheet/MPR121.pdf),
[startup example](https://www.nxp.com/docs/en/application-note/AN3944.pdf),
[auto-configuration](https://www.nxp.com/docs/en/application-note/AN3889.pdf),
[debounce](https://www.nxp.com/docs/en/application-note/AN3892.pdf) and
[filter timing](https://www.nxp.com/docs/en/application-note/AN3890.pdf).
V1's reset/Stop/configure/Run order, status-error rejection, startup-held
suppression, pad/chord filtering and bounded stale-work rules are retained.
Its direct callbacks, `Pet`, toggle and shutdown meanings are replaced.
`i2cdev` 0.6.2 supplies Linux combined transfers; a small injected register
interface permits fault tests. `mpr121-hal` 0.5.0 was inspected but not used:
its reset discards a write error and its touch accessor masks fault bits.
See the source comparison in [reuse decisions](reuse-decisions.md#touch-strip-and-head-task-33-2026-10-14).

`serve --touch --pins FILE` selects the new, independent GPIO/strip path.
It **configures the selected MPR121 even in shadow mode**; omit `--touch` to
keep the installed reader. `--enable` separately marks published requests as
enabled, overriding a configuration file's `enabled` field. Neither switch
wires Voice, Expression, lights or motors to the requests. This task installs
no unit and changes no per-lamp deployment file.

### What a hand requests

`TouchEvent` on `controls/touch` and `TouchState` on `controls/touch_state`
are new declarations, separate from the existing controls types. They carry
the original clock, Controls and reader sessions, source, contact identity,
contact-begin/acquisition/release times, active mask/names and strip geometry.
The speaking/quiet/asleep alternatives are conditional requests, not reports
of the lamp's current state or of an action applied.

| Observation | While awake | Contact began asleep |
|---|---|---|
| Qualified head touch (20 ms) | Stop if speaking, Notice (nothing asked) if quiet | No request |
| Head tap: qualified, let go before 0.5 s from its first acquisition | Attend | Wake |
| Head hand kept 1 s | Stroke | No request |
| Head hand kept 3 s | Sleep | No request |
| Qualified three-pad base touch | Stop if speaking, Attend if quiet | Wake |
| Stationary qualified base held two seconds | Sleep | Wake |
| Moving base contact | Slide; an eligible slow movement also emits Stroke | No further request |
| Optional primary button, short release | Stop | Stop |
| Optional primary button, release after two seconds | One-way SoftwareMute | One-way SoftwareMute |
| Optional reset button, release after five seconds | FactoryReset only | FactoryReset only |

Sleep and Wake are separate, idempotent requested transitions. They never
toggle microphone mute. First's 18:23 decision in
[#711](https://github.com/autonomous-ai/lampOS/issues/711) makes Voice close
microphones for Sleep and reopen on Wake, subject to physical privacy and
its existing independent mute gate. Controls grants no capture permission.
The lifecycle owner must bind awake/asleep **at `began_us` for the entire
contact**, using history that covers that instant. Unknown history denies
Sleep. A waking hand still held at two seconds must remain a Wake request,
not send the lamp back to sleep. Speaking/quiet is checked at application.
Deduplicate accepted events durably across owner restarts before applying a
destructive request such as reset; this crate has no factory-reset executor.

`TouchReceiver` checks source and Controls sessions, original acquisition age
below 100 ms, current source freshness below 100 ms, state freshness below
300 ms, sequence ordering and fault/restart fences. The 500 ms transport
declaration is not a renewed action lifetime. Repeating retained state cannot
refresh old evidence. It suppresses lifecycle/reset records for contacts
already underway when a receiver attaches or recovers. An owner must also
validate the bus declaration, envelope and trusted publisher before passing
payloads to it; state publisher boot equals `TouchState.session_id`, while the
event topic has its own publisher boot. Do not compare that event-header boot
to the state publisher's session.

For an explicitly configured primary button, touch mode also publishes the
existing Stop/SoftwareMute protocol so installed Voice keeps working. A new
owner must choose one authoritative primary protocol; it must not independently
apply both representations of one release. Base and reset never enter that
legacy engine, and a malformed complete GPIO observation cannot retain a
primary action from just the valid-looking portion. The head pads enter it
only with `"legacy_head": true` in the pins, for a lamp whose voice and
Expression do not read `controls/touch` yet (the product's, the reviewer on
#1167): a pat is then still the old engine's `Pet` on `controls/gesture`,
which wakes a lamp asleep at night, and `head_active` on `controls/state`.
A `Pet` never touches the mute, nothing reads `head_active`, and the voice
counts pets only for its trace. `legacy_head` goes only with neither
`LAMP_HEAD_TOUCH=on` nor Expression's `--head-touch`, which read the head
from `controls/touch`. And `--head-touch` goes only with the voice's
`LAMP_HEAD_TOUCH=on` and `--controls-mute`: without the voice's sleep mute,
a hand held 3 s puts the face to sleep with the microphones open (the
reviewer on #1176). Install's test
`head_touch_comes_whole_and_never_beside_the_old_engines_head` holds the
product's units and pins to both. With no head pads, `legacy_head` is
ignored, and the journal says so (`legacy_head ignored: the pins name no
head pads`): the old engine would have nothing to read, and its refused
reads took the touch path's GPIO down with it. It is ignored rather than
refused, so a pins file install kept does not stop the button and the
reset (#1209).

The reset button arms only after it has read released for one second
running (`RESET_ARM_US`), and the touch pins require it pulled up: a line
that rests pressed and lifts briefly is never a reset, so two short lifts
5 s apart cannot wipe the person's data (the reviewer on #1167). A reset
line that reads pressed through all of its first second never arms until
Controls starts again, and the journal says so once (`hand: reset read
pressed through its first second: …`): a line that rests pressed is not a
reset button, even if it lifts for a second later (the reviewer on #1176).
One released read in that second is enough, so a line still charging
through its pull-up, or a hand leaving the button as Controls starts, does
not turn the reset off (#1209). The journal's
`hand: reset held … let go` line ends `enabled:`, which is Controls'
`--enable`, not whether `lamp-setup watch-reset` acted.

### Geometry, qualification and missing evidence

The pins file lists **3–12 working electrodes**, each once, in physical order.
First's [20:05 hardware answer](https://github.com/autonomous-ai/lampOS/issues/711#issuecomment-6169047710)
confirms E4/E8 are faulty on lamp-4ace: its set is E0–E3, E5–E7, E9–E11.
That exclusion belongs to this unit, not every lamp. The hardware specification
orders E11→E0 from right to left. A complete twelve-pad order may be any
permutation; a sparse order must be strictly ascending or descending, since
it otherwise does not locate the missing physical slots unambiguously.
Position is a filtered centroid over the full twelve-slot span, 0..1;
sparse ascending E5 stays at 5/11, rather than becoming the fifth of ten pads.
Reversing the order reverses that coordinate. Excluded pads are skipped only
when checking whether a footprint is connected; they do not shrink distance.
Distance is accumulated normalized travel and speed is normalized travel per
second. None is millimetres, physical velocity or an extrapolated finger.
Direction names the configured endpoint electrode, not a guessed left/right.

One or two pads can provide position. Touch and Sleep need a three-pad chord
qualified continuously for 10 ms, after the 5 ms per-pad filter and 10 ms
contact qualification. Release needs 30 ms. In a sparse working set, an inactive
working electrode between active ones makes the footprint ambiguous and
consumes the contact. The original full-set mode retains its one-hole tolerance
(a gap spanning more than two ranks is ambiguous). Raw selected samples
determine actual release; filtered geometry must not invent a release during
electrode handoff. Movement consumes Sleep for that contact, even after a
pause or reversal. Releasing a slide never adds a tap/Touch. Direction uses
half-rank hysteresis, and speed covers at most 200 ms/128 acquisitions.
Stroke requires fresh movement, at least 300 ms since movement began, at
least one rank travelled, and mean travel at most two ranks/second over that
moving span. This authored heuristic needs hands-on qualification; a pause
followed by travel can lower its estimate.

Known head inputs settle for 500 ms and qualify a touch after 20 ms of
unbroken active acquisitions. **The head carries every touch action**
(the owner, 2026-10-15; [creature.md](creature.md#touch), #711), so a hand
that stays must stay one hand: V1 FastMode TTP223 pads can pulse for about
50–100 ms while a hand remains there, so released acquisitions shorter than
the gap do not end a contact. Only an unbroken qualifying run starts the
requests, so short blips joined by gaps never do. Three settings in the touch
pins file, each optional, are to be sized from a recording of lamp-4ace's
pads (First, #711):

| Setting | Default | Bounds | What it does |
|---|---|---|---|
| `head_qualify_ms` | 20 | 20..200 | An unbroken active run this long qualifies a touch: the Stop |
| `head_tap_min_ms` | 120 | qualify..499 | A tap is let go no sooner; just above the lone 117 ms edge lamp-4ace's journal showed at 19:25 with nobody known at the lamp |
| `head_gap_ms` | 150 | 0..500 | Released acquisitions shorter than this do not end a hand |

A lone short edge can therefore stop speech at 20 ms, as any touch does,
but never asks the lamp to listen or wake. The contact's time,
through such gaps, is `touch_state.held_us`, the countdown readers use;
Stroke comes at 1 s and Hold (Sleep) at 3 s of it. A tap is confirmed only
when the gap has passed with the head released, so Attend and Wake come
one gap after the hand leaves; Stop comes at qualification, not at
the release. After a Stroke the letting go asks nothing: it was affection.

`TouchEngine::set_head_pushed` takes the body's "the head is being pushed by
hand now". A contact that overlaps a push asks nothing more from then on and
reports no `held_us`; a push felt before the touch qualifies stops even the
Stop. A Stop already sent stands: waiting to feel a push would break the
150 ms stop. In `--touch` serve, the runtime reads the body's `body/hand`
(#848) every 20 ms, before each step's head samples, so a touch that begins
during a push asks nothing from its first sample. Only a fresh message from
the body that says `pushed` counts: missing, expired (the body gives each one
at most a second) or from anyone else is not a push, so a stopped body never
silences the head. A change is recorded as `head_pushed`, and a replay applies
it as the runtime did. The body feels a push reliably with `--give-way`, on
lamp-4ace since 2026-10-15 (the fifth, #703).
Polling can miss a pulse shorter than its interval; start qualification at
5 ms, and measure actual hand latency.

Both sources preserve acquisition time and consecutive sequence, rejecting
100 ms age/gaps, future/duplicate/reordered samples, wrong clocks, source
changes and malformed levels. Recovery requires a released baseline.
GPIO failure makes GPIO unknown; strip failure makes the strip unknown.
Neither is a zero/release, nor does either refresh the other source.
`TouchSourceState.active_mask` contains the raw acquired **working** bits;
current geometry and event masks are filtered evidence. Acquisitions and
recordings retain the complete raw twelve-bit mask. An excluded-only sample
cannot start a contact or keep one held. No observation is invented while a
finger crosses a blind area: an actual all-selected-released interval still
ends a contact after the release grace. An unchanged published state does not
mean a newly acquired sample.

### Reader, startup and shutdown

The new pins schema has `chip`, `buttons`, `head_pads`, `every_ms`, optional
`reset`, optional `base` and optional `legacy_head` (false unless set; see
above). Only chip 0 is allowed. Optional primary is line 99; reset is
active-low line 100, pulled up, in its own field. Head candidates are
37/96/97/98, selected and polarized per unit. Names/lines are unique and
match the Controls configuration. The privacy switch, chip 1 line 9, remains
Voice's. A base requires `base_selected_mask` to exactly match its working
electrodes (4095 for all twelve, 3823/0x0EEF for lamp-4ace's ten), and a complete
explicit profile: twelve independently specified touch thresholds and twelve
release thresholds, baseline filters, FFI/SFI/ESI, global/per-pad CDC/CDT,
debounce, ECR and auto-configuration limits. Unknown fields and missing
electrical settings fail validation before I/O. There is no shared-threshold
shortcut, and the served path never changes a threshold by itself; every
working pad is calibrated through its own assembled shell with a person at
the lamp (`calibrate-touch`, below). The arrays are indexed by electrode
number, not by position in the working list. Each release threshold is below
that electrode's touch threshold.

An I²C child does all reset, calibration and reads. The parent runs GPIO and
the nonblocking socket, draining at most 32 acquisitions from each per 2 ms
step. State is refreshed every 20 ms or on an event. No I²C, worker wait,
record-file write or hand-log output is in that polling path. A child packet
is bounded to 1 KiB; faults retain at most 256 UTF-8 bytes. Source time is
sampled before the I²C transaction sequence. A full socket drops the packet
without reusing its sequence, so the next arrival exposes the loss.
Touch recordings use a separate strict tagged schema. The existing bounded
recorder keeps its 32-entry queue, 32 MiB/100,000-record/16 KiB-line/ten-minute
limits; a dropped touch record inserts `all_fault` so replay cannot splice a
continuous hold across missing observations. Mixing legacy/touch formats fails.

The child must answer within seven seconds of spawn and thereafter within
150 ms of its last packet. These deadlines are checked before draining even
a fresh queued packet. Any failure inhibits it; no automatic replacement
starts. Teardown waits at most 100 ms, reports whether exit was actually
observed, and never calls a kill request proof of exit. A kernel-blocked child
may outlive that bound.

Besides `controls.lock` per bus root, the child holds the fixed advisory file
`/run/lock/lamp-controls-mpr121-<bus>-<address-hex>.lock` before opening I²C.
The file is mode 0600, regular, single-link and opened without following a
symlink. It is **never unlinked**; a still-blocked child retains ownership
even if its parent dies. The service user needs permission to create/open
this file and access `/dev/i2c-0`. A lock or spawn failure leaves GPIO working
and strip state unavailable. A GPIO open failure on Linux similarly leaves
the configured strip working. The lock excludes cooperating Controls owners,
not arbitrary external I²C tools: First must stop the prior reader first.

Initialization checks existing overcurrent before reset, verifies reset
CONFIG2, writes in Stop, starts with ECR last, waits a profile-derived settle
interval, then reads one sample, unpublished: status, part 0 of the static
configuration and ECR (the whole of it was verified in Stop just before,
and parts 1 to 4 follow over the next four reads). The observed lamp-4ace profile gives an
authored conservative wait of 1,044,480 µs, not a measured calibration time.
Each sample makes four combined transfers (26 register bytes), checking
status before/after a static configuration readback and ECR. The readback
is one fifth of the static configuration (17 of its 85 bytes) in turn, so
all of it every five samples (`CONFIG_PARTS`; 25 ms at `every_ms` 5, at
most 250 ms at 50); ECR, which a reset clears, is still checked every
sample. So a reset or brown-out is caught on the sample it happens (ECR
reads 0, and only 0x8c..0x8f passes), while any other register that
changes is caught within five samples: up to four masks read with it are
served before that, 20 ms at `every_ms` 5 (shorter than a tap can be
recognized: 10 ms of contact debounce and 30 ms of release grace, and
the fault resets the base) and 200 ms at 50. Read whole every sample the
check was about 1,000 bit-clocks, ~9.7 ms at the 100 kHz the air sensor
needs on the same bus (#1368), past the 5 ms pace; a sample is now ~3.5 ms
of wire time there and ~0.9 ms at 400 kHz, worked out from 354 bit-clocks,
not measured: the four ioctls, the driver's time between bytes and the
air sensor's turns on bus 0 come on top (the first builder, #705 17 Oct
19:04). On lamp-4ace at 100 kHz the median and largest gap between strip
sample stamps should stay near 5 ms; not yet shown. OVCF, OOR on any
working electrode, ACFF/ARFF, proximity/reserved status, stop/reset or
static-register drift are terminal unknown input. OOR confined to explicitly
excluded electrodes does not invalidate the working set. ECR enables a
contiguous hardware prefix, so all twelve are still scanned to reach E11;
excluding E4/E8 in software does not disable them electrically. An excluded
electrode can still cause a global auto-configuration failure, which remains
a fault and requires actual per-unit electrical qualification.
Auto-adjusted CDC/CDT are not falsely compared to their startup values.
No force-address, scan, adapter-global timeout/retry ioctl or bus-speed change
is made. This checks reported faults; it cannot undo a physical bus fault.

### Run without a lamp, then qualify on one

From `lampOS/`, the checked-in CLI fixture runs without hardware or a bus:

```sh
cargo run --offline -p lamp-controls -- replay --touch \
  --config crates/controls/fixtures/strip-711-controls.json \
  --pins crates/controls/fixtures/strip-711-pins-example.json \
  --input crates/controls/fixtures/strip-711-replay.jsonl \
  --clock-id strip-711-authored --enable --until-us 19948000
```

Expect 1,052 inputs, zero rejected, 54 events (one Touch, one stationary
Sleep alternative, 52 Slides, zero Stroke), 9,975 steps and 1,050 frames.
Only 57 changed masks came from First's lamp-4ace comment; 995 constant-mask
acquisitions, monotonic/session/sequence and receipt times are **authored**.
Replaying only the real changed rows correctly gives nine stale rejections
and no events. The [fixture provenance](../crates/controls/fixtures/strip-711-README.md)
keeps both versions. The original full-set example profile transcribes First's
earlier register dump; its uniform thresholds are historical evidence, not
recommended calibration. Its E0→E11 order is for replay coordinates.
The `strip-711-working-controls.json` and `strip-711-working-pins-example.json`
pair runs the same fixture with lamp-4ace's ten-pad working set and explicit
**authored** unequal thresholds. It gives the same geometry/events for these
rows, which never contain E4/E8. Neither example is a qualified live profile.

`--output FILE` creates a new private file, buffers writes in 8 KiB, and
returns success only after flushing the same JSONL bytes as stdout. An
existing file is refused. This is an offline replay path; it is not the
bounded live acquisition recorder.

First's on-lamp sequence is:

1. Keep the installed service as rollback. Stop the prior GPIO/I²C owner
   (`lamp-controls` itself included: calibration takes the same per-chip lock),
   and determine the unit's working electrodes. On lamp-4ace use the confirmed
   ten; other units must establish their own set. Then calibrate, following
   the hardware team's MPR121 procedure ([#711](https://github.com/autonomous-ai/lampOS/issues/711#issuecomment-6187078361)):

   ```sh
   lamp-controls calibrate-touch --pins <unit-touch-pins.json> \
     --output <unit-touch-pins-calibrated.json> > <calibration-report.json>
   ```

   It says each step on stderr:
   1. **10 s, hands away.** The chip starts with the profile's settings and
      auto-calibrates its charge. Nothing is measured.
   2. **5 s more, hands away.** Each working pad's noise: the largest
      |baseline − filtered| the chip may have compared. The chip shows only
      the upper 8 of its 10 baseline bits, so the noise counts up to 3 more
      than the read difference (NXP AN3892).
   3. **30 s: swipe each working pad a few times**, end to end. At each
      reading the touched pad is the one furthest above its own noise (more
      than 2). A swipe on a pad is 3 or more readings in a row in which it is
      that pad; a neighbour's touch reaching it is not. Each own swipe's peak
      (baseline − filtered) is kept, and what each pad reads while another is
      touched is its **coupling**.
   4. **Saved.** Each pad's touch level is the lowest of its own swipes that
      reached at least half its strongest, so a swipe that only grazed it does
      not set it. **Touch at 40% and release at 20% of the way from its noise
      to that level**, rounded down (`--touch-percent` 10..90,
      `--release-percent` below it), always noise < release < touch < level:
      the chip counts a touch only when its difference is strictly above the
      touch threshold, and a release only when it is strictly below the
      release threshold. A pad with no own swipe is `not_touched`; one whose
      level is less than 5 above its noise is `too_close_to_noise`. Only if
      every working pad has thresholds is the output written: the same pins
      file with those pads' thresholds replaced and nothing else changed.
      Electrodes left out of the working set are reported as `not_working`
      (E4 and E8 on lamp-4ace) and keep their thresholds. The report on stdout
      gives the rule in numbers and, per pad, the noise, the coupling, the
      number of its own swipes, their weakest and strongest, the level, the
      verdict, the thresholds, the baseline at the start of step 2 and the end
      of step 3, the charge current and time the chip chose, and how often the
      old thresholds counted a touch in step 3. It also gives the **footprint**:
      how many working pads were lit at once while a pad was touched, as the
      chip said with the profile's thresholds, and as the same readings replay
      with the new ones: a pad lights after the profile's touch debounce above
      its touch threshold and stays lit until as many readings below its
      release threshold. The replay uses the deltas as read; the chip may
      compare up to 3 more (baseline bits it does not show) and samples between
      readings, so it shows the likely footprint, not the chip's. The base's
      Touch and Sleep need three pads at once.

   `--record FILE` keeps the session's readings (a new private file);
   `--replay FILE` computes the report and the pins file from kept readings,
   with no chip, so a rule can be tried again on one person's swipes.
   `--settle-seconds`, `--quiet-seconds` and `--swipe-seconds` change the
   steps (1..600 s each). An existing output file is refused. A chip fault
   (overcurrent, out of range on a working pad, auto-configuration failure,
   a register that changed) ends the run and nothing is saved. The rule is
   authored, not a datasheet value: it aims to catch a touch lighter than the
   lightest swipe seen while staying clear of the noise. Edit the saved file
   if the hardware team wants other numbers. The noise filter settings
   (MHD/NHD/NCL/FDL) are the profile's and are not changed by this command.
   Charge auto-configuration still runs at each start, so the reported charge
   values are a record, not settings.

   Then **10 minutes with nobody touching, with the office as it is**: people
   walking past, phones on the desk, not a quiet room, since phantom touches
   come from the room. That is the hardware team's last step, on the saved
   file: step 3 below with `--seconds 600` and `--record`, hands off the lamp,
   then replay the record with the saved pins file. It must show no touch
   event. Verify any excluded pad does not cause a global chip fault.
2. With hands away initially, run `lamp-controls discover-head --seconds 20`,
   then touch and release each physical pad twice. It probes pull-down/up,
   requires driven idle and two complete ≥20 ms pulses, prints candidate
   polarity and installs nothing. A floating, untested or startup-held line
   is not a pad mapping. Set the actual observed candidates in the pins file;
   remove the nonexistent primary from lamp-4ace's configuration, not all lamps.
3. Run `lamp-controls serve --touch --config <unit-controls.json>
   --pins <unit-touch-pins.json> --bus-root <isolated-bus> --seconds 60
   --record <new-touch.jsonl>` first without `--enable`. Verify the closing
   counters, independent readiness, no capture/output changes and actual child
   cleanup. Never reuse an existing record pathname. Then repeat with enabled
   requests on that isolated bus before wiring a consumer.
4. Tap/hold each head pad, tap the base near both ends, hold for two seconds,
   slide both ways quickly and slowly, reverse, pause, lift one edge at a time,
   hover near the surface, cross both missing positions on lamp-4ace, and
   start/restart with a hand present. A missing electrode must not split an
   otherwise observed contiguous hand; a real blind interval must not become
   invented contact. Record
   first/repeated contact latency from a separate physical reference; acquisition
   intervals alone cannot measure hand-to-response latency.
5. Verify device permissions/lock conflict, missing strip and acquisition gaps
   leave GPIO usable. Test external reset/unplug only under First's hardware
   procedure. No fault may emit release, Sleep, reset or mute. Replay the full
   saved clip with its recorded boot ID and the exact configuration.
6. Voice/Expression owners apply fresh requests with privacy, contact-start
   lifecycle and dedup guards. Verify actual acoustic Stop, microphone closure
   on Sleep, one Wake from an asleep touch, and no Wake→Sleep from the same
   still-held hand. Qualify motor/light responses separately.

Most likely wrong on the lamp: the authored calibration rule (40% of the
way to the weakest swipe may still miss a lighter touch, or catch a hand near
the shell); a noise step taken at a quieter moment than the room's day; per-pad thresholds admitting a nearby hand;
startup-baseline learning; a faulty excluded pad upsetting auto-configuration;
a narrow finger becoming unobserved over a dead pad; the pulsed head not providing
sustained-hand evidence; sampled slide speed around pauses; I²C load alongside
Air; service-user lock access; physical GPIO polarity; and final owner latency.
No lamp, acoustic, electrical contention, sustained-head, privacy application,
light/motor response or physical timing result is established by this task's
host tests. Native ARM64 evidence and the complete gate are recorded in Third's
task log before the ready line becomes yes.

## What exists and what we take

V1 has three different inputs. Its mechanical button stops speech on a short
release. Its MPR121 base pad also stops speech. Its TTP223 head pads are for
petting and never stop speech. They are not interchangeable touch sensors.
The actual drivers, tests and `robots/lamp/docs/physical-controls.md` were read
before this implementation; the source-by-source decision is in
[reuse decisions](reuse-decisions.md#controls-keep-the-recognition-cases-change-the-actions-2026-10-14).

Keep V1's independent 200 ms press/release debounce, immediate first short
release, matched-edge requirement, startup-held suppression, release-only hold
actions, and touch-noise cases. Reuse the MPR121 fixture cases and its 5 ms
pad filter, 10 ms contact/chord qualification and three-pad minimum. Retain
TTP223's 500 ms settling, 200 ms contact grouping, 1.2 s decision window and
extending 1.5 s cooldown. These are source settings and historical fixtures,
not new measurements of this lamp.

For First's adapter, the repository already uses Rust
[`gpiocdev`](https://docs.rs/gpiocdev/latest/gpiocdev/) for physical privacy.
It supplies Linux line requests, timestamps and event sequence information.
The [official libgpiod Rust bindings](https://libgpiod.readthedocs.io/en/master/rust_api.html)
are another option, with a C library dependency. A plain
[`embedded-hal::InputPin`](https://docs.rs/embedded-hal/latest/embedded_hal/digital/index.html)
does not carry acquisition times or missed-edge evidence, so Controls uses
the slightly richer local `PinReader` trait. No GPIO library is added here.

Linux's [kernel debounce](https://www.kernel.org/doc/html/latest/userspace-api/gpio/gpio-v2-get-line-ioctl.html)
waits for a stable level and timestamps the end of that interval. Setting it
to 200 ms would discard quick taps and would not reproduce V1's per-polarity
suppression. Keep that rule in this engine. The
[edge-event sequence](https://www.kernel.org/doc/html/latest/userspace-api/gpio/gpio-v2-line-event-read.html)
must also be checked for overflow by the adapter. Active-low configuration
changes logical polarity; never guess it from a board name.

## Built here

`lamp-controls` is an optional process, crate `crates/controls`. It reads
complete observations through `PinReader`, recognizes gestures on supplied
monotonic clocks, publishes typed requests and feedback, and reports health.
With `--pins`, the `serve` command owns the configured GPIO inputs through
First's board reader. Without it, `MissingPins` reports that no adapter is
configured. Controls owns no capture, speaker, camera, ring or motor output.
First's installed service and the remaining physical checks are described below.

Hand requests and their receivers require explicit enablement. CLI configuration
cannot turn on a process without `--enable`; the flag overrides the file's
`enabled` value. Shadow mode publishes recognizable evidence with
`enabled:false`, which `Receiver` refuses as authority. A publishing success
means neither a stopped speaker nor a closed microphone.

| Input | Recognized gesture | Request |
|---|---|---|
| Mechanical button | Short release, under 2 s | Stop speech/playback/following through their owners |
| Mechanical button | Double or further short releases | Another Stop; no reboot and no unmute |
| Mechanical button | Release after at least 2 s, including 5 s and longer | One-way software mute |
| MPR121 base | Qualified stationary short release | Stop |
| MPR121 base | Second short contact | Stop, with double-tap classification |
| MPR121 base | Qualified stationary hold, released after at least 2 s | One-way software mute |
| TTP223 known head pads | Completed grouped touch/pet | Advisory Pet, never Stop or mute |

Stop and software mute are separate from physical privacy. No gesture opens
capture. There is no implicit wake, unmute, voice-mode toggle, Harness entry,
shutdown or reset. This deliberately changes V1's wake-on-tap and long-hold
actions. The 2 s and 5 s feedback tiers both mean mute on release; the 5 s
tier does not arm shutdown. Feedback carries duration and intent, not an old
red shutdown cue. First must provide a separate explicit owner-side clear
action before enabling software mute; the tested transaction is described
below. Repeated panic taps remain safe while muted.

Mechanical double-tap classification uses release-to-release **≤400 ms**.
Base double-tap classification uses previous release to next press **<400 ms**.
Those match the two V1 paths' different timing rules; neither delays the first
Stop. A release without an accepted press cannot emit an action. Fresh holds
show feedback while held but never commit before release.

Base touches require three selected pads simultaneously qualified for 10 ms;
separate short intervals cannot add up. Qualification survives staggered lift.
A moving contact is consumed without a tap, mute or swipe action. Stationary
qualified release uses 30 ms grace, the existing V1 fast-tap floor, instead of
V1 Automatic's 120 ms; ambiguous/moving contacts retain 120 ms. Spatial swipe
actions and the full V1 directional recognizer are deliberately absent. This
must be tried against real handoffs and noisy electrodes on the assembled base.

Head pads are explicitly known installed pads. There is no probing or learning
of candidate pins; V1's 20 ms LOW rule was for learning an unknown pad, not for
every touch. Both edges extend the 200 ms contact timer, and each completed
session restarts the 1.2 s decision timer. An isolated pulse resolves as Pet
1.4 s after its last edge. Directional labels, early reversal classification,
the first-session chirp and spoken phrases are not ported. State publishes
head-active edges promptly for an optional small visual reaction; it cannot
establish listening readiness. Any brain notification must remain limited to
one per 60 s by its consumer, as in V1; the Pet event is not permission to speak.

## The adapter contract

`PinReader::read(now_us)` returns immediately with at most one `Observation`,
`None` for no new acquisition, or a bounded `InputError`. `None` is never a
fresh released level. A full observation contains `ClockStamp`, reader session
identity, all configured named button/head levels, and an optional 12-bit base
mask. Logical `active:true` means pressed/touched. Configuration lists at most
eight buttons, four known head pads and one explicitly selected base mask.

Use the shared `lamp_ipc::monotonic_us()` clock and Linux boot ID. Preserve
actual acquisition times. A queued older edge must not receive the time it
was drained. Consecutive full observations have strictly increasing source
times and sequence numbers increasing by exactly one; batch simultaneous
changes into one snapshot. A restarted reader gets a new session ID. Report
kernel event loss, read errors, unknown levels and invalid MPR121 status as
faults, never as a released button or a zero touch mask. In particular,
[MPR121 overcurrent](https://www.nxp.com/docs/en/application-note/AN3892.pdf)
is a fault. Its chip debounce and software filters are different delays.

An observation or inter-observation gap of 100 ms is stale. Unknown, missing,
future, duplicate, out-of-order, wrong-clock, lost-sequence and changed-reader
input cancels recognition and withdraws hold feedback. Recovery requires a
new released baseline; a still-held contact cannot become a fresh press.
Clock reversal and exhausting the 32-retired-reader fence require a new
engine. Accepted Stop/mute records survive input failure independently of
pending recognition. All feedback duration comes from observed acquisition
time; an advancing timer cannot invent a held button during a reader stall.

Read the mechanical/head GPIO edges without discarding intervening transitions.
For an unchanged level, periodically acquire it again rather than replaying a
cached observation with a fresh stamp. Base polls normally run at 10 ms; the
adapter must describe honestly when each mask was read. Full observations are
a contract above those individual interfaces, not a claim that hardware reads
are simultaneous. If one configured input cannot be read, report the fault;
do not fill its value with the previous sample. Omit truly absent optional
inputs from configuration before starting instead of silently changing it.

The runtime targets one step every 2 ms, drains at most 64 observations per
step, ages a sample after the adapter returns, and faults on a full drain.
These are software budgets, not hard scheduling guarantees. `controls.lock`
allows one process per bus root. The OS releases the lock when a process dies.

## The board's reader (first builder, 2026-10-14)

`src/pins.rs`. `lamp-controls serve --pins FILE` reads the board's GPIO
lines named in FILE in place of the stand-in that reports Missing.
Without `--pins` nothing changes. Physical privacy observation additionally
requires the exact `LAMP_PRIVACY_CONTROLS=on` selection and an explicit `privacy`
section, described below; absent/off never opens chip 1.

The file is the board's wiring and nothing else: the chip, and for each
button and head pad its name (the same names, in the same order, as the
controls' own configuration), its line, whether a press or a touch pulls
it low, and how it is held when nothing drives it. A field nobody knows
is refused. `deploy/lamp-4ace/controls-pins.json` and
`deploy/lamp-4ace/controls.json` are lamp-4ace's.

Every configured line is read together every `every_ms` (5 on lamp-4ace)
and given as one complete observation stamped with the moment of the
reading, numbered one after another, under a session of the reader's own
that is new at every start. A line that cannot be read is reported as
Missing, never as released. The lines are taken for this program alone:
if another holds one, it does not start. The hand reader does not include the
privacy slide in these snapshots. Its separately selected reader owns a
different chip request and cannot become a hand gesture.

`lamp-controls probe --pins FILE` reads each line of the file once with
a pull-down and once with a pull-up, 10 ms each, and serves nothing: a
line that reads the same both ways is driven by something, and one that
follows the pull is not.

**On lamp-4ace, 2026-10-14, about 10:35, nobody touching it.** The older
software's record names the primary button on chip 0, line 99, and four
places a head pad may be wired, lines 37, 96, 97 and 98
(`robots/lamp/docs/physical-controls.md`). The probe:

| Line | With a pull-down | With a pull-up | So |
|---|---|---|---|
| 99, the button | high | high | Held high by the board: a button to ground, pressed is low |
| 37 | low | low | Driven low: a pad is wired here |
| 96 | low | low | Driven low: a pad is wired here |
| 97 | low | high | Follows the pull: nothing wired |
| 98 | low | high | Follows the pull: nothing wired |

So this lamp has the button and two head pads. The two pads rest low,
where the older software's page says a wired pad rests high: lamp-4ace's
file takes a touch to be high. **That is read from the lines at rest and
not from a touch.** Twenty seconds of `serve` with the file, watching
only: 3,232 observations taken, one every 6.2 ms, none refused, no
reading failed, the longest step 0.5 ms, and all three inputs at rest
throughout. `controls/state` was published.

**Running on lamp-4ace since 11:03 the same day**, as its own unit
(`deploy/lamp-4ace/lamp-controls.service`), watching only: no `--enable`, so
its gestures say `enabled:false` and nothing acts on them. Its two files sit
beside its build in `/opt/lampOS/controls/<sha>/`, because `/etc/lampOS` is
root's alone and the first start, with them there, failed on "Permission
denied". It says `controls/state` on the bus, and since 11:17 it writes one
line to its journal (standard error; standard output stays the closing
summary) whenever a hand changes something: the button pressed or let go,
the head touched or let go, a gesture, a fault. Changes only, at most 20
lines in ten seconds, the rest counted. Its first line was
`hand: button primary released, head not touched`. So the first real press
or pat is kept whenever it comes, with
`journalctl -u lamp-controls | grep hand:`. Polling every 5 ms it took 11.5 s
of processor time in its first 14 minutes, 1.4 % of one core. Its own
report on `diagnostics` said watching only as a warning, so for its first
hour the lamp's health read "has a warning" all day; since 11:56 watching
only is reported as well, with the same words, and the lamp is well
(`496421be0`).

Not shown: a press or a touch. Nobody's hand has been on the lamp for
this. The first press of the button and the first pat of the head, with
`serve --record`, will show which way each line goes and how long a
touch lasts, and the file is to be corrected from that before anything
acts on it. Voice now has an opt-in `controls/state` consumer, described
below; both hand-action switches remain off by default. That code is not
new evidence of a press or touch on this board.

## Topics and the receiver

Both types live in `lamp-msgs` and are additive; the old first-topic table and
all actuator contracts are unchanged.

| Topic | Kind | Meaning |
|---|---|---|
| `controls/gesture`, `ControlGesture` v1 | Event, 500 ms lifespan | Source, gesture, one-way intent, original release/duration, enabled flag and stable event identity |
| `controls/state`, `ControlsState` v1 | Latest, declared 100 ms heartbeat, 300 ms lifespan | Input health, observed button/base hold feedback, head activity, independent retained Stop and mute records; optional independent physical privacy evidence |
| `diagnostics`, `Diagnostic` | Event, once a second | Reader faults, counts, drops, publication/recording errors and explicit lack of physical confirmation |

State is also published on feedback transitions and gestures. Its session is
published before that step's event; consumers still handle loss and order.
Quiet gesture topics renew their publisher liveness once a second. A later
pet never overwrites a Stop/mute. Retained commands keep their original event
identity and time, so a new heartbeat does not create another action.

The opt-in Voice consumer subscribes to `controls/state` and advisory
`controls/gesture`. Body's consumer remains its owner's separate integration.
`receiver::Receiver` checks the payload, boot clock, enabled flag, monotonic
state, session and freshness, and deduplicates requests. It fences 32 retired
publisher sessions, then refuses further input until recreated with the saved
mute. This exhaustion is counted as unavailable hand input: unmuted Voice may
continue, while a muted Voice remains inhibited until restart and a fresh local
hold. Stop requires both original release and publication age below 500 ms;
state must be younger than 300 ms. A heartbeat cannot renew an old Stop. An
older retained mute can still close input. `Effects::stop_requested` identifies
only Stop, keeping Voice's Stop and mute switches independent; the legacy
combined `stop` field still includes a newly accepted mute.

`Receiver::new` remains one-way. Voice explicitly uses `with_hold_clear` when
software mute is enabled. A second hold can request clear only after the current
start/session/fault/gap fence and the previous mute watermark: its press must
have begun after both, its release and publication must be fresh, and the
current Controls owner must be ready. The first state after start or publisher
change cannot clear. Retained holds, duplicate delivery, a tap and a Controls
restart never clear. `prepare_clear` leaves the live latch muted; its proposed
record must be saved before `commit_clear`, synchronously with no other writer.
Voice only performs that transaction with audio workers absent.

`Receiver::pet` requires a fresh known owner and deduplicates advisory events.
Voice records `hand_pat_observed` with `action: none`; it does not speak, stop or
open input. Missing/stale Controls means no new hand gestures; otherwise healthy
Voice continues and an existing mute stays set.

The helper does not grant hardware permissions, authenticate arbitrary local
processes, or supply an actuator acknowledgement. Use the existing trusted
bus directory and validate the typed bus header/expiry before calling it.
Never use the bus header's publisher-relative `at_us` as the shared clock.

`voice/src/hand_controls.rs` keeps one subscriber pump for the entire service,
including its mute cycles. It checks source `controls`, exact topic/type/kind,
JSON encoding, declared lifespan, expiry and payload; a state must name its bus
publisher's instance. Gesture publication has a separate instance, while its
payload names the state owner. A 32-entry channel uses nonblocking sends;
coordinator polling consumes at most 16 entries without bus/filesystem I/O.
Overflow, queued discontinuity and a full poll batch fence pending clear; Stop
and one-way mute remain recoverable from later retained state. The pump targets
a 2 ms tick, retries subscription errors after 250 ms, and records bounded
health counts. Final owner destruction asks it to stop and waits at most 20 ms
before detaching a thread stuck in an OS read. It is not respawned per audio
cycle. These are scheduling/resource choices, not measured physical deadlines.

## Voice and physical privacy

With `LAMP_PRIVACY_CONTROLS` absent or `off`, the existing Voice GPIO worker
remains the physical slide reader. Selecting the new Controls producer does
not forge Voice's `PhysicalMute` restart token or grant capture. The consumer
must retain the fresh 50 ms observations, stable 60 ms unmute, verified shutdown,
new runtime identities and input-readiness checks in
[service privacy](service-privacy.md). Only one process may own chip 1 line 9;
the consumer's selected route must relinquish its direct GPIO request before
Controls can own the line.

### Separately selected physical slide, Task 755

The producer requires both the exact environment value
`LAMP_PRIVACY_CONTROLS=on` and this section in a legacy or touch pins profile:

```json
"privacy": {
  "chip": "/dev/gpiochip1",
  "line": 9,
  "active_low": true,
  "pull": "up"
}
```

This is the task's hardware-confirmed header-11 mapping: electrical LOW means
muted. All four values are checked; changing the chip, offset, polarity or pull
is rejected. The declared privacy line cannot also be a button or head-pad line
on the same chip. Header number 11 is not a GPIO line offset. The implementation
reuses the current `gpiocdev` request API and pull-up input convention, through
a separate request and injectable `Lines` reader. It adds no GPIO probing or
dependency. The board mapping is task evidence, not a new measurement by Third.

Absent, `off`, differently cased and other environment values do not select the
reader. No implicit chip-1 request is made by an old pins profile. A pins file
that has a `privacy` section is refused without the selection, as any unknown
field was before the switch existed, so a profile installed without it fails at
start instead of running with no hand input. The selected
profile defaults to observing its configured switch; removing the selection is
the explicit off override. `--enable` remains the hand-gesture authority flag.
Physical privacy has its own explicit opt-in and is not disabled by shadow hand
messages or by a failed hand/strip source. It never changes the software-mute
record. A fresh HIGH sample alone is not permission to reopen either microphone.

`ControlsState.privacy` is an optional `PrivacySwitch`, omitted entirely when
the producer is not selected. Existing messages retain their serialized fields
and type/version/declaration. The nested value carries an actual-acquisition
`ClockStamp`, `reader_session_id`, `muted: true|false|null`, and `fault`.
Exactly one of a known level or a fault is present. Before any accepted
acquisition, the unknown sentinel has time and sequence zero; it cannot be
fresh. Read/open failure is unknown `Missing`, never HIGH. A line that cannot
be taken (another process holds it, or this user may not open it) is asked for
again each second (`privacy::Reopening`, `REOPEN_EVERY`), each new reason said
once in the log, and a line that stops answering is let go and asked for again
(#795, lamp-4ace 2026-10-16); there is never another worker.

The independent monitor rejects malformed, future, wrong-boot, old, duplicate,
out-of-order and skipped acquisitions. Reader changes withdraw evidence for the
first new-reader sample; retired readers cannot return, and the retirement set
is bounded to 32. Clock reversal and exhausting that fence require a new owner.
At 50 ms, known evidence becomes unknown `Stale`, retaining the actual last
acquisition timestamp. An outer heartbeat, live strip or live button cannot
refresh it. The slide uses neither button debounce nor startup-held suppression:
an initial LOW is observed immediately. Consumers must validate the nested
50 ms age independently of the topic's existing 300 ms lifespan.

The acquisition target is 5 ms, with a selected owner-loop target of 1 ms
(the existing off-path loop stays 2 ms). While selected, Controls publishes state at
most 20 ms apart in its running software loop and on physical level/fault/reader
transitions immediately in that step. The selected Bus rescans subscribers every
5 ms so a later Voice subscriber need not wait the ordinary 200 ms discovery
interval, which exceeds the nested 50 ms freshness limit. The off route retains
the ordinary Bus setting. The existing declaration remains 100 ms with a 300 ms
lifespan. These are software scheduling targets; GPIO/system-call,
filesystem and scheduler delays are not hard-bounded. No lamp switch-to-gate or
switch-to-ring latency measurement is claimed here.

The physical field is independent of top-level hand `ready` and `fault`.
Base-only touch mode creates the `controls/state` owner when privacy is selected.
A privacy-only touch profile uses that state owner without inventing a button or
publishing an empty `TouchState`. Missing hand inputs may therefore coexist with
fresh physical switch evidence. No switch sample becomes a Stop, software mute,
clear, tap or touch event.

Acquisition recording and replay keep privacy samples/faults separate from hand
and strip input. Legacy records add optional `privacy` and `privacy_fault` fields;
touch records add `privacy` and `privacy_fault` input kinds. A recording gap
withdraws all potentially lost sources, while a GPIO/strip fault remains local.
The existing 32 MiB total, 100,000-record, 16 KiB-line and ten-minute limits still
apply. Replays open no GPIO, audio, I2C or Bus. Selected ordinary touch replay
adds an optional `controls` state beside its existing touch output; privacy-only
touch replay emits the ordinary Controls replay frames. Off output omits these
additions. That replayed `controls` state models the button alone: a recording
from pins with `legacy_head` (the product's) replays with no `head_active` and
no head `Pet` there, though the live `controls/state` has both (the reviewer on
#795).

The `privacy-*.json*` fixtures are authored host scenarios, not lamp captures:

```sh
LAMP_PRIVACY_CONTROLS=on lamp-controls replay \
  --config crates/controls/fixtures/privacy-controls.json \
  --pins crates/controls/fixtures/privacy-pins.json \
  --input crates/controls/fixtures/privacy-replay.jsonl \
  --clock-id privacy-test
```

Host tests cover startup LOW/HIGH, exact selector/off bytes, missing/stale input,
clock/order/reader faults, independent hand/strip failure, typed payload bounds,
privacy-only/base-only state and deterministic replay. Authored test coverage is
not a pass claim; the current task's validation report records executed checks.
The ignored `synthetic_line_to_bus_latency_with_stalled_strip` test reports first
and repeated elapsed times from an injected GPIO read through retained Bus
read/parse/validation, with the strip giving one sample then stalling. Its
authored 5 ms clock and local filesystem work do not measure an electrical
switch, subscriber delivery, microphone closure or the lamp's ring.
Voice's Linux-only `privacy_worker` integration fixture launches the selected
child with private IPC and Bus paths, a synthetic 5 ms producer already running
before subscription, and real Controller authority. It covers first-LOW
revocation, stable-HIGH gating, verified replacement and producer loss under the
unchanged 50/60 ms rules. It opens no GPIO or audio devices; authoring it does not
claim native ARM64 execution or lamp latency.

Voice's switches are independent and absent by default:

| Option | Effect |
|---|---|
| `--controls-stop` | Consume fresh, deduplicated Stop in the coordinator. |
| `--controls-mute ABS_PRIVATE_RECORD` | Restore software mute and enable released-hold mute/fresh-second-hold clear. The parent directory must already exist, be trusted and private (0700). |
| `--controls-bus-root ABS_PATH` | Override `LAMP_BUS_ROOT` or `/run/lampos-bus`; this option alone enables nothing. |
| `LAMP_HEAD_TOUCH=on` (environment) | Consume the head pads' requests from `--touch` Controls (#711). A qualified head touch stops the lamp while it says its answer (its reply has audio or playback), through the same cancellation as Stop (the speaker's discard of the rest of the answer is then expected, as for the button's Stop, the mute, sleep and a capture gap; before 16 Oct it ended the voice, which came back muted); while it is quiet or still thinking, it is only noted (`head_touch_noticed`). With `--controls-mute` too, a hand held 3 s on the head puts the lamp to sleep: the software mute with the reason `sleep`, traced as `head_sleep` (`hand_control_cancel` and `service_cycle_muted`); a later tap on the head wakes it through the saved clear, as the usual unmute does. A tap on a quiet lamp plays a soft "hm?" of its own, at most once in 30 s ([attention.md](attention.md#a-tap-on-its-head-is-a-call-711)). The voice then says `voice/mute` (muted, and `reason: sleep` while asleep) when it changes and every second, in a conversation and in the wait, from its one controls pump. Without `--controls-mute` a hold is not a sleep and `voice/mute` is not said. A head tap counts as a call for 2 s in the attention rule, the window Expression shows (and the ring, when the hum plays), with `LAMP_BUS=on` ([attention.md](attention.md#a-tap-on-its-head-is-a-call-711)). A hand kept on the head dims the ring's face from 1 to 3 s, a countdown to sleep ([ring-choreography.md](ring-choreography.md#a-hand-kept-on-the-head-the-countdown-711)). A tap while the lamp waits out a mute or a closed privacy slide shows the asleep face for about a second, where that face is the muted look (not with `LAMP_PRIVACY_CONTROLS=on` or `LAMP_RING_ASLEEP=on`), traced as `head_tap_muted_face` ([ring-choreography.md](ring-choreography.md#the-asleep-face-712)). |

For example, append the selected options to `lamp-voice serve
PRIVATE_CONFIG.json NEW_OUTPUT`. No controls clock/file read or pump starts
when both action switches are off. The separate Controls producer must also use
`--enable`; its default shadow messages are never actions. Changing/removing the
mute path is an operator configuration change, not a local clear procedure;
keep the same path and mute option across restarts.

After physical privacy validation and before provider traffic, Voice polls the
hand controls. Stop revokes the current turn, clears pending local PCM/provider
input and abandons the current input until its end. It sends fresh authority to
the speaker before provider, diagnostic, persistence or presentation work. Late
output of the stopped turn cannot revive the answer or schedule another look.
The existing playback ownership checks drop held audio. Stop neither admits a
replacement answer nor clears software/physical mute. This task does not cancel
unrelated agent work or add a new Body, music, endpoint or Policy action.

Software mute locally revokes microphone/camera/output permission and cancels
the answer. The already-armed durable guard keeps restart inhibited; after
teardown the next quiescent poll saves the newly observed request watermark.
A terminal service exit also attempts that save. `SoftwareMute` is distinct
from the physical token. `serve` returns to its privacy-only wait
only after verified cleanup, and polls controls throughout that wait, even when
the slide remains in one position. A finite `directed` run ends on mute; it does
not automatically create a new runtime. A fresh explicit clear while the slide
is closed removes only software inhibition. Opening the slide alone never
clears it. New audio still requires fresh stable physical permission, verified
old-worker destruction and the ordinary new-runtime readiness path.

`MuteRecord` is loaded and its initial/rebased state saved before any worker is
opened. Corrupt, future, exposed or interrupted state refuses startup. The mute
preference now survives a **machine reboot** as well as a service restart;
timestamps are rebased to the new boot while the muted value is preserved.
The same-boot release watermark prevents a retained pre-clear hold from
re-muting. The JSON file is private; no CLI edits it behind the live owner.

**Sleep from the head is this mute with a reason (#711).** First decided it
on #711: one way to switch the microphones off, kept across restarts and
updates, so a lamp put to sleep stays asleep. `MuteRecord` has an optional
`reason`, written only while muted (`"reason":"sleep"`); a record without it
reads as before, and an unmuted record with one is refused.
- `Receiver::request_sleep` takes a head Hold that `TouchReceiver` accepted:
  muted, reason `sleep`, persisted as any new mute. A contact that began at
  or before the last mute change or clear asks nothing, so a hand that woke
  the lamp cannot send it back. A lamp already muted keeps its mute and
  reason: sleep never takes over a person's privacy mute.
- `Receiver::request_wake` takes a head Tap that `TouchReceiver` accepted,
  only while the reason is `sleep`, only with `with_hold_clear`, only for a
  tap that began after the owner's clear fence and the sleep, and released
  within 500 ms. It marks the clear pending; the quiescent owner prepares,
  saves and commits it exactly as for a clearing hold.
- The button's hold still clears a sleep. A button hold that becomes a mute
  instead makes the reason none: a person's privacy, which a tap never opens.
- A rebased record keeps its reason across a machine reboot.

Before audio starts, `arm_active` durably stores a conservative muted recovery
record without changing the live preference. A crash, provider fault or failed
cleanup therefore restores muted, even if no hand mute had been requested. This
is a deliberate availability cost: a fresh local hold is needed after restart
(or, with `LAMP_RESTORED_MUTE=on` and `LAMP_HEAD_TOUCH=on`, a fresh head tap,
below).
Only verified clean destruction permits `disarm_after_cleanup` to store the
actual unmuted preference, and so does a stop the service was asked for
(`guard_released_on_stop`, #960): SIGTERM or SIGINT ended the conversation,
cleanly or at a termination check, and then cleanup reported a fault. On
lamp-4ace at 09:40 on 2026-10-16 an ordinary restart for an install logged
"endpoint worker exit not confirmed within 500 ms" and came back muted, deaf
with nothing to show; such a stop now writes the latch as it was. A chosen
mute stays muted; a fault or a privacy exit keeps the guard. A failed initial arm opens no audio; an accepted mute
whose subsequent save fails still has the active durable guard. Persistence
failure stays visible and cannot clear the live latch.

**A restored mute told from a chosen one** (#960 point 2, `LAMP_RESTORED_MUTE=on`
on the voice, off unless set). Arming also leaves a private marker,
`<record>.restart-guard`, beside the record, written before the guard and
naming the guard record it goes with (its `clock_id` and `through_us`). A clean
release, a mute the person chose (as soon as it is taken), and a clear each
remove it. A restart that finds the record muted with the marker present, and
the record as stored (before any rebase on a new boot) still the one the
marker names, knows the mute is the guard's after a fault: `HandControls::restored_after_fault`, the trace
record `mute_restored_after_fault` and one log line. A marker beside an unmuted
record (a fault between the two writes), or found with the switch off, is
removed. **Writing** the marker is advice: failing to write it changes no
permission, since a missing marker reads as a chosen mute, which stays shut.
**Removing** it is not, because the marker is what lets a tap clear a mute
(below). So it goes as soon as a person's mute is accepted: after every
conversation, whatever its result, after revocation and before teardown
(`HandControls::mute_chosen`, from `run_inner`; `arm_active` refuses to start
muted, so a muted live latch there is the person's), and again before a muted
record is saved (`persist_muted`, `disarm_after_cleanup`), never after. The
folder is synced after the unlink, as a save is. A removal or sync that fails
is a persistence fault for this run, and the person's mute is still saved.
Across the restart that fault causes, a marker that could not be removed no
longer counts: every person's mute moves `through_us` forward, so it never
matches the guard record the marker names. A crash in teardown, between the
removal and the save, or after a removal that failed while the save went
through, therefore never lets a tap open a person's mute (the reviewer's P1
and P2 on #997). One case is left, inherent to keeping the person's mute in
the same record: if both the removal and the save fail, the record on disk is
still the guard's and the marker still names it, so after the restart a tap
can wake it, as a guard's mute. The person's mute never reached the disk in
that case, which the persistence fault says.

The marker is written with its folder synced before the guard is saved, so a
power cut soon after arming cannot leave it empty. After a reboot, restore
saves the record rebased to the new boot's clock, and when the marker matched
it is rewritten to name that saved record: a second restart in the same boot,
such as an update's clean stop, still knows the mute is the guard's (the first
builder on #997). If that rewrite fails, the next restart no longer matches,
and the mute reads as a chosen one.
**A head tap wakes such a mute, as it wakes sleep** (`Receiver::request_wake_after_fault`):
on lamp-4ace on 2026-10-16 a crash at 12:20 brought the voice up muted, and
with no main button and the base strip off nothing at the lamp could open it.
The tap is checked as a wake is (fresh, begun after the clear fence and the
mute, released within the event lifetime), and only makes the clear pending
for the quiescent owner to save. It needs `LAMP_HEAD_TOUCH=on` too: there is
no head receiver without it. A person's own mute, or any mute without the
switch, stays until their hold. With `LAMP_RING_ASLEEP=on` (#1025) the
wait shows the asleep face, not a dark ring.

Saves retain a private `.pending` inode while linking/renaming the replacement
and syncing file and directory; `.replacement` and `.pending` evidence both
refuse later startup. Removing the pending marker is the final operation.
A crash may leave or resurrect a marker after an otherwise successful write,
requiring explicit operator recovery. No arbitrary storage/power-failure
guarantee is claimed. Clear saves its proposed unmuted record before changing
the live latch, then normal startup writes the active guard again. Disk sync
has no hard userspace bound and happens only outside the urgent audio loop.

The design target is **2 ms from coordinator observation to the local speaker
stop-authority send**. `hand_control_cancel` names both timestamps and the
original release separately; for a head touch, `reason` is `head_stop` and
`acquired_at_us` is Controls' qualifying acquisition.

**The head's stop (#711), from the touch to the speaker:** Controls qualifies
a touch after 20 ms of contact, at its next 5 ms acquisition; the bus and the
pump bring it to the coordinator's poll; the same cancellation sends the
speaker its stop. Measured on Third's Intel i7 Mac (macOS 26.6.2, release
build, `head_touch_bus_to_poll_latency`, ignored by default), from the touch
state and event being published on a real bus to the listener accepting it,
three runs of 100: medians 2.0, 2.1 and 1.8 ms, p95 2.7 ms in each, and a
worst of 2.9 ms in the first (the other two worsts were not kept).
It is bounded by the pump's 2 ms tick. An earlier run once failed to deliver
a touch within 2 s; it did not happen again in three runs, and the probe now
prints the listener's counts if it does. The 150 ms target from the touch to
silence still needs the lamp: the pad itself, the speaker worker and the
device's own buffer are not in these numbers. Injected-clock checks cover that software boundary;
GPIO recognition, bus/pump delivery, speaker processing and acoustic silence are
outside it. Measured results belong in the qualification entry, with their
machine and exact boundary, not inferred from this target.

Before enabling actions, First shadow-records one button press and both head
pads. The installed `lamp-controls` service already owns those GPIO lines:
stop that owner before a foreground recording, or add recording to that same
service invocation. Never run two readers together. The foreground command is
`lamp-controls serve --config CURRENT_CONFIG --pins CURRENT_PINS --record
NEW_RECORD`, without `--enable`; use the installed files beside its build,
as described above. Correct polarity from those traces first, then restore
the single service owner and use the five operator sequences:

| Action at the lamp | Expected result |
|---|---|
| 1. During an audible answer, press/release the primary button once; repeat rapidly. | First release cuts the old answer locally; repeated delivery never restores it. No tap unmutes or starts an answer. |
| 2. Tap while waiting for an answer, then while listening/idle. | Pending/late output is abandoned. Idle Stop produces no spoken acknowledgement or new turn. |
| 3. Hold at least 2 seconds, release, then restart Voice after the mute save succeeds. | No action before release; capture closes and remains closed after restart using the same record. |
| 4. While software-muted, close/open the slide, then make a fresh second hold; also check a fresh clear with the slide closed. | Slide opening alone never clears mute. Explicit clear cannot open capture through a closed slide; reopening requires the normal fresh privacy/startup path. |
| 5. Pat each head pad, and repeat while muted. | Advisory `hand_pat_observed` only: no Stop, speech, mic opening or clear. |

These are First's future physical checks, not Fourth's results. The leading
board uncertainty remains head-pad polarity inferred only at rest. Body's
separate final intention boundary must use its locally safe controlled stop;
no gesture supplies raw joint targets, torque-off or clearance for a new move.
Controls emits no spoken listening cue and no feedback claims capture readiness.

## Replay and local records

One written scenario exercises both button taps, a base tap, a released hold,
head petting, lost input during another hold, recovery, and a moving base
contact. From the standalone `lampOS/` root with the binary on `PATH`:

```sh
lamp-controls replay --config crates/controls/fixtures/controls.json --input crates/controls/fixtures/hand-controls.jsonl --clock-id written-controls --enable --output /var/tmp/controls-written-20261014.jsonl
```

Use a fresh output filename. The fixture has 901 receipt records, 900 complete
acquisitions and one explicit overflow. It yields six requests with no rejected
acquisitions; the held contact spanning the fault produces no mute. Replay
never opens a bus or hardware, and `--enable` only labels simulated output.
It has a supplied 2 ms clock, at most ten simulated minutes, 32 MiB input,
100,000 records and 16 KiB per line. It emits changes/gestures and 100 ms
snapshots. EOF creates no fresh samples; feedback becomes stale.

For real acquisitions, record through the single GPIO owner as described above.
Use its installed config and pin files; these placeholders are absolute paths:

```sh
lamp-controls serve --config CURRENT_CONFIG --pins CURRENT_PINS --record NEW_RECORD --seconds 60
lamp-controls replay --config CURRENT_CONFIG --input NEW_RECORD --clock-id "$(cat /proc/sys/kernel/random/boot_id)" --enable --output NEW_REPLAY
```

Omitting `--pins` truthfully records Missing faults instead of GPIO acquisitions.
The optional writer has a 32-item queue, creates a new 0600 file, and cannot
block the reader. It shares replay's size/count/time limits and reserves room
for an overflow marker. Lost acquisitions interrupt replay recognition; a
terminal drop cancels pending timers at the last recorded time. Invalid large
observations become a bounded Invalid record. Writer drops/errors and a finish
timeout are reported; a recording with any of these is incomplete evidence.
Shutdown gives the optional writer 100 ms to finish; disk synchronization is
not a hard userspace bound. Replay output errors are returned, never called a
complete replay. Keep the printed summary with a real recording.

## Qualification and evidence

The unchanged workspace built on Third's x86_64 development Mac before this
task. The new tests exercise pure clocks, historical/written touch masks, local
Unix datagrams, source/owner restarts, input loss, private mute files, a real
CLI child and SIGTERM. The final gate and standalone-copy evidence are recorded
in [Third's log](https://github.com/autonomous-ai/lampOS/blob/lamp-third/lampOS/docs/work-log-third.md) and the
[host evidence](verification/controls-host-20261014/results.json); none is an
on-lamp result. The Controls crate has 121 tests and the new message contracts
have 13. The source-only copy passed all 159 Controls/message checks, including
the pre-existing message tests, and built the whole workspace independently.

Written nominal 2 ms polling cases cover 16 first/repeated GPIO releases at
eight phases: release-to-recognizer dispatch is 0–1,999 simulated microseconds.
Base pulse cases at 10 ms polling give 32.45–39.95 simulated milliseconds from
authored release to recognition, including sampling phase; accepted cases are
exactly 30 ms after the first sampled full release. Neither includes electrical
noise, chip processing, bus/owner delay, buffered audio or actual joint motion.

One release-mode invocation ran the written replay 100 times on Third's Intel
Core i7-10700K development Mac (macOS 26.6.2, Rust 1.96.0). The measured boundary
was input parsing, engine construction, injected-clock replay and JSON
serialization to a sink: first iteration 3.340848 ms, median 2.975859 ms, worst
3.340848 ms. Configuration parsing preceded timing. This excludes process
startup, the bus, disk writing, scheduling jitter and all physical response;
there is no before/after comparison. Reproduce that host workload with
`cargo run --offline --release -p lamp-controls --example replay_cost -- 100`.

**Fourth, task 21: local Stop dispatch timing.** A release-mode probe on Fourth's
macOS 26.6.2 (25G83), x86_64 Intel Core i7-10700K at 3.80 GHz, 16 logical CPUs,
64 GiB, Rust/Cargo 1.96.0 measured 41 µs on the first call. The next 99 calls
had median 6 µs, nearest-rank p95 9 µs and worst 16 µs; the worst across all
100 was 41 µs. Timing began immediately before `hand::cancel` and ended at the
`lamp_ipc::monotonic_us` sample after a successful local speaker Authority
datagram send. Fixture setup, allocation and teardown were excluded. This
fixture holds 480 queued PCM samples, one pending speaker receipt and two
provider input commands; it is not a worst-case backlog measurement. The probe
does not include gesture recognition, bus delivery, speaker-worker scheduling,
device playback or acoustic silence. Other local tests were running; this is
not a controlled cold-cache or cold-boot measurement. Separate injected cases
of 0, 700, 1,500 and 2,000 µs use authored clocks to check send-before-sample
ordering and the 2 ms assertion; they are not wall-clock measurements. From
`lampOS`, reproduce the release probe with
`cargo test -p lamp-voice --release --lib coordinator::tests::hand_controls::local_hand_stop_dispatch_latency --offline -- --exact --ignored --nocapture --test-threads=1`.
Task 21's final serial gates passed separately: on the Mac, 142 Controls,
973 Voice and 284 Voice-eval tests; in the standalone Linux x86_64 source copy,
142 Controls, 977 Voice and 284 Voice-eval tests. Each had six opt-in probes
ignored; the new timing probe was run explicitly above. Both workspace builds
passed. Native strict Clippy passed; ARM64 strict dependency Clippy found an
inherited Body dead-code warning, while selected-crate `--no-deps` checking
passed. Earlier test failures, their fixes and the repeated timing-sensitive
cases remain in [Fourth's log](https://github.com/autonomous-ai/lampOS/blob/lamp-fourth/lampOS/docs/work-log-fourth.md) and
[task 21 evidence](verification/controls-voice-host-20261014/results.json).
No ARM program or lamp hardware was run by Fourth.

First checks the configured lines and electrodes, then observes real taps,
holds, repeated panic taps, head pats and untouched motor/speaker activity.
Measure from a physical input observation to owner revocation, then separately
to acoustic silence and motion stopping. Try a press during provider/network
stall, a held boot, reader overflow/unplug, Controls SIGKILL/restart, Voice
restart while muted, a physical-switch cycle and an explicit software clear
while the switch is closed. No old reply or captured audio may resume. Faults
must remain faults; a stopped component or a successful bus send is not proof
that an actuator stopped.

## Not shown

Neither GPIO/I²C adapter was run on a lamp by Third. Installed subscriptions,
physical privacy transitions, electrode-noise acceptance, touch-to-silence
latency and safe motion stops remain unmeasured by Third. No hardware feedback
or applied mute/clear receipt is provided by this process. The most likely lamp
mismatch is base qualification
and release timing under its actual electrodes/noise, followed by lost GPIO
edges or an acquisition clock converted incorrectly. Third's original handback
could not operate a hand control until First supplied the real reader and the
owners' integration. First has since added the board reader above, and Fourth's
task 21 adds the optional Voice consumer. Those additions do not establish a
physical press, touch, acoustic Stop or Body integration result.
