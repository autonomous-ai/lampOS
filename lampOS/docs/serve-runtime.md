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
afternoon (#783; `Running::end` in `crates/policy/tests/program.rs` now
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
against false yields. The replay that decides it reads the `vad` share over
the lamp-alone seconds as well as over the stop. Capture says at start that
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

## Stop and current limits

After entering the coordinator loop, SIGTERM/SIGINT records a service-stop request,
cancels any unfinished reply as `service_stopped`, revokes camera/microphone
authority and requests Stop from all workers before waiting. Worker failure or a
required forced kill remains failure. Only after worker cleanup does the optional
logger receive up to 500 ms for final acknowledgement. A timed-out writer is
detached; its eventual file update cannot revise that returned timeout. Process
exit reclaims the remaining thread. A signal during incomplete startup retains
the existing explicit startup failure path.

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
