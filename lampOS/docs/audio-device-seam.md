# Audio device seam

Scope: `crates/audio/src/{device,alsa_device,testing,capture}.rs` and the
capture loop in `crates/voice/src/audio_workers.rs`. **The capture half is
done. The speaker half is not started.**

## Why

The microphone and speaker boundaries decide whether audio may be read or
written: permission, privacy generations, epochs, sequence numbers, deadlines,
what counts as a discontinuity. Until now they called ALSA directly, so every
one of those decisions could only be exercised on Linux against ALSA's `null`
plugin, which never stalls, never underruns and never disappears. The capture
and speaker loops built on them had no tests at all off the board.

A trait between the boundary and the device changes that. The boundary and
the worker loop are unchanged above it. Below it is ALSA on the board and
anything a test scripts elsewhere.

## Shape

```rust
// crates/audio/src/device.rs, every unix target
pub struct DeviceFault { pub errno: i32, pub call: &'static str }
pub enum DeviceState { Setup, Prepared, Running, XRun, Draining, Suspended, Disconnected, Other }
pub struct CaptureStatus { state, monotonic_us: Option<u64>, available: i64, delayed: i64 }
pub enum ConfigureError { Fault(DeviceFault), Unsupported(CaptureFormat) }

pub trait CaptureDevice: Sized {
    fn open(name: &str) -> Result<Self, DeviceFault>;
    fn configure(&mut self, config: CaptureConfig) -> Result<CaptureFormat, ConfigureError>;
    fn start(&mut self) -> Result<(), DeviceFault>;
    fn read(&mut self, into: &mut [i16]) -> Result<usize, DeviceFault>;   // one nonblocking read
    fn status(&mut self) -> Result<CaptureStatus, DeviceFault>;
    fn discard(&mut self) -> Result<(), DeviceFault>;
}
```

- `MicrophoneOn<D: CaptureDevice>` is the boundary. `Microphone` is still the
  name of the ALSA one (`MicrophoneOn<AlsaCapture>`, Linux only), so no call
  site changed.
- `AlsaCapture` makes exactly the calls the boundary made before, in the same
  order, including the same negotiation and the same acceptance of a format.
  It is the only code that mentions ALSA on the capture side.
- The boundary checks the reported format against its own bounds as well. A
  device cannot talk it into a rate, channel count or buffer it would not
  have accepted from ALSA.
- `CaptureError` carries a `DeviceFault` (call name and `errno`) where it
  carried an `alsa::Error`. The fields are `device_error` and `close_error`.
  Nothing outside the audio crate read the old field.
- `errno` values are named in `device::errno`. Two that the boundary acts on,
  the suspended-stream and bad-state codes, exist only on Linux; elsewhere
  they keep their Linux numbers so a scripted device can still produce them.
- `capture_on::<D>` in `audio_workers.rs` is the capture worker over any
  device. `capture` and `capture_with_noise_suppression` call it with
  `AlsaCapture` and are otherwise as they were.

## The scripted device

`lamp_audio::testing::{CaptureControl, ScriptedCapture}`. A test registers a
name, opens a `MicrophoneOn<ScriptedCapture>` on it through the same `open`
the boundary always uses, and then decides what the device holds and when it
fails: push samples, fail the next call of a given name with a given `errno`,
change the reported stream state, stall the next read, change the format it
configures to. It counts opens, starts, discards and reads.

It has no clock and no rate. A room or a real-time model belongs on top of
it; Agent 2's simulation harness is the intended user. Nothing in it is used
by the runtime.

## Tested

On the macOS host, `crates/audio/src/capture.rs` unit tests, 12 in all. The
last eight are new behavior coverage that did not exist off Linux:

| Test | What it pins |
|---|---|
| `blocks_are_assembled_across_short_reads_in_order_from_sequence_one` | 100 + 60 samples make block 1 exactly; the next 160 make block 2; an empty device is neither an error nor a frame; no status clock is invented |
| `an_xrun_closes_the_device_destroys_the_partial_block_and_restarts_as_a_new_epoch` | The partial block is erased; nothing reopens before `reset`; the restart is epoch 2, sequence 1, and carries nothing the old handle held |
| `a_device_that_vanishes_or_stops_running_is_a_discontinuity_never_silence` | Four stream states and four `errno` values, on read and on status, each mapped to its discontinuity; a whole block read just before is not handed over |
| `a_read_that_blocks_past_its_budget_is_a_discontinuity_not_late_audio` | A read that returns after 15 ms: `ReadBudgetExceeded`, device closed, the late block never delivered |
| `a_block_left_incomplete_for_a_buffer_length_is_dropped_not_finished_late` | Ten samples, then 42 ms: `PartialTimeout` instead of a block assembled across the gap |
| `open_and_configuration_failures_leave_nothing_open` | A missing device; a failing configure, with the opened handle discarded; four out-of-bounds formats refused |
| `privacy_lineage_change_discards_an_incomplete_block` | Denied and allowed again inside one snapshot: closed, 40 samples erased |
| `a_failed_discard_is_reported_and_the_handle_is_still_gone` | The close error is returned and the device is closed regardless |
| `after_a_stall_a_muted_authority_keeps_the_microphone_closed` | After an overrun, a fresh snapshot that mutes the microphone: `prepare` refuses with `NotPermitted` and the device is not opened again |

The existing Linux tests against ALSA's `null` plugin
(`crates/audio/tests/it/capture_linux.rs`) still run through `AlsaCapture`.

Results are in [Agent 1's status](../coordination/agent-1.md).

## After a stall (#784)

A capture worker held up for a few hundred milliseconds (on lamp-4ace, the
kernel re-tuning its SD/MMC hosts) comes back to a lapsed authority or an
overrun buffer: `AuthorityExpired`, `XRun`, `PartialTimeout` or
`ReadBudgetExceeded` closes the device as before. Once the microphone has
started, the worker (`audio_workers.rs`) no longer ends on these: it says
`CaptureGap` with the closed epoch, keeps the microphone closed, and opens it
again only through `reset` and `prepare` under the coordinator's next fresh
snapshot, which checks the privacy switch and the mute before the device is
touched (the test above). It then starts it without waiting for the speaker
clock, which is already running, and reports `CaptureStarted` with the new
epoch. A changed privacy generation in the gap, any other discontinuity, or
no reopen within 1.5 s still ends the worker. The coordinator drops the
closed epoch's frames still in flight, and an utterance the gap cut into is
abandoned as a hand Stop abandons it; what is still said of it after the gap
is dropped until its speaker pauses, so it is not a new question.

**The speaker** (#1178). At midnight on lamp-4ace the voice ended on a bare
"authority snapshot has expired": a worker held up past its lease (a turn's
is 100 ms) while the log sync loaded the eMMC, with nothing fresher in its
queue, found its installed authority lapsed. In the speaker that is the
check of its queued silence (`check_silence` under `maintain`), and minting
or writing the next zeros (`zero_authority`, `try_write_silence`,
`start_clock`, the prime): each returned the lapse as an error, the worker
ended, and the voice with it. Now, as the provider workers skip a lapsed
snapshot, the speaker worker (`audio_workers.rs`, `lapse`) stops the device
once, reports the reset as after any discard (`PlaybackClockReset`), says so
in one journal line ("its authority lapsed while it was held up; silent
until a fresh one"), and writes and primes nothing until a fresh authority
is installed; then it primes and starts its clock again, as after any
reset, and says how long it was silent. A lapse that lasts past the
capture worker's own bound (1.5 s, `REOPEN_US`) still ends the worker: the
voice has ended by then anyway, and a speaker whose coordinator is gone must
not keep the device.

What the lamp was saying when its speaker lapsed is lost, and that costs
the answer, never the voice (the reviewer on #1214). Queued speech is
dropped by its own ownership check, which names its owner and the lapse
(`PlaybackDiscarded`, `ExpiredState`), and the chunk not yet written in full
is refused as lapsed (`PlaybackRejected`, `lapsed`), so it is never written
under its old permit after the fresh authority. The coordinator took either
as a failed voice ("unexpected playback discard", "current reply rejected
at speaker"); now it cuts that answer, as a reminder's lapsed lease cuts
one, with what the provider had queued for it (`cut_after_speaker_lapse`,
outcome `speaker_lapsed`), and goes on. The capture side's late answer to a
prime the lapse reset is dropped with one journal line, where it ended the
worker (`InvalidStart`; Ten on #1214).

Tests: `tests/speaker_lapse_worker.rs`, the worker's own loop on ALSA's
`null` device (Linux only, so the ARM64 run): its turn's 100 ms lease left
unrenewed for 200 ms and then the capture side ready, so that its next
prime needs the lapsed authority, it stays up and stops cleanly, where the
prime's `zero_authority` ended it before; and after a lapse and a fresh
authority, the late answer to the prime of the reset epoch is dropped.
`coordinator::tests::speaker_lapse`: a lapse discard or a lapsed refusal of
the current answer cuts it and goes on, and any other refusal still fails.
`endpoint_session::a_speaker_lapse_mid_answer_cuts_it_and_the_next_question_is_answered`:
through the coordinator's own loop, the speaker reports a lapse after the
first answer's second chunk; that answer ends `speaker_lapsed`, nothing more
of it is sent, and the next question is answered (with the cut taken out of
the loop the session ends in an error).
`null` plays every frame as it is written, so the clock never starts there
(the prime needs 480 frames queued): the lapse with speech or silence
queued on the device, and the prime again after it, are shown on a lamp
only.

**The speaker underrunning** (#3697). On lamp-52e6 at 09:18:44 on 29
October, 0.39 s into a plain start, as the models loaded, ALSA's
`snd_pcm_avail_delay` failed with "Broken pipe (32)": the speaker worker was
held up past its 40 ms queue. Outside a declared drain every EPIPE was a
fault (`pcm.rs`, `discontinuity`), so the worker exited 1, the voice with it
("verified cleanup failed: speaker worker exited abnormally"), and the
restart guard muted its next start. Now an EPIPE with only zeros queued (no
speech in the ledger, no retiring tail, no unreported discard) is stopped and
prepared as by `stop`, the guard kept, and returned as `pcm::Underrun`. The
worker (`audio_workers.rs`, `underrun`) reports the reset as after any
discard and primes the clock again through the barrier, with one journal
line: "it underran while held up with only zeros queued; priming again".
Nothing said is lost, because nothing said was queued. Still faults, as
before:
- an underrun with speech queued, or with an answer under way (its speech
  begun, or a chunk part-written), since its gap would cut it mid-word;
- `ENODEV`, `EBADFD` and `ESTRPIPE`: a disconnect, a bad state or a
  suspend;
- more than `MAX_UNDERRUNS` (3) within `UNDERRUN_WINDOW_US` (10 s), in
  `crates/audio/src/underrun.rs`: a speaker that keeps underrunning is
  failing, not held up, and the restart guard should see it.

Tests:
- `underrun::tests` (any host): three in the window are recovered and a
  fourth refused, underruns spaced wider than the window are recovered each
  time, and a refused one is not counted.
- `pcm::underrun_tests` (Linux only, so the ARM64 run), with ALSA's errno
  injected on the `null` device:
  - an EPIPE with only zeros queued stops, counts a discard, keeps the guard
    and primes and starts again;
  - the fourth within the window closes the guard;
  - `ENODEV`, `EBADFD` and `ESTRPIPE` close it;
  - an EPIPE with speech queued closes it.

The worker's own branch has no test: `null` never underruns, and the worker
loop's one test (`tests/speaker_lapse_worker.rs`) cannot inject an errno.
An underrun mid-answer stays a fault; recovering it needs a discard reason
the coordinator cuts an answer on, as it does for a lapse.

**A speaker held up, with recovery off** (#1178, lamp-cb94 at 04:53:53 UTC
on 17 October, 4 h 34 min into Thirteen's soak). The journal had a bare
"Connection refused (os error 111)", then `reference ReferenceLate`, then
the voice's own "Connection refused (os error 111); verified cleanup failed:
capture worker exited abnormally: exit status: 1". Nothing there connects.
Read from the code and the session's own record, the chain was:
1. cb94 ran `--reference-clock capture --reference-recovery off`. With
   those, the capture worker ends on `ReferenceLate` when, at a capture
   block:
   - the speaker's newest reference is more than 40 ms old
     (`MAX_REFERENCE_AGE_US`);
   - or the queue the speaker last reported has run out at the nominal
     rate (`before_capture`, "nominal starvation");
   - or its ring runs dry.

   cb94's fault (Thirteen, from the session's `events-1.jsonl`): the last
   reference was accepted 20.0 ms before it, with 456 frames (19 ms)
   queued, and nothing received since. So it was the queue running out,
   not the 40 ms. The speaker had sent nothing for as long as its queue
   held. In that same second the kernel logged the SD controller retuning
   with "wait dma hold bit clear timeout", and the USB capture's callbacks
   suppressed: the processes held up together for a moment.
2. Its datagram sockets close. On Linux the next send to a closed peer is
   refused. The first bare refusal is most likely the speaker's next
   reference to it: a worker's last line, since each worker prints its
   error and exits 1, and the log relay doesn't keep order across
   processes. The voice's own is the coordinator's next `publish`
   (authority to every worker), which ended it.
3. That refused send also drops what the gone worker had sent and not yet
   been read, its fault among it
   (`transport::tests::a_refused_send_drops_what_the_gone_peer_had_sent`).
   So the voice ends on the refusal and not on the fault.

A refused send now names its socket (`capture.c.p: Connection refused …`,
`ref.d.speaker: …`), as a refused receive does. A full queue is still a
plain `WouldBlock`.

What keeps the voice up is `--reference-recovery bounded`
(`reference/recovery.rs`), which already exists. A speaker held up past its
queue, or a dry ring, makes the reference **untrusted** instead of a fault:
the canceller isn't run and no echo evidence is given, for at most 1 s,
then it reseeds from fresh reference. It allows three reseeds a minute and
warms for 3 s. That costs the canceller for a moment, not the voice. Since 17 October
the product unit has it on too (#1332); lamp-4ace has run it from a
drop-in since the morning of 17 October and hadn't ended on `ReferenceLate`
by 13:21 that day (the first builder on #1178). lamp-cb94 took it at 07:21 UTC,
with Twelve watching an hour of `NRestarts`, the reference statuses, run
ends, mute and the kernel's mmc and usb lines.

Tests: `tests/capture_hold_worker.rs`'s
`a_speaker_held_up_ends_the_capture_worker_unless_recovery_is_bounded`
(Linux only, so the ARM64 run), the worker's own loop and sockets:
- **Off:** after 1 s of stream the speaker accepts nothing while the
  microphone goes on. The worker ends on `ReferenceLate` once the queue its
  packets report (40 ms in this rig) has run out, and the speaker's next
  reference and the controller's next authority are refused, each naming
  its socket.
- **Bounded:** the same 100 ms pause; every block is handed on through it
  and after it, and the worker stops cleanly.

The rig sends the worker nothing after what ends it (`Stop`, or authority
that closes the microphone). Until #1359 it went on renewing authority while
it waited for `CaptureStopped`. When a renewal fell due just after the
worker had gone, it was refused, and on Linux the refusal dropped the
`CaptureStopped` waiting unread
(`transport::tests::a_refused_send_drops_what_the_gone_peer_had_sent`). The
bounded half failed that way about one run in six to ten (Tests, Linux
x86_64 in podman). `a_stop_is_read_though_the_worker_has_gone_when_authority_falls_due`
makes that order certain.

`transport::tests::a_send_to_a_peer_that_has_gone_is_refused_with_the_sockets_name`
checks the name. The hold was most likely the kernel's, from the same
second (cb94 kept no stalls trail): the SD/MMC host's retune, which #784 found holding the voice's processes for about 0.3 s
some eleven times a day on lamp-4ace. On cb94 the ring's slow SPI write
(its third in 4.5 h) came in the same second.

## Not done

- **The speaker half.** `pcm.rs` still calls ALSA directly and is Linux
  only. The trait for it was published in `A1-007`. Until it exists there is
  no scripted speaker and no accepted-PCM echo path through a scripted
  device; the speaker loop's one test runs on ALSA's `null` device on Linux
  (`tests/speaker_lapse_worker.rs`), where its clock can never start.
- **The capture loop itself has no test yet.** `capture_on` exists and
  compiles for the board; nothing drives it. `audio_workers` is compiled only
  on Linux (`crates/voice/src/lib.rs`, the integrator's file), so a test of it
  runs in a Linux container or needs that gate opened.
- **Nothing here is evidence about a device.** A scripted stall is a sleep; a
  scripted xrun is a number. They prove what the boundary does when the
  device says so, not when or how a real codec says it.
- Restart remains a new worker incarnation, which is the integrator's. The
  boundary's own `reset` then `start` is tested above and gives a new epoch.
