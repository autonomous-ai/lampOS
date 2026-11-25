# Lamp ring driver

`lamp-ring` is a Rust library for the fixed 32-pixel WS2812 ring, not a complete
runtime or a running ring process. Conversational pixels require an actual
`lamp-turn-owner` light permit. A separate, narrow physical-mute method can
render only a dim red breath under fresh denied authority and a matching
physical switch observation. Neither path manufactures listening readiness.

`lamp-voice` now supplies an optional supervised writer and a phase policy for
directed qualification. See [conversation/ring ownership](../../docs/ring-choreography.md)
for its option, bounded scheduling, cancellation and remaining physical checks.

The codec emits exactly 1,028 bytes: 10 low primer bytes, 32 GRB pixels with
MSB-first bits encoded as `0xC0`/`0xFC`, and 250 low reset bytes. The policy passes
a validated channel ceiling of 0–120 (for example 40 at night). All channels
scale together, with nearest-integer quantization, to preserve hue.

The Linux transport opens only `/dev/spidev3.0`, obtains an advisory exclusive
lock, sets mode 0, 8 bits, MSB first and 6.4 MHz, and rejects differing
configuration readback. Returned settings describe the driver's accepted
configuration; they do not measure actual clock rate or optical output.
Legacy boot/shutdown LED services do not honor this lock and must be stopped or
excluded by deployment. Do not run competing ring writers.

`GuardedRing::show` encodes before checking `BoundaryGuard` with a fresh shared
`lamp-ipc` monotonic timestamp immediately before a single frame write. It
rechecks authority on return. `install_authority` applies revocations without
waiting for another paint; `tick` expires the displayed cue when no new work
arrives. Drain priority control messages before output and call `tick` at least
every 10 ms. The owner must refresh authority and policy; the driver never
manufactures readiness or permits from its last displayed color.

`show_privacy_muted` accepts a `PrivacyObservation`, never arbitrary pixels or
a light permit. A trusted Voice reader checks the typed bus envelope, source,
publisher incarnation, sequence and kernel clock. The ring independently
requires fresh denied microphone and camera authority with no turn, the same
controller boot and microphone generation, `permitted:false`, and a physical
mute observed no more than 50 ms before its expiry. Unknown, missing, open,
future or expired evidence cannot paint red. Ordinary Allowed snapshots cannot
authorize this path. The fixed four-second breath uses red 2–10, green/blue 0,
and at most 40 writes per second. Evidence can refresh between writes; switch
reopening or evidence loss blanks the indication immediately, and `tick`
expires it without new messages. The post-write check also blanks a frame whose
lease expired in I/O.

Voice selects this separate quiescent worker only with
`LAMP_PRIVACY_CONTROLS=on`; unset preserves the previous ring behavior. Its
shutdown black receipt and process exit must precede the next ring writer.
Shutdown itself does not release the SPI handle: destruction does. The same
exclusive SPI lock protects both workers. A red frame asserts only fresh
physical-mute evidence and denied authority, never an electrical microphone
power cut or optically verified state.

Rejected old permits or stale/duplicate snapshots preserve a newer valid cue.
A displayed cue that ends for no safety reason (its permit runs out, its turn
ends, another turn takes the lamp or its presentation changes) fades out over
`FADE_US` (200 ms), at most one frame every `FADE_STEP_US` (25 ms), squared so
it eases out as an eye sees it, then sends one black frame (#8615). It fades
only while a resting frame could be shown (fresh authority, a permitted
microphone, input available, no fault) and its own camera grant is current; a
resting frame or a new cue paints over it with no dark frame between. Any other
revocation of the displayed cue (privacy, input, camera, expired authority, a
fault) and a refused paint still cause one black frame at once. `dim` brings
the lit cue or resting frame down to a tighter output cap in the same steps,
never brighter and never black. Malformed
trusted control, transport loss or an I/O fault latches the writer fault and
blanks. Report both the original write error and any failed cleanup. There are
no automatic retries: one primary frame and at most one cleanup frame per fault.
`off` explicitly sends black; it is not a privacy/readiness status and does not
change controller permissions. `shutdown` is explicit and idempotent. Drop does
no I/O; after a crash the supervisor must arrange an exclusive replacement
writer before it can blank the hardware.

A frame's nominal wire duration is 1.285 ms at 6.4 MHz. The 10 ms write budget
is checked **after a write returns**. Linux spidev has no per-write userspace
timeout, so a stalled kernel call is not hard-bounded by this crate. Run the
writer in its own supervised process and enforce process-health deadlines
without blocking microphone capture or local speaker interruption. Bytes and
cleanup attempts are bounded; hard timing and optical response are unproven.

Dependencies are confined to lampOS path crates `lamp-turn-owner` and
`lamp-ipc`, plus Linux-only external Rust binding `spidev = 0.7.1`. The kernel
SPI driver, device node and permissions must be provisioned by lampOS. No HAL,
os-server, Python source, or sibling-repository configuration is required.

Validation commands after workspace inclusion:

```sh
cargo fmt --all -- --check
cargo test -p lamp-ring
cargo clippy -p lamp-ring --all-targets -- -D warnings
```

Tests cover all 256 channel encodings in all RGB positions, every ceiling
outside the permitted range, hue quantization, guarded cancellation/privacy,
idle expiry, stale-message preservation, short writes, fault cleanup and
shutdown. Linux-only tests also reject incorrect configuration readback. Unit
tests are not physical LED tests; device timing, visual brightness, ownership
handover and failure recovery still need qualification. Fake-sink privacy
checks cover denied/allowed authority, observation identity and expiry,
low-to-high and missing input, paint bounds, write failure, authority loss and
destruction before a successor writer can acquire ownership.
