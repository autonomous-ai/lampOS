# Supervised physical mute in serve

With `LAMP_PRIVACY_CONTROLS` unset, `serve` begins with only the GPIO privacy worker. It sends denied microphone and
camera snapshots to maintain that worker's existing parent heartbeat. No capture,
speaker, camera, ring or provider worker is created during this wait. Missing,
malformed, out-of-order, future or stale switch observations are faults, not user
mute. The existing 50 ms freshness limit and 60 ms stable-unmute requirement are
unchanged. Fresh muted readings may continue indefinitely without creating a
conversation readiness cue.

With `LAMP_PRIVACY_CONTROLS=on`, the selected privacy worker instead consumes
the installed Controls privacy observations and publishes the typed Voice-only
`voice/privacy` lease. This is an explicit experiment; unset retains the current
direct-GPIO path. Permission still starts denied, and neither a Controls
observation nor an indication can grant it independently of Voice. With
`LAMP_PRIVACY_CONTROLS=no-slide` the same worker (told by `--no-slide`) takes
each fresh Controls publication saying the lamp has no slide as an open
observation, so the software mute alone closes the microphones; `serve`
refuses to start without `--controls-mute`, and the wait never starts the
muted indicator ([privacy-switch.md](privacy-switch.md#a-lamp-with-no-slide-no-slide)).

During that mode's quiescent wait, a separate optional ring process starts only
after a validated muted observation and may show a dark-red physical-mute
breath. It reads the shared validated privacy lease,
matches the current controller boot and microphone generation, and requires
fresh denied microphone/camera authority with no owner and an original fresh
physical LOW observation. Software mute alone does not authorize red. Its only
nonblack output is the fixed four-second red 2–10 breath; normal conversational
and resting-frame permissions remain closed. Missing, unknown, stale or open
state blanks it. The worker drains control before and after bounded bus reads;
all SPI writes remain outside the sampler and service wait loop.

The active ring must have completed its existing verified cleanup before the
indicator can start. Before the wait returns for unmute, the indicator must
return a shutdown black-write receipt and exit successfully, releasing its
exclusive SPI lock. A requested Stop or successful black write alone does not
release ownership. Optional indicator failure never grants audio permission;
unverified destruction cannot be followed by another ring writer. A writer that
started and then failed, at startup or later, must still meet that verified
black-write and exit contract; its failure stays an error even after reaping and
never turns into a successful unmute. A startup failure before any writer
process exists (a bind or spawn failure) opened no SPI and owes no black write:
it is logged once, the wait goes on without the light, and it does not block a
fresh stable unmute. An optional light never ends a healthy voice. See the
[ring indication contract](ring-choreography.md#physical-mute-indication-755).

After stable fresh unmute, the privacy-only worker must cooperatively stop and
exit successfully. Only then does the ordinary coordinator create a fresh full
runtime. That runtime independently checks new privacy observations before its
existing microphone/input readiness path; the outer observation is not a permit
or a promise that the switch stayed open during startup.

During an active serve runtime, a current valid physical mute ends the current
owners. An unfinished reply is cancelled as `privacy_muted`; it is not reported as
completed. Microphone/camera permissions close, all workers receive Stop before
any final wait. That exact fresh-mute outcome with successful cleanup may
return to the GPIO-only wait. Optional camera/ring failures disable that capability
during the active conversation without cancelling healthy audio. Final cleanup
still verifies their exit and required local-revocation, teardown or black-write
receipts. A missing receipt, failed exit or forced termination blocks reopening;
contained presentation failure alone is not permission to skip cleanup. Lost
freshness, unknown status, mandatory worker/provider faults and invalid messages
remain service errors on the direct GPIO route. On the Controls route
(`LAMP_PRIVACY_CONTROLS=on`, [privacy-switch.md](privacy-switch.md)) a switch
that cannot be read (no source, a fault, stale or broken evidence, its worker
ended) is closed instead: the cycle ends as a fresh mute does, with the same
verified cleanup, and the wait reads the switch again each second with a fresh
worker, so the voice stays up with its microphones shut and never crash-loops
(#795; lamp-4ace, 2026-10-16). That includes a worker that ends cleanly
(status 0) before the wait has read its start, as one does at once while
Controls restarts under it: a clean end is the worker's own
(`Worker::ended_cleanly`), so the wait counts it as an unreadable switch and
reads again, where it once ended the voice and rolled back lamp-0c89's
update (#1396, 21 Oct 13:13: the voice switched 0.76 s before Controls).
The conversation's own privacy worker counts the same if it ends cleanly at
its start (`privacy_started`, Seventh on #1396): the conversation ends closed
and the wait reads the switch again. A worker that crashed (a panic, a
signal, any other status) and any other start failure end it as before
(the reviewer-lead on #2506). The same holds when the conversation first
finds it gone by its refused socket, as it sends the worker its authority
(`authority_refused`): a worker that ended cleanly, waited for at most
100 ms, is the switch unreadable, not an exit (lamp-75c9, 21 Oct 22:32:43,
the first builder on #2602: the session's new worker had ended on
`physical privacy source expired` before its first authority, and
`privacy.c.p: Connection refused` ended the voice). Each read again tells the wait the switch
reads unavailable, so after 1 s the health says the microphones are off
until the privacy switch can be read, while every fresh worker ends at its
start as well as when one ends running (the first builder on #2506). The worker's own `privacy switch unreadable:`
fault, queued before it ended, and a message of its aged out after a clean
exit are part of that ending, not a failed cleanup; its exit status is still
checked. A failed cleanup still ends it. One window is left: in the muted
wait, if the wait notices the worker's end more than about 10 ms after the
muted-ring writer loses its lease, that writer blanks and exits with a fault,
and the wait ends as a failed cleanup. A previous runtime
error cannot hide a shutdown failure. The combined error retains both causes
and does not expose a restartable physical- or software-mute token.

**A short gap ridden inside a conversation** (`LAMP_PRIVACY_GAP_RIDE=on`,
off unless set, Controls route only; #2602, the first builder's (B), lamp-75c9
on 21 Oct 16:33:06). A lapse of the switch's freshness after the microphone was
allowed no longer ends the conversation at once (`privacy_gap.rs`):
- **The worker's own expiry:** the conversation passes `--gap-ride` to its
  Controls worker because that child's environment is cleared. After the
  worker has published an allowed capture lease, a missing fresh acquisition
  sends `PrivacyUnavailable { since_us }`, closes the worker's `voice/privacy`
  lease, and keeps reading in the same worker. `since_us` is the last accepted
  acquisition plus the unchanged 50 ms freshness. The coordinator treats it
  as unavailable even if recovery is queued in the same batch, so the cut and
  hold below still happen. The muted wait and direct GPIO route are unchanged.
- **From the lapse:** no capture frame is taken, including those capture still
  reads inside its lease (`GapRide::takes_frame`). The turn in flight is cut
  (`privacy_gap`) and the speaker is told at once; nothing new begins (it holds
  as the keeper's pause does), so the lamp says nothing. The capture's and the
  camera's authority aren't renewed (`Rig::capture_held`, in both `publish` and
  `publish_hand_revocation`), so their leases lapse: within 250 ms
  (`MAX_AUTHORITY_LEASE_US`), about 100 ms while a turn is under way. Capture
  then closes the microphone as on a stall (`Discontinuity::AuthorityExpired`).
  The eyes are told no at the lapse and hear no yes until the reopen. The
  microphone permission and its generation are untouched: capture keeps one
  generation for its life.
- **The camera:** a camera worker given no authority for 250 ms
  (`WORKER_INITIAL_AUTHORITY_US`) ends itself, so a gap longer than that costs
  the camera for the rest of the conversation (`camera_unavailable`); the voice
  goes on.
- **A fresh reading that says open:** once steady for the gate's 60 ms
  (`REOPEN_STABLE_US`), as the first grant needs, within 1 s (`RIDE_US`), the
  authority is renewed. Capture reopens in the same generation with a new
  epoch, no frame read before the reopen is taken, the microphone's retention
  heartbeat (1.5 s) counts from the reopen, and the conversation goes on with
  its history.
- **A fresh reading that says closed** is the physical mute, as before.
- **No stable recovery within 1 s:** the conversation ends as before, and the
  wait reads again. The worker and coordinator both enforce the absolute
  deadline; delayed delivery, isolated fresh readings, another lapse during
  stabilization, or recovery at the deadline never renew it. That's before
  capture's own 1.5 s (`REOPEN_US`). During this bounded episode only, the bus's
  late-publication notice (after 200 ms) is tolerated, including one queued
  with recovery. A gone/replaced publisher, lost sequence, malformed evidence
  or fresh physical LOW keeps its existing fail-closed behavior.
- **What it says:** the trace's `privacy_gap` (`began`, `reopened`, or `ended`
  on every privacy exit while one is ridden, with `why`: `no fresh reading`,
  `read closed` or `privacy worker failed`; each with `lasted_us`) and one
  journal line at each. Off, every lapse ends the conversation as it always has.
- **No recording of the lease's tail:** the ride runs only in serve, which
  refuses raw audio diagnostics, so no local recording holds what capture read
  after the lapse.

The worker-expiry regression comes from Eighth's lamp-75c9 hour (21 Oct,
22:00:29): `physical privacy source expired` ended the conversation before the
ride saw unavailable. The stepped coordinator test now uses the production
worker expiry rule: a 60 ms publication silence emits `began`/`reopened` in one
conversation, with no renewed capture authority or speaker chunk in the gap;
a later silence outlasting the bound ends it. Linux tests exercise the actual
worker, local bus, closed lease, recovery and timeout. These use synthetic
switch evidence and no audio hardware; a short gap does not itself prove
capture's descriptor closed before its authority lease expired. Device
latency and the live conversation check remain with the lamp's owner.

With the independent `--controls-mute ABS_PRIVATE_RECORD` experiment, the
software latch is restored before any worker, including the metadata writer.
A restored software mute on the default direct-GPIO path waits with no
GPIO/audio/provider workers until a fresh explicit local hold has been durably
accepted as clear. A mute whose reason is `sleep` (a hand held on the head,
`LAMP_HEAD_TOUCH=on`, #711) is also cleared by a fresh head tap, through the
same durable clear; a mute without that reason never is
([controls.md](controls.md)). The selected Controls privacy path keeps its
observation worker during that wait and can show a verified physical mute; it still creates
no audio/provider workers and cannot clear the software latch. If software mute
arrives while the GPIO worker is already waiting, that worker continues to
refresh its observations while the software gate inhibits reopening. Clearing
software mute never replaces the physical checks above; a closed slide stays
closed. The per-iteration callback runs before physical freshness is checked,
so a slow durable write cannot grant input using an observation from before it.

An active software mute revokes local ownership and microphone/camera authority,
tells the speaker to discard its held output, and yields a distinct
`SoftwareMute` outcome. Successful full cleanup lets the supervisor return to
the combined wait. A failure in startup or cleanup stays terminal. One optional
bounded controls pump survives these cycles; it never opens an audio device.
Accepted head pats remain metadata reports during the quiet wait as well as an
active conversation; they do not clear either privacy gate or request speech.
A conservative muted recovery record is saved before audio opens, and is
disarmed only after verified clean exit, or after a stop the service was
asked for (SIGTERM or SIGINT, as for an update or a restart) whose cleanup
reported a fault (#960): the latch is then written as it was, muted only if
it was muted. Crash/fault recovery consequently requires another explicit
hold even if no mute was acknowledged before the fault; a requested stop does
not. With `LAMP_RESTORED_MUTE=on` the voice also knows when its mute was
restored by that guard rather than chosen (trace `mute_restored_after_fault`),
and with `LAMP_HEAD_TOUCH=on` as well a fresh head tap clears such a mute, as
it wakes sleep; a person's own mute still needs their hold. See [controls](controls.md#voice-and-physical-privacy) for the switches,
durability protocol, clear fences and physical acceptance procedure.

Unmute creates fresh controller/worker/provider identities. Old PCM, prospective
input, request IDs, permits and queued output are not replayed. A line
kept to be sent again after a lost connection (`LAMP_RESEND_LOST_LINE=on`,
#705) is tied to its privacy generation: a mute or a privacy change drops it,
and it is never sent across one. With that switch on, two more things are
true (the reviewer on #1434):
- **Each line's audio is held** in the provider worker's memory, up to 15 s
  (480 KB), from its Start until its turn ends, its answer comes, a newer line
  begins, or a mute; with the switch off nothing is held.
- **A lost line's audio goes to the service a second time**, whoever it was
  said to. A line lost with its connection has no attention verdict yet (that
  comes at a reply's first output), so the lamp cannot tell room talk from a
  line for it; the model judges the second copy as it judged the first, and
  room talk is answered with silence as before. A line that went with the line
  before it or the eyes' picture, had any output or tool call, or came before
  a mute, is never sent again; a note told beside it is told again with it.
  No local "spoken to the lamp" test is added: the line already went to the
  same service once, in the same conversation, so a resend shows it nothing
  new, and such a test would miss the follow-ups said without the name that
  are lost most often (the first builder on #705, 18 Oct 09:4x). No preservation
of Gemini conversational memory across physical mute is claimed. There is no
periodic context reset or timer-based reopen. A later stable unmute must satisfy
the new runtime's ordinary readiness checks again.

**Which service hears a line.** By default each admitted line's audio goes
to Gemini Live (through the company relay on the lamps); with
`LAMP_VOICE_SECOND_CONFIG` the second route hears a line only when it takes
one over ([second-voice.md](second-voice.md)). With `LAMP_VOICE_ROUTE=second`
it hears every one: on every turn, each admitted line's audio goes to speech
to text (the relay's `/ws/audio/transcriptions`, or Deepgram directly with
`deepgram_credential_file`), its words to the text model (the relay's chat,
or with `LAMP_SECOND_CHAT` Google's Gemini API, OpenRouter, and the provider
it routes to, or DeepSeek's own API, whose data is processed in China), a
look's picture with them unless the model takes none, and the answer's text
to speech (ElevenLabs or OpenAI's voice, or Google's with
`LAMP_SECOND_SPEECH=gemini`); with `LAMP_SECOND_SEARCH` the model's queries
go to DeepSeek's search, OpenRouter's (Exa), or the service `search_url`
names, and a page it asks to read to DeepSeek's fetch. With `LAMP_MEMORY_OBSERVE=on`
(#1693 (2), off unless set), at the end of each conversation its exchanges'
text (what was heard and said, up to 32 KiB, with their ids and earlier
observations; no audio or picture) go once more to the same DeepSeek model,
on the relay or DeepSeek's own API as the route has it, for the memory's
observation pass ([memory.md](memory.md)). A mute or a privacy change ends a line there
too: an authority with no owner retires the request, which closes its
speech-to-text socket (the reviewer on #1686).

One service invocation owns one signal registration and one bounded rotating
metadata writer. Each GPIO-only/full runtime uses a fresh private IPC directory;
a successful cycle explicitly removes the now-empty directory after all workers
and local endpoint bindings have ended. Unexpected remaining paths refuse the
next cycle rather than being recursively removed or accumulating indefinitely.
No per-cycle recording/output directory is created. The typed receipt ledger is
cleared only after verified destruction of the old runtime. Cue sequence numbers
continue; each runtime has a new boot and cleared capture/reference context.
A latched cue fault stays latched. Consumers must distinguish boots; old finite
cue consumers do not become multi-cycle service clients by assumption.

SIGTERM/SIGINT during the established mute wait requests cooperative Stop and
reaps the privacy worker. Audio/provider startup is not attempted. The wait loop
polls at 2 ms and publishes its denied heartbeat at most every 25 ms; these are
software scheduling targets, not measured GPIO or acoustic response bounds.
The selected wait gives both workers one shared 300 ms cleanup grace, including
a final receipt drain after the ring's exit and rejection of duplicate shutdown
receipts. Direct-GPIO privacy-worker shutdown retains its existing 300 ms grace; full-runtime cleanup
uses the existing shared shutdown bound. Native kill/wait cannot promise a hard
userspace deadline, and any forced cleanup remains failure. A signal or failure
during incomplete worker startup stays an explicit startup failure rather than
claiming a verified graceful mute lifecycle.

The regressions use actual local Unix datagrams and synthetic child processes,
including repeated verified cycles, stale/unknown input, active owned-reply
cancellation, cleanup failure refusing the next cycle and an actual process
SIGTERM while muted. They do not establish native GPIO behavior, full-duplex
quality, continuous-session resource use or acoustic latency. Device qualification
of the composed optional-camera/ring failure policies remains required. With
both hand-control switches absent, finite directed/fixture behavior and audio
policies are unchanged. An explicitly enabled software mute ends a finite
directed run; only `serve` supervises another cycle. Fixtures reject hand-control
flags rather than claiming to exercise their hardware lifecycle.

## Child-exit fixture observations

The test-only `a_privacy_worker_ended` helper records its command, callsite,
child PID, setup/wait times, poll count and observed exit code/signal. A failed
OS exit query is reported immediately, separately from a child still running.
Its real five-second exit-observation bound and two-millisecond poll stay the
same. The command fixture's `R` marker proves its first shell started; it does
not prove the inner command completed. Only the observed exit status does.

Only the deliberately crashing stand-in runs
`ulimit -c 0 && kill -SEGV $$`. The checked limit avoids unnecessary regular
core files, and exact SIGSEGV and exit-101 assertions prevent a shell error
from satisfying the intended crash/nonzero cases. Linux may ignore RLIMIT_CORE
for a piped core collector ([core(5)](https://man7.org/linux/man-pages/man5/core.5.html));
a collector can delay reaping ([kernel core_pipe_limit](https://www.kernel.org/doc/html/latest/admin-guide/sysctl/kernel.html#core-pipe-limit)).
This is fixture hygiene and observability, not proof that the original #3544
timeout involved core writing or a particular command. Its failed native result
remains failed; no current pass substitutes for its missing process timeline.
Production freshness, unreadable-switch retries, cleanup and failure policy are
unchanged.

On 28 October 2026, the macOS x86_64 gate on the fixture change based on
`fc9c781af` passed formatting and strict Voice all-target Clippy. A freshly
compiled Voice library test executable then ran these exact cases once each,
with one test thread and no retries:

- `service_privacy_controls::tests::only_a_privacy_worker_that_ended_cleanly_at_its_start_is_an_unreadable_switch`
- `coordinator::service::tests::a_conversations_privacy_worker_ended_at_its_start_goes_back_to_the_wait`
- `service_privacy_controls::tests::an_unreadable_switch_is_read_again_and_never_ends_the_service`

All three passed. The first two supplied ten real child start/status pairs;
every observed child PID was absent after its case. The compile, discovery and
runtime process groups were absent at closure, source and executable guards
matched, and all 192 previously retained drivers remained unchanged. These
are selected host fixture results, not a full-suite, Linux, device or original
failure-clearance result. This result paragraph was added after execution and
is not a compiled or runtime input to these cases.
