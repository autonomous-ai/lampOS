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
observation nor an indication can grant it independently of Voice.

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
(#795; lamp-4ace, 2026-10-16). The worker's own `privacy switch unreadable:`
fault, queued before it ended, and a message of its aged out after a clean
exit are part of that ending, not a failed cleanup; its exit status is still
checked. A failed cleanup still ends it. One window is left: in the muted
wait, if the wait notices the worker's end more than about 10 ms after the
muted-ring writer loses its lease, that writer blanks and exits with a fault,
and the wait ends as a failed cleanup. A previous runtime
error cannot hide a shutdown failure. The combined error retains both causes
and does not expose a restartable physical- or software-mute token.

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
and it is never sent across one. No preservation
of Gemini conversational memory across physical mute is claimed. There is no
periodic context reset or timer-based reopen. A later stable unmute must satisfy
the new runtime's ordinary readiness checks again.

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
