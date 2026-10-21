# Requested local Music on lamp-4ace — planned, not run

This is the proposed lamp-4ace check for requested audio on the second voice
route. It records no lamp result yet. Run in an authorized device slot after
the exact candidate's host and ARM64 gates pass. Use the existing services and
speaker owner; no extra player, device-side renderer, volume boost or motor
test is part of this check. See [requested audio](requested-audio.md) and the
[source example](../examples/relaxing-music/README.md).

**Prepare on the build host, from the `lampOS` root at that revision:**

```sh
cargo test -p lamp-skills --example render_relaxing_music
cargo run -p lamp-skills --release --example render_relaxing_music
cargo run -p lamp-skills --bin lamp-skills -- pack \
  --out artifacts/skills/relaxing-music/packages \
  artifacts/skills/relaxing-music/staged/relaxing-music
shasum -a 256 \
  artifacts/skills/relaxing-music/staged/relaxing-music/relaxing.wav \
  artifacts/skills/relaxing-music/packages/relaxing-music-1.0.0.tar
```

Keep the renderer's frame, byte, peak and sliding-window energy output. The
original Apache-2.0 example is 48 seconds, 1,152,000 frames of 24 kHz mono
16-bit PCM, 2,304,044 WAV bytes, with two-second fade-in and three-second
fade-out. Its absolute sample peak is at most 900; do not normalize it or
raise the mixer to compensate for its quiet start. Have the operator judge
comfort and quality at the intended safe listening level; stop if unsuitable.
No commercial recording is needed.

Before changing the lamp, save its effective unit/drop-in configuration,
binary paths and SHA-256 values, skill inventory, speaker mute/volume state,
active output caps, and microphone privacy state. Keep the known working
configuration and artifacts available for restoration. Determine the effective
Skills `--dir`: the checked-in lamp-4ace unit uses `/var/lib/lamp-skills`,
whereas the installer default is `/var/lib/lampOS/skills`. Install the package
with the existing `lamp-skills install --dir DIR PACKAGE.tar`, substituting
that effective directory and the transferred package path. Verify the package
and installed WAV hashes against the host record. Preserve an existing skill
with the same name before any replacement.

**Enable only for the allocated check:**

| Process | Required setting |
|---|---|
| `lamp-voice` | `LAMP_REQUESTED_AUDIO=on`; `LAMP_VOICE_ROUTE=second`; absolute `LAMP_SKILLS_DIR=/run/lampos-skills` matching Skills' effective `--tools-dir` |
| `lamp-skills` | `LAMP_REQUESTED_AUDIO=on` |
| `lamp-actions` | `LAMP_REQUESTED_AUDIO=on` |

Only the exact value `on` enables the feature. Keep the existing second-route
configuration, privacy controls and output caps. Confirm all three use the
same bus: Voice's `LAMP_BUS_ROOT` defaults to `/run/lampos-bus`, matching the
other services' `--bus-root`. Restart the affected services through the normal
device procedure and record their effective settings and identities. Confirm
normal Voice readiness and a current, known, unmuted `speaker/state` before
requesting Music. Installing the package alone does not enable playback.

**Exercise and record these cases:**

1. Ask aloud, “Lamp, play relaxing music.” Run the first request after service
   startup and at least three separate repeated requests. Require one short
   confirmation, then one recording under that same request. It must not
   overlap the confirmation, repeat, or trigger by itself. Listen through one
   complete 48-second recording; check the fades, absence of clicks and
   unintended speech, and return to normal conversation afterward.
2. On a fresh request, say “stop” during the recording through the existing
   spoken Stop path. Repeat with the existing Stop control if available.
   Record when each stop was observed, when the speaker tail retired, and
   when acoustic silence occurred. Do not treat an IPC receipt as acoustic
   silence. Remain quiet through the original request's maximum 70-second
   lifetime: the recording must not restart. Repeat with microphone privacy
   closed during playback and then reopened; reopening must not resume it.
3. On another fresh request, mute and promptly unmute the speaker through the
   existing Actions `volume_mute`/`volume_unmute` control path. Do not bypass
   Actions with a direct mixer write, and do not issue a new spoken request
   during this test: that would introduce a separate interruption. Record
   the mute-generation change and actual silence. Unmute must not resume the
   old recording, even when it happens before a normal speaker-state refresh.
   Observe through the original request's lifetime, then require a new spoken
   request for any later Music.
4. Check a normal spoken interruption replaces Music and is answered, with no
   later resumption of the old recording. Record owner/request identities and
   whether cancellation interrupted either the confirmation or the recording.
5. Check the disabled path with the switch unset in all three processes,
   then with each required process disabled individually while the other two
   are enabled. Start each configuration cleanly so stale state cannot count
   as readiness. The recording must never start; ordinary spoken answers and
   existing short Say cues must still work. Restore the enabled test
   configuration only when another planned case requires it.

During the authorized lamp run, record Skills CPU usage and ordinary tool-call
IPC latency while full-content reloads run, along with inventory size,
storage/cache conditions and any lease or start failures. Measure first and
repeated scan duration only when its boundary is observable; otherwise mark
it unmeasured. Keep these results separate from the Mac benchmark. Leave
maximum-inventory measurements unrun unless included in the allocated check.

**Timing and evidence:** retain the session trace's
`speech_final_sample_retired`, `requested_audio_handoff`,
`requested_audio_first_write`, and `requested_audio_final_sample_retired`
records with the same owner/request. The software boundary is confirmation's
final retirement to the first accepted recording PCM:
`confirmation_to_first_write_us`, target at most `250000` with the asset
already prepared. Report every first/repeated value and any preparation or
deadline failure; do not omit failed starts from the results.

Measure audible onset separately at the fixed listening level. The recording
fades in, so state the onset criterion and noise floor. To report confirmation
retirement to first audible Music, establish and describe the recording-to-host
clock alignment and its error. Without that alignment, report the acoustic gap
from the confirmation's audible end to audible Music and the software handoff
as separate measurements. Neither one alone proves the other. Retain redacted
trace/journal excerpts, the measured audio if authorized, and their hashes;
keep credentials and unrelated speech out of public evidence.

**Restore and close:** end playback, restore the saved switches/drop-ins,
skill inventory, services, volume/mute and privacy settings through the normal
device procedure, and verify ordinary Voice readiness and no unsolicited
Music. Record the restored binary/configuration hashes and effective switch
values. The run record must identify source commit, deployed Voice/Skills/
Actions hashes, package and installed WAV hashes, route, volume/caps, case
results, timing boundaries, failures, restoration result, and artifact hashes.
Keep unrun or unmeasurable cases explicitly marked as such; host tests and a
quiet digital waveform are not evidence of lamp latency or acoustic comfort.
