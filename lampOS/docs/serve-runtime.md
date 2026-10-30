# Continuous chat core

`lamp-voice serve PRIVATE_CONFIG.json NEW_OUTPUT [session options]` uses the same
coordinator, provider recovery, capture, speaker/reference, privacy and output
ownership as `directed`. It has no elapsed conversation deadline and does not
restart the provider or cancel an answer at 600 seconds. Startup still requires
real retained microphone data and the current provider-ready session within the
existing 25-second readiness budget. Time alone cannot create readiness.

`NEW_OUTPUT` must be a new leaf under an existing parent. The coordinator creates
it with mode 0700. Provision the provider configuration separately; no credentials
belong in command output or the source tree. This mode introduces no Python,
legacy service or shell launcher dependency. Deployment and native qualification remain separate work; this core alone is
not an all-day device release. Physical mute is supervised as described in
[service privacy](service-privacy.md).

The ordinary `SessionOptions` flags remain explicit. No camera or ring is opened
by default; no motor is opened by this coordinator. Existing noise suppression,
turn-taking, endpoint and playout defaults are unchanged. For example:

```text
lamp-voice serve /etc/lampOS/gemini.json /var/lib/lampOS/sessions/NEW_ID \
  --turn-taking v1-baseline --playout-start-ms 200
```

These explicit policies require their own physical qualification. Adding
`--camera-config ABS_PATH` or `--ring-channel-ceiling 0..120` retains the same
camera/privacy and phase-bound ring authority. The supervisor selects a private
continuous ring-worker lifetime instead of its finite 630-second ceiling.
Initial authority, parent transport, installed permission expiry and 100 ms
frame age still apply. The lifetime option supplies no output permission.
A ring worker that faults (SPI writes over 10 ms in a run lasting 2 s, #911)
is started again by the voice, at most three times a run and 2 s after
the fault, off the loop, so a stop after it finds a live ring that writes the
stop's black; past the third the ring stays off and the voice's health says
`ring` = `off` (#1620, [ring-choreography.md](ring-choreography.md), "Slow SPI
writes and the ring's restart").

`serve` rejects `--diagnostics` before startup. It does not silently keep
pre/post-AEC or render PCM forever. Use a separate finite `directed` experiment
for raw diagnostics. Finite `directed` and `directed-fixture` retain their 1..600
second duration, strict 20,000-row evidence cap, complete trace file and existing
failed-on-signal behavior.

## Metadata never supplies interaction authority

Cancellation and accepted-output receipt lineage now lives in a typed bounded
ledger. A dispatched command retains its original owner/sequence through partial
acceptance or rejection. Accepted cursors remain unsettled until an actual drain,
retirement or reset/discard receipt. The original successor admission is retained
inside a cancelled owner's entry. Completing a successor cannot relabel the old
tail or erase that relationship. At most 64 owner entries exist; only settled
terminal entries can be evicted. Exhausting the bound with genuinely unsettled
commands is a protocol fault, not a logging fault.

The cancelled-tail and expected-discard handlers consult this ledger, never JSON
rows. Metadata deletion, rotation or forged rows cannot grant authority or erase
a pending obligation. Normal controller permission checks, privacy priority,
request/session matching and the accepted-tail deadline remain unchanged.

Service ring logs are append-only request/feedback events. They do not keep
mutable indexes into old log batches. Of the light's renewals, sent every
20 ms and each validated as before, only the first request of each cue and
of each second is written down with its receipt; a renewal's plain success
is not, and a rejection, a blank or a receipt that fails validation always
is. Written out in full they were about a hundred rows a second: on
lamp-4ace they filled fourteen of the record's sixteen megabytes and left
it about seven minutes long, too short to look back at an afternoon of use. The real choreographer still retains and
validates its one outstanding frame independently. Cue emission consumes each
bounded metadata batch once while retaining its boot, capture/reference context
and datagram sequence. Losing a local batch invalidates further scheduling cues;
a slow or absent cue receiver does not stop the conversation. A requested cue
endpoint must still validate and connect at startup. No new strict-cue mode is
introduced.

## Bounded best-effort storage

The coordinator retains at most 256 metadata rows per loop batch, each with a
32 KiB serialized ceiling. It performs bounded serialization and a nonblocking
push into a 64-slot SPSC queue. Only the metadata thread performs file or stderr
I/O after construction. This targets sub-millisecond metadata submission on the
host; it is not a target-hardware latency guarantee.

The writer exclusively opens four private files before activation and keeps the
descriptors pinned. `metadata/events-0.jsonl` through `events-3.jsonl` rotate at
4 MiB each; their total event storage is at most 16 MiB, plus the small health
file. Rotation truncates an already-owned descriptor rather than following a
replaced pathname. A segment header identifies its generation; every retained
service row has a monotonically increasing `service_sequence`. Rows may include
private conversation transcripts; no raw PCM is written by this logger.

Queue saturation, oversized rows and disk failures latch health/counters and drop
metadata. They do not return an audio-loop error, revoke an owner, create readiness
or restart Gemini. A writer fault stops logging for this invocation; it does not
retry or grow a replacement queue. `metadata/health.json` reports counts and
rotation state when the writer makes progress. The writer emits at most one
bounded dropped-event warning and one fault warning to stderr. An indefinitely
stalled native filesystem call can delay that thread's health/stderr publication;
the in-memory counters remain latched. No syscall preemption is claimed.

These rolling files are deliberately incomplete evidence, even without detected
drops. They cannot qualify acoustic latency or a benchmark denominator.
`unconfirmed_events` distinguishes accepted metadata not yet written, including
pending or abandoned prefixes. The final `service_metadata_finished` stdout row
reports the bounded finish acknowledgement separately from the run outcome.

## Terminal provider shutdown

A recoverable outage keeps the existing provider recovery policy and local
interruption path. A terminal status alone does not end serve: the priority
status can arrive before valid queued audio and that request's final outcome.
Only the ordered provider `Ended` packet enables terminal service shutdown.
Submitted Starts can still receive their local `NotDelivered` outcome afterward.

Serve continues normal privacy, capture and speaker servicing while retained
work settles. Abandoned active input drains to the existing detector's actual
endpoint without replay or detector reset. Already received PCM keeps its
original owner through actual acceptance/retirement. Each turn gets its existing
completion or failure outcome once; a partially generated failed answer remains
failed even if its retained PCM is played. Pending commands and accepted cursors
of cancelled owners must also settle in the bounded live receipt ledger.

After no reply, abandoned input drain, queued input or speaker obligation remains,
serve records `service_provider_ended` and returns an explicit terminal-provider
error. It does not fabricate readiness, restart the provider, replay input or
return to the physical-mute supervisor. Ordinary worker, privacy and protocol
faults retain their existing immediate failure behavior. This adds no grace
period or response timeout; incomplete obligations remain subject to their
existing capture, authority, transport and utterance bounds. Finite experiments
keep their existing lifetime behavior. The tests use synthetic peers and real
local IPC, not a cloud outage or acoustic measurement.

One terminal end is treated differently after all of that: the provider's
quota or its rate limit (`quota_exceeded`, `rate_limited` in the terminal link
state). Serve shuts down and records exactly as above, and then the process
stays for 120 s before it exits with the same failure status, with a line in
the journal saying so; a request to stop ends the wait at once. The reason is
whoever restarts the service. On lamp-4ace on 2026-10-13 the relay refused
every new session with `quota_exceeded` from about 07:05 local, each start
ended within a second, and systemd (`Restart=on-failure`, `RestartSec=4s`)
started it again about 380 times in half an hour before anyone looked. With
the wait that is one request every two minutes. The lamp is still silent and
dark for as long as the refusal lasts; nothing here tells the person in front
of it why. Checked by a unit test of which faults count as such a refusal,
not against a provider that refuses.

## How loud an utterance was

Each `local_endpoint` record carries `speech_level` when the detector scored
any block of the last eight seconds as speech: how many such blocks, and the
middle and the upper tenth of their levels after cancellation, on the face's
scale (0 is -48 dBFS or under, 255 is -14 dBFS or over). It is there to find
out whether someone at the lamp can be told from someone across the room by
level; nothing acts on it yet. Added on 2026-10-13, when an office full of
people talking brought the conversation checks down: other voices were taken
for the person at the lamp going on, and for that person cutting in.
What has been done about that since is in [group-talk.md](group-talk.md).

## Beside other work on the lamp

The capture worker ends on a microphone read that took more than 10 ms, and
the reference path on a speaker reference about 20 ms late. Both are meant
as faults: after either, the echo canceller's timing is not known. Both end
the service, and whoever supervises it starts it again a few seconds later
with a new conversation.

At the default scheduling these are easy to cause. On lamp-4ace on
2026-10-13 the service ended twice in an afternoon of played checks (once
on each fault), each time within seconds of the test harness logging in to
the lamp. Then, on purpose: twelve busy loops for forty seconds on its
eight cores ended the service four times in those forty seconds. With the
unit given round-robin real-time scheduling at a modest priority
(`CPUSchedulingPolicy=rr`, `CPUSchedulingPriority=20`, which its workers
inherit; see [`deploy/lamp-4ace/lamp-voice.service`](../deploy/lamp-4ace/lamp-voice.service)),
the same forty seconds ended it not once, and the body and the eyes ran
through it. The processor reached 92 to 95 degrees C in twenty seconds of
that load either way, which is its own limit on what else this lamp can be
asked to do.

### The capture worker's cores

The workers inherit the unit's affinity as well as its priority: every one
is `RR 20` on cores `0-7`, and the scheduler places it. lamp-4ace has two
clusters: policy0 is cores 0-5, the six small cores (max 1794 MHz), and
policy6 is cores 6-7, the two big cores (max 2002 MHz). On 18 October
(09:54-09:59, the first builder on #705, `pidstat -t -u` every 10 s, a quiet
room) `worker capture` took 17.0-17.4 % on small cores 0, 1, 2, 3 and 5,
then moved to big core 6 and took 6.5-7.2 % there, with the small cores
mostly at 1794 MHz and no zone over 69.5 degrees C: the core type, not the
clock.

**Don't set `LAMP_CAPTURE_CORES`: it crashes the voice in a conversation
on lamp-4ace (#1459, 18 Oct 11:20); it is only for the quiet-room
measurement.** At `6-7` (the first builder, 11:06–11:21, `b9b4a5d66`) the
quiet share fell from 17.65 % to 6.17 %, but in four played talking turns
the lamp answered none, capture stalled three times (99, 58 and 67 ms,
`LAMP_STALLS=on`), the second route's and the speaker's leases lapsed, and
the voice exited with status 1 and came back muted by the restart guard.
The fix (`lamp-turn-end` off capture's cores) is #1475, and the switch
stays unset until the same check passes: four turns answered, no lapses,
capture stalls no worse than with it unset.

`LAMP_CAPTURE_CORES=6-7` (off unless set, Linux only) keeps capture
on the big cores: at its start the worker calls `sched_setaffinity` on its
own thread, before the threads it starts, and logs one line with the cores
and what `sched_getaffinity` reads back ("it took" or "it did not take").
If the read-back differs (a cpuset allowing only part of the list), the
mask read before the change is set back and the line says so, so the
default is really kept. Unset, nothing is called. A value that is not a
cpu-list (`6-7`, `6,7`, `6`), or that names a core not online, is logged
and the default kept; the voice never stops for it. The supervisor passes
it to the capture worker only. The risk is the big cores' other tenants,
not the eyes (`lamp-eyes` has no `CPUAffinity`). With `--endpoint
smart-turn-fast`, `lamp-turn-end` keeps itself and its two ONNX Runtime
threads on cores 6-7 (`keep_on_fast_cores`, `FAST_THREADS = 2`) at the
unit's `RR 20`. During a verdict (about 148 ms on lamp-4ace) both big
cores run RR 20 threads of the same priority, and a capture thread held to
6-7 cannot preempt them or move to a small core: it can wait up to an RR
slice (100 ms), longer than the 40 ms the microphone buffers
(`buffer_blocks: 4`), so it overruns just as the person stops talking.
Unpinned, the scheduler moves it to a small core. A single big core (`6`
or `7`) does not help, since the verdict holds both. `lamp-name`
(`CPUAffinity=6 7`) is what loses capture's ~7 % of a big core. So the
switch is not turned on beside `smart-turn-fast` until talking turns with
`LAMP_STALLS=on` show no held passes and no overruns; a quiet-room
off/on/off never runs a verdict and cannot show it.

A minute of what the harness itself does, concentrated (138 rounds of a
log-in, a 16 MiB log read, a Python start and an 8 MiB synced write to the
SD card), ended nothing at the default scheduling, so those two faults are
not explained by that load alone. They have not recurred since; that is a
few hours.

## Log lines never block or end the voice

Every process of the voice (`serve` and each `worker`), and since the same
day every lampOS part (actions, air, body, controls, ears, expression, eyes,
health, memory, name, policy, schedule, tasks: `let _log =
lamp_ipc::log_relay::guard();` as the first line of `main`), starts by
putting a pipe of its own on stderr (`crates/ipc/src/log_relay.rs`). A thread
drains that pipe at once and copies whole lines to the real stderr without
waiting on it. On Linux it sends to a socket (the journal's stream) with
`MSG_DONTWAIT`, and writes to a pipe (a worker's stderr, its parent's relay
pipe) through a file of its own on that pipe, opened write-only and
non-blocking from `/proc/self/fd`. To anything else it writes only when
`poll` says there is room, at most `PIPE_BUF` at a time: on Linux a
terminal, a file, or a pipe `/proc` cannot reopen, and everything on macOS.
Another writer that
takes a pipe's room between the two calls can still make it wait until the
pipe is read. It never sets `O_NONBLOCK` on the real stderr: that flag
belongs to the open file, and a worker's stderr is its parent's relay pipe,
so a worker's relay that set it made the coordinator's own stderr
non-blocking, and the coordinator's next `eprintln!` into a full pipe would
have panicked (the eighth on #1178). `tests/relay_shared.rs` runs a real
worker on a parent's pipe and fails if the worker's relay leaves that pipe
non-blocking. A worker's line over 4 KiB can reach its parent's pipe in
pieces, with another writer's line between them. When the journal does not take
them, up to 64 KiB is held, the oldest whole lines are dropped, and the
count is said once lines go through again ("N log lines dropped because the
journal did not take them"). A line already begun is always finished.
Before a process ends, and after a panic's message, it waits up to half a
second for its last lines to go. "Gone" means written or held: the relay
marks itself busy before each read, so an empty pipe whose bytes it has
taken but not yet written does not count as done (#837). Tests pin that
window (#886): `log_relay`'s `finish_waits_for_words_read_from_the_pipe_and_not_yet_written`
and `finish_waits_for_the_usage_line_held_after_the_error_line_went` stop the
relay, through a test-only hook, just after its read and just after the
error line, with the writer still open, and require `finish` not to say done
until it goes on, then the exact error and usage lines. Both fail without the
busy mark. `finish_is_bounded_by_a_full_journal_and_freed_by_a_broken_one`
keeps the bounds, and `tests/relay_exit.rs` checks 200 normal exits of a real
child for exactly its two lines. Run: `cargo test -p lamp-ipc` (23 passed on
2026-10-16; ARM64 run 38785245580). They show the window is closed, not that
it caused each earlier lost line. `std::process::exit`
runs no destructor, so a part that exits that way calls
`lamp_ipc::log_relay::finish` first (tasks, schedule and voice do): tasks
lost its `bad_request` refusal on the loaded ARM64 runner without it (#876).
A process killed outright (SIGKILL, or SIGTERM in a part with no handler
for it) loses what is still in the pipe. A test that kills a part and then
reads its lines must wait for those lines first: on 2026-10-15 the
`lamp-policy` program test killed the program at once and lost its last
lines on the slow ARM64 runner, which failed every builder's run for an
afternoon (#783; `Running::end` in `crates/policy/tests/it/program.rs` now
waits for them).

Why: `eprintln!` blocks while the journal does not read and panics when its
stream breaks. On lamp-4ace on 2026-10-15 a journald restart (01:52) ended
the ring worker (#744). At 09:00:01 the RAM log was full, journald could
not write, and the capture worker overran its read budget and ended the
voice. That second crash is the likeliest effect of a blocked stderr; it is
not proven.

## What stalls the voice: `LAMP_STALLS=on`

On lamp-4ace on 2026-10-17 the voice ended at 00:00:06, five seconds after
the lamp's daily log jobs began, on an "authority snapshot has expired"
(#1178). Each loop of the voice that holds an authority renews it against
a lease of 250 ms (`turn-owner`'s `MAX_AUTHORITY_LEASE_US`), and **100 ms
during a turn**, when `controller.rs` caps it at the capture and admission
leases. So some loop was held past that. Log lines are not it (above), and
the session trace is written by its own thread.

With `LAMP_STALLS=on` in the voice's environment (off unless set), the
coordinator's loop and the ring, speaker, capture, camera and provider
workers' loops each time their passes (`crates/voice/src/stalls.rs`). One
line is written, never on stderr, to `stalls-<role>.log` in
`/run/lampos-voice` (or `LAMP_STALLS_DIR`), for:

- a pass's own `work` of 50 ms or more, with how it ended: `end: "done"`
  at its normal end (before its wait: each `continue` that skips the rest
  of a pass calls `done()` first); `"skipped"` when it left early some
  other way and the loop went on; `"loop"` when the loop itself ended in
  it, on an error or a stop, which is the pass that ends a worker;
- a `gap` of 50 ms or more from the end of one pass's work to the start of
  the next: its wait overran. A slow pass is one `work` line, not also a gap.

Each line has `role`, `pid`, `kind`, `ms` and `at_us` (and `lost_before`,
the lines dropped since the last one, when any were), then:

- `thread`: this thread's major and minor page faults and voluntary and
  involuntary context switches over exactly that span
  (`getrusage(RUSAGE_THREAD)` at its two ends; Linux only, so on a
  builder's Mac they are zeros);
- `system`, read just after by the recorder: the lamp's major faults
  during the stall (`major_faults_during`, from the recorder's reading
  `read_ms_before` the stall began: it reads every 100 ms and keeps 10 s),
  its dirty and writeback pages, and its runnable and blocked processes
  (`/proc/vmstat`, `/proc/stat`; the lamps' kernel has no `/proc/pressure`).
  A recorder whose file cannot be opened says so once on stderr.

The loop itself only reads the clock and `getrusage` twice a pass, and at a
stall hands a few numbers to a thread of its own with `try_send` (a full
queue of 64 loses that one): the recorder reads `/proc`, builds the line and
writes it, so recording never lengthens the stall it measures. A file past
512 KiB starts over, the one before kept as `stalls-<role>.old.log`, so the
newest lines are always there. A loop's end never waits for its recorder,
so its own cleanup (closing the microphone, blanking the ring) comes first;
`main` gives the recorders up to 300 ms at its very end, after every device
is closed, before `log_relay::finish`, and the same while a panic unwinds
out of it (`stalls::Finishing`), so a pass that panicked is still written as
the loop's end.

To read it on the lamp, `lamp-voice stalls` (or `lamp-voice stalls DIR
--minutes N`; the voice's run folder and the last 15 minutes unless given)
prints each role's stalls, the role with the longest first: its five
longest, and every pass that ended its loop however short, each with how
long ago it began, its counters, the lamp's, and what held it: "memory"
(major faults of its own), "it waited in the kernel" (a `work` pass with
voluntary switches), "the CPU was taken from it" (involuntary switches),
"it ran that long on the CPU (its own work)" (a `work` pass with neither:
the loop's own code was the slow part), or "it woke late" (a `gap` with
neither); off Linux, "no counters". A role says how many of its passes
ended the loop, and how many lines the recorder's queue lost ("(N lost)").
A file that cannot be read is counted, and a byte in one that is not UTF-8
loses only its own line. `/run` is emptied at boot, so its lines are always
this boot's, and `at_us` is that boot's clock; read from a folder that
outlives a reboot, lines of two boots can mix, and then "ago" is wrong.

How to read it: major faults say memory (the voice's own pages read back
from the eMMC); involuntary switches with none say the CPU was taken;
voluntary switches in a `work` line say the loop waited in the kernel, as
the ring worker's SPI write does while the SD/MMC host re-tunes (#911).
Workers are started with a cleared environment, so the coordinator passes
`LAMP_STALLS` and `LAMP_STALLS_DIR` on to each.

The second route's worker writes `stalls-second.log` (#4332). Its loop is
one pass a 2 ms tick, and the stretch from `Ready` to its first tick is one
more pass, so a tick held up by another task on its single thread shows as
a `gap`. That alone does not say which task held it, so two more kinds of
line name it, each by its `task`:

- `poll`: one poll of 50 ms or more of a task on that thread. The worker's
  startup tasks (`routes`, `air`, `seems`, `texts`, `face_refresh`) and
  the engine's own (`cues`, `down_line`, `warm`) are wrapped in
  `stalls::Timed`;
- `call`: a call made inline that took 50 ms or more: those between
  `Ready` and the first tick (`ready`, `render_lines`, `link_watch`,
  `link_ready`), `warm_ahead`, and the texts' bus setup (`texts.bus`,
  `texts.subscribe`, `texts.publisher`).

`lamp-voice stalls` shows a line's task after its kind. With the switch
off, a wrapped poll is one branch and reads no clock.


## What each capture stage costs: `LAMP_CAPTURE_STAGES=on`

On lamp-4ace on 18 October `worker capture` took 18 % of a core with nobody
there, while its code benched flat out sums to 2–6 % at the lamp's scale
(#705, 09:39). With `LAMP_CAPTURE_STAGES=on` (off unless set; the
coordinator passes it to the capture worker only), that worker says once a
minute on stderr (the journal) what each stage of its passes cost its own
thread (`crates/voice/src/stage_cost.rs`, `CLOCK_THREAD_CPUTIME_ID`, so time
it waited or was preempted is not counted). The line's shape (not measured
numbers):

```
lamp-voice: capture: thread CPU P % on CPU C over the last 60 s, N passes, B blocks: read X ms (Y µs each of N), reference …, canceller …, detector …, encode …, rest of pass …, wait … (LAMP_CAPTURE_STAGES=on)
```

- `read`: the microphone's `try_read`, timed on every pass while it runs.
- `reference`: each reference packet accepted and each block's step. Both
  render into the canceller and the echo evidence's observer.
- `canceller`: all of `EchoProcessor::capture`. That is AEC3, and also the
  echo evidence's search (the work `LAMP_QUIET_EVIDENCE` skips) and the
  steady reference, so a large `canceller` is not AEC3 alone.
- `detector`: the speech detector; `encode`: the frame's encode and send.
- `rest of pass`: the rest of each pass (control, sockets, authority).
- `wait`: from one pass's end to the next's start: the sleep's own CPU,
  and with `LAMP_STALLS=on` that diagnostic's own `getrusage` pair and two
  clock reads a pass, and once a minute this line itself.

Two switched paths, both off in the product unit: the stop words' raw
stream (`--raw-blocks`, #1353) copies each block in `rest of pass` and is
serialized in `encode`; the linear speech-detector input
(`LAMP_VAD_INPUT=linear`, #1416) is copied inside `echo.capture`, so in
`canceller`, and chosen in `detector`. `C` is the CPU the thread is on as
the line is said (`sched_getcpu`, `-` off Linux): on lamp-4ace the small
cores 0–5 and the big cores 6–7 differ about 2.7 times, so a minute is read
with it. Each call's cost is printed to 0.1 µs.

The share is the thread's whole CPU time against the wall clock. Read it
beside each core's `scaling_cur_freq` and the thermal zones: a stage that
costs more than its bench at the same code points at the core or the clock
the worker gets. Its own cost: four clock reads a pass, eight a block and
two a reference packet, about 3,000 a second, each a system call on Linux,
and each stage's time carries about one of them (it matters for `read`, a
few µs a call). On lamp-4ace on 18 October (the first builder, #1454, at
`7b634c687`, when the line still came with `LAMP_STALLS=on`; nobody there,
`worker capture` held on one core type with `taskset`) that cost was not
measurable: the worker read 17.1–17.2 % of a core on the small cores and
6.3 % on the big ones with the line, against 17.0–17.4 % and 6.5–7.2 %
without it (`pidstat`, 09:54–09:59). Its split, per 10 ms block, small core
then big: canceller 732–736 and 208–211 µs, detector 327–330 and 101–103 µs,
read 180 and 80 µs, rest of pass 205 and 100 µs, wait 75 and 45 µs (each
over the block's five passes), encode 92–93 and 43 µs, reference 98 and
40 µs. It is a diagnostic, in no unit.

## A quieter pace while nothing happens: `LAMP_QUIET_PACE=on`

The coordinator's loop passes every 2 ms whatever is happening. On lamp-4ace
on 2026-10-17 the quiet voice took half a core, and the chip ran past the
90 °C where it slows itself (#705, 08:37). With `LAMP_QUIET_PACE=on` (off
unless set, `crates/voice/src/quiet_pace.rs`) the coordinator passes every
5 ms instead while nothing is going on: no reply, nothing queued for the
provider, no admitted speech, nothing draining, the room not busy, and no
block the detector called speech in the last 2 s. The first pass after any
of those ends is at 2 ms again. What it costs is up to 3 ms more before a
pass sees the first block of speech; authority is still renewed every 20 ms,
far inside every lease. The workers' own loops keep their pace for now.

The capture worker's echo evidence (`crates/audio/src/evidence.rs`) works
on every 10 ms block while the lamp says nothing: it searches up to 32
alignments of the speaker's sound against the microphone on a block louder
than -60 dBFS (a fan, an office), and, once the lamp has spoken since the
last canceller reset, it fits the held alignment on every block, in a room
of any level. With `LAMP_QUIET_EVIDENCE=on` (off unless set, passed on to
the capture worker only) neither is run while every sample of the speaker's
sound they would read, the last 520 ms, is zero. They could find nothing
then: the evidence is the same block for block (no echo, the held
alignment's age, the search's place moved on), which a test runs both ways
to show. With the switch on, capture says once a minute in the journal how
often it applied: `the echo search was skipped on 5700 of 6000 blocks in the
last 60 s, 4100 of them loud or with an alignment held`. Only the second
number saved work: a quiet room with nothing held is skipped, but would not
have been searched. Measured on a laptop (`measure_a_silent_render_block`), the
evidence took 12-15 us a block in a -26 dBFS room, with or without an
alignment held, and 4.4 us in a quiet room with one held; 1.4-1.8 us with
the skip.

On lamp-4ace on 2026-10-17 capture rose from 7 % to 17-19 % of a core
(#705, 11:46). What caused that is not known yet. By these numbers the
evidence is at most a part of it; the A/B with the switch on lamp-4ace will
say how much. The canceller itself is not it either: on lamp-75c9 it took
203 us a block (p50) with the lamp's echo playing and 124 us in the silence
after it (`lamp-audio bench-aec-fixture`). A laptop is no guide to that
second figure: on x86 the canceller's filters decay into subnormal floats in
the silence after speech and its cost rises tenfold (705 us), which the
lamp's ARM cores do not pay.

With `LAMP_VAD_INPUT=linear` (off unless set, passed on to the capture
worker only, and only that exact value) the local speech detector reads the
canceller's linear output in place of the processed block (#828): the
residual echo suppressor after the canceller can take a person's "Stop."
said over the lamp's own voice down with the echo, and the linear output
keeps it ([microphone-experiments.md](microphone-experiments.md),
`--linear-output`). It also keeps most of the lamp's own voice: on lamp-4ace's
mic2 the linear stage took 3.1–4.1 dB of it out, and a person 10 dB under the
echo stayed 4–7 dB under (#828, 17 October 08:01). So with the switch the
detector may score the lamp's own speech as speech for most of its talk,
opening the "no speech" gate that held most of the stop lines' blocks, and
leaving the explained share and the floor (`audio/src/duplex.rs`) alone
against false yields. Those two are what the duplex turn-taking (every unit's
`--turn-taking duplex`) applies to each speech-like block while the lamp
sounds: a block the render explains more than 75 % of is the lamp's own
(`Heard::Own`, no yield, a score of 0 to the turn's start). In the 18 October
mic2 rehearsal (#828, 07:40, played lines) every lamp-alone block the linear
output scored as speech was 93 % explained or more, so own, while the stop
blocks were 94 % at the median and 0 % at the p10: the share alone says the
lamp is not heard as a person, and the open question is whether a stop gives
the seven `Over` blocks in a row that yield. `hearing_replay REPLAY_DIR
--signal aec_linear [--explained N]` scores the linear output through those
rules, `over_run` counting the blocks in a row against `onset_needed` (10
in the reply's first 1.2 s, 7 after) and `step` giving what `Interruption`
does with each block, so a replay shows both the
lamp-alone false yields and the stops caught, at today's 75 % or another
share. The replay that decides it reads both. Capture says at start that
the detector reads the linear output, and once a minute on how many blocks
it did (for example `the speech detector read the linear output on 5990 of
6000 blocks in the last 60 s`, not a measured figure): 0 would mean the switch
did not take. Only the detector's input changes: the provider's audio, the
residual and the evidence keep the processed block. The score's readers all
get the new score: the turn's start and end, the re-centre's quiet test
(`reference/recovery.rs`, `note_score`), the frame's probability handed to the
turn detector, and the recordings' `vad_score`. The linear output comes
before noise suppression, which every unit runs off (`--noise-suppression
off`), so that part changes nothing today. The canceller keeps its
linear output only with the switch, and a block with none (no reference
yet) is read as before. `activity::tests::the_detector_hears_the_linear_output_only_with_the_switch`
scores a generated "Stop." (Piper `en_US-libritts-high`) with the processed block silenced: speech with
the switch, silence without it.

With `LAMP_VAD_INPUT=linear-gated` (the second exact value, passed on the
same way) the detector reads the linear output only on a block where speech
would plainly be a person: the room clear of the lamp (`Heard::Clear`), or
someone over it that the render explains no more than `plain_percent` (20 %)
of, with a fresh alignment (`Hearing::plain`), judged by `Hearing::EVALUATED`
on the block's own trusted evidence at a score of 1
(`activity::linear_admitted`). Everywhere else (the lamp's own voice, a block
over the lamp but 20 to 75 % explained, one not yet judged, untrusted
evidence) it reads the processed block, as without the switch.

Why 20 % and not the duplex rules' 75 %: a yield is a run of `Over` blocks,
and a block 20 to 75 % explained can be `Over` on either score, so a gate at
75 % gives every block a yield can come from the linear score and with it
`linear`'s false yields (the reviewer on #1504); the lamp's own voice
misjudged as a person was 30 to 75 % explained on lamp-4ace
(`duplex.rs`, `plain`). On the first builder's `hearing_replay` tables of
18 October (#828; 14 October's cut-a, cut-b, cut-d and both-a, and this
morning's mic2 stop rehearsal: 2.6 minutes of the lamp alone, 19 lines said
over it; the gate worked out from each block's `aec_only` and `aec_linear`
rows, the detector's state across a switch not modelled):

| detector input | false yields over the lamp alone | lines over the lamp yielded |
|---|---|---|
| processed (`clean`) | 1 (0.4 a minute) | 10 of 19 |
| `linear` | 13 (5.1 a minute) | 14 of 19 |
| gated at 75 % | 13 | 14 of 19 |
| gated at 50 % | 4 | 12 of 19 |
| gated at 20 % (`linear-gated`), worked out per block | 1 | 11 of 19 |
| **`linear-gated` replayed (`--signal aec_linear_gated`, one detector)** | **2 (0.8 a minute)** | **10 of 19** |

The last row is the figure to quote: one detector over the switched stream,
as the lamp runs it (`hearing_replay` built after #1504; the first builder,
18 October 15:26), counted from the replay's own `Yield` steps, on which
count the processed block gives 1 and 10 of 19 and `linear` 12 and 14 of 19. It loses the one line the per-block mix gained, and gives
one false yield more than the processed block (both-a). So on these played
sets the switch is today's catch with about today's false yields, not
`linear`'s 12; it is the third side of Monday's real "Stop.", which these sets
cannot stand in for (a real voice, its level, the owner's room). For the
score's other readers (the busy room, the re-centre's quiet test, the frame's
`vad`, a turn start) the lamp alone is the processed block's score too, but
for blocks the gate opens. The gate's thresholds are the duplex rules' own;
it adds none. With `LAMP_LISTEN_GAPS=on` the coordinator's `Hearing` takes a
gap of the lamp's silence as `Over` (`silence_heard`), but the gate uses
`Hearing::EVALUATED`, so such a gap stays on the processed block.
`hearing_replay REPLAY_DIR --signal aec_linear_gated` scores a replay this
way, from its AEC-only and linear outputs and the AEC-only evidence, gated by
the replay's own `Hearing` (so `--explained` moves the judge and the gate
together; the gate itself is `plain_percent`, so only an `--explained` under
20 % moves it). Two ways it differs from the device: a recording keeps no
reference trust, so the replay gates on every block's evidence where the
device takes only trusted evidence and reads the processed block over an
untrusted span; and on the device the judge and the gate stay at
`Hearing::EVALUATED` whatever `--explained` a replay uses. The table above
was worked out per block from separate `aec_only` and `aec_linear` scores;
`--signal aec_linear_gated` runs one detector over the switched stream, as
the device does, and is the figure to quote (Thirteen and the reviewer on
#1504: in a synthetic probe the two differed by a yield in one pattern of
several). Tests:
`activity::tests::linear_gated_reads_the_linear_output_only_where_speech_would_be_a_person`
and `linear_gated_keeps_the_lamps_own_voice_out_and_a_stop_over_it_in` (the
generated "Stop." kept in the linear output: at 95 % and at 50 % explained
it scores as speech with `linear` and as silence with `linear-gated`; at 10 %,
or with the room clear, block for block what `linear` gives).

## The capture worker waits instead of ticking: `LAMP_CAPTURE_WAIT=on`

The capture worker's loop passes every 2 ms (`audio_workers.rs`, `TICK`),
five passes to a 10 ms block, four of them with nothing to read; on a small
core those passes cost 460 us a block (#1454). With `LAMP_CAPTURE_WAIT=on`
(off unless set, passed on to the capture worker only, which says so once
in the journal at start) the end of a pass is a `poll` instead of the tick's sleep, on:

- the microphone's ALSA descriptors (`snd_pcm_poll_descriptors`, their
  `revents` read through `snd_pcm_poll_descriptors_revents`;
  `CaptureDevice::wait` in `crates/audio`);
- the coordinator's control socket, which carries the stop and the
  authority, and the speaker's reference control and render sockets
  (`impl AsFd for Channel`, `transport.rs`). The capture worker has no
  other stop: a stop is `Control::Stop` on that socket;
- at most 20 ms from the pass's start (`WAIT_AT_MOST`), so a wake that was
  missed costs one wait, and the authority, the partial block's deadline
  and the channels' ages are still looked at that often.

With the switch, each wait first sets the microphone's `avail_min` to the
samples the block being assembled still needs (a whole block after a whole
one was read; set again only when that changes), so its descriptor wakes
the wait as the block arrives, and the microphone is polled for the whole
wait, beside the sockets. A block is read as it arrives whenever the
speaker's render comes: some microphones hand over whole blocks (the rig's,
and the lamp's `plug:device_micro2`, whose rate plugin converts the Jieli's
48 kHz a period at a time), and the speaker renders on its own clock, so
a wait timed from the read would read each block when the next render came
(#1498's review). A descriptor that wakes with no samples (a plugin's)
costs a 2 ms tick before it is polled again, never a busy loop. Off,
`avail_min` stays 1. A block already in the device
(its status after the last read said so) is read at once, with no wait:
nothing is read ahead of the loop, so there is nothing else a `poll` could
miss. Nothing is read in the wait: each pass reads controls, reference and
then at most one block, in the old order, so no block is lost or reordered
and the overrun handling and the reference path are the same. The loop
waits only while the microphone runs with nothing due on time alone: before
its start, through a stall's gap and while a prime awaits its start it
ticks as before. Off, the loop ends every pass in the old tick.

On Linux x86_64, native, 20 cores (run by Nine on this head:
`capture_wait_cost` in `tests/capture_hold_worker.rs`, the real loop over
a scripted microphone that holds a whole block at once, 10 s each, release
build). On a core this fast the share of a core is at the noise floor both
ways; lamp-4ace's `pidstat` is what counts for it. The rig sends
the speaker's render beside each block, so these numbers hold only when
the render comes with each block. With the render 5 ms after each block,
`with_the_render_5_ms_after_each_block_all_but_a_few_blocks_are_read_within_a_ms`
(the same file) holds all but a few reads within 1 ms of the block, at most
5 of 200 slower when the runner takes the capture thread off its core (#1569
row 41): the wait follows the microphone, not the render:

| | wakeups/s | capture thread | block read after it was there | frame sent after it was there |
|---|---|---|---|---|
| tick, quiet | 484 | 2.43 % of a core | 1,049 us | 1,167 us |
| wait, quiet | 124 | 2.84 % | 87 us | 266 us |
| tick, talking | 484 | 3.03 % | 1,070 us | 1,243 us |
| wait, talking | 124 | 2.53 % | 69 us | 257 us |

The 124 wakes a second are the speaker's render (one a block, sent beside
it) and the coordinator's authority (every 40 ms in the rig). How the
lamp's own microphone and speaker clocks fall, only a lamp shows.

On lamp-4ace the first builder measures it as for #1459: the same build
with and without `LAMP_CAPTURE_WAIT=on` in the drop-in, `LAMP_STALLS=on` in
both; `pidstat -t -w -p $(pgrep -f 'lamp-voice worker capture') 1 60` for the
capture thread's share of a core and its `cswch/s` with nobody talking, then
four talking turns, each answered, with no stall line and no lapsed lease
in `/run/lampos-voice/stalls-capture.log` or the journal.

## Stop and current limits

After entering the coordinator loop, SIGTERM/SIGINT records a service-stop request,
cancels any unfinished reply as `service_stopped`, revokes camera/microphone
authority and requests Stop from all workers before waiting. Worker failure or a
required forced kill remains failure. Only after worker cleanup does the optional
logger receive up to 500 ms for final acknowledgement. A timed-out writer is
detached; its eventual file update cannot revise that returned timeout. Process
exit reclaims the remaining thread. A signal during incomplete startup retains
the existing explicit startup failure path.

Cleanup retains failures by stage and worker instead of letting a later failure
replace an earlier one. Distinct causes are returned together in first-seen
order: a forced speaker followed by a forced capture or an endpoint timeout
names each. Repeated failures from the same stage and worker keep their first
detail and a repeat count. Production collection is bounded to 128 stage/worker slots and
1,024 UTF-8 bytes per stored error message; existing trace events remain
unchanged.
An ordinary single error retains its original kind and message.

Both audio certifications still run and emit `audio_diagnostics_certified`.
If both fail, the result includes both the speaker's and capture's certification
errors. An unsolicited receipt is added to any earlier cleanup causes, rather
than replacing them. A certification failure on its own still fails the run.
Collecting errors does not skip cleanup work or change worker stop deadlines,
microphone revocation, forced termination or teardown success criteria. The
host regressions for #1620 cover one or both audio workers finishing after
900 ms against the 750 ms diagnostic grace; they do not establish why a
worker was late on a lamp.

At the shared shutdown deadline, the coordinator checks each remaining child's
exit status again before forcing it to stop: an earlier child's native reap may
have taken time while a later child exited on its own. An observed normal exit
does not erase an earlier cleanup failure; a nonzero exit still fails cleanup
and is recorded as `worker_exit_failed`, not `worker_forced_shutdown`. The exit
check does not extend the deadline or bypass the worker's resource cleanup.
For the ring, a `worker_exit_failed` at the stop says `exited_before_stop`:
true when the ring had already ended and been disabled mid-session, so its
cached exit is not mistaken for one the stop caused
([ring-choreography.md](ring-choreography.md), #1620).

With `LAMP_VOICE_ROUTE=second` the second route's worker is `rig.provider`
under the role `second`, and goes through this same stop: Stop, deadline,
recheck, kill, reap, and its causes under `second`. Its speech to text, chat
and voice are tasks in that one process. Its runtime is ended without waiting
for a blocking tool call or DNS lookup (`worker::finish`), which a plain drop
waited for, up to 1.5 s for the executive, past this deadline
([second-voice.md](second-voice.md), "Second only").

Serve waits with only its GPIO privacy worker while physically muted. A fresh
active mute closes the full runtime and returns to that wait only after verified
cleanup; stable unmute then creates new capture/provider identities. Unknown or
stale privacy and worker/provider faults remain errors. There is no claim that
reopening preserves Gemini context. See [the precise restart and bounded cleanup
contract](service-privacy.md).

Host regressions cover simulated progression beyond 600 seconds without a session
expiry, bounded rotation and stalled/failing writers, cancellation/privacy while
metadata is dropped, immutable old-owner settlement after history rotation,
append-only ring observations, batch cue continuity and orderly synthetic worker
shutdown. macOS mock channels are retained through the final reap because peer
socket teardown differs from Linux; actual Linux worker socket shutdown, sustained
on-device operation, resource use and acoustic behavior remain native checks.
