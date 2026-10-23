# Product head-touch acceptance for #3276

Prepared by Third; **not executed**. This continues the [earlier handoff](../wake-hum-permission-20261022/README.md#lamp-handoff)
and separates the remaining acceptance boundaries. #3275 must first land
with its privacy and units/update reads. The lamp owner uses one coherent
product build containing that merge, not the development checkout or a
collection of temporary feature overrides.

## Ownership and setup

First/lead routes the attended run to lamp-52e6 with its owner, or lamp-c0a4
with its owner and operator. Third owns no lamp and performs no device action.
The lamp owner follows joining.md: permission for access and room-visible
steps, the lamp lease, a Queue `PLAYING` window, and the previous-build/restore
record. Before motion, show only the lamp, its placement and clearance, name
the fold/wake movement, and have the operator at the arm. Do not record a
person's face or a screen; the lamp and necessary hand contact are enough.

Record the installed commit, binary identities, effective product units and
local overrides, actual per-unit pins, second voice route, speaker level,
physical privacy hardware, and flags. Required together: Voice
`LAMP_HEAD_TOUCH=on`, `--controls-mute`, `LAMP_RESTORED_MUTE=on`, Expression
`--head-touch`, Body's head-touch switch and the matching non-legacy pins.
Record whether `LAMP_HEAD_TAP_CUE_DBFS` is unset (the product's −20 default)
or overridden. An override or temporary test-input source is not acceptance
of the product defaults. Keep the original mute and privacy state for restore.

## Timing record

Record physical contact down and release, first audible cue, and visible
listening/sleep/body changes on one audio/video timeline, with its real frame
rate and timing uncertainty. Keep both contact-to-sound and release-to-sound;
a gesture qualified on release must not silently lose its held duration.
Audibility is judged from a seat beside the lamp at its own product speaker
level, not with an ear beside the driver or by turning up a recording.

Retain the corresponding source trace: tap `began_us`/`acquired_at_us`, the
`head_tap_cue` decision, speaker first-write/retirement events, and
`listening_ready`. The existing quiet-cue acquisition-to-first-write target
is under 150 ms; the wake acquisition-to-first-write target is 500 ms. Those
source intervals are distinct from the physical and acoustic intervals.
ALSA's first accepted write is not acoustic onset; the listening face alone
is not proof of retained capture.

For physical tap-to-listening readiness, record how the audio/video clock
was aligned to the lamp's monotonic trace and the alignment uncertainty.
Without that evidence, report physical-to-sound and acquisition-to-readiness
separately, and mark physical-to-readiness unmeasured. #3276's row 3 mentions
both first-speaker and listening time: report both boundaries explicitly.
Never subtract unrelated clock values or relabel readiness as first sound.

## Attended rows

| Row | Action and evidence | Pass record |
|---|---|---|
| First quiet tap | After the coherent product build starts and listening is genuinely ready, make one real tap. Capture the audible "hm?", face and first spoken words retained afterward. | Seat-level audibility; physical and source timings above; actual retained input. |
| Repeated quiet taps | Repeat three times with the unchanged quiet/readiness conditions and at least 30 s between hums. Retain a refused attempt's reason rather than deleting it. | Every attempt and its latency; rarity/gate refusals separated from missing feedback. |
| Hold, sleep and wake | From standing with clearance, microphone permission Allowed, quiet and listening-ready, hold 3 s. Keep it quiet through the 2.0–2.2 s breath decision window and retain any refusal reason. On camera capture the last-second sigh, fold and light; release, then use one real tap to wake. | Sigh heard, fold and light seen, wake behavior, mute state and resumed capture. Record any first-word loss. |
| Hands away | Leave the head untouched for 20 s after the gesture sequence. | No invented touch, unwanted repeated hum or repeated sleep/wake reaction. |
| Crash recovery | With real capture already delivered, both required Voice switches on and physical privacy open where present, the lamp owner sends SIGKILL to Voice. Observe restart, the guard's restored mute, then one fresh real tap. | Restart stays muted before the tap; one tap permits resumed capture/readiness. Record the crash, restore, tap and capture times. A wake hum is not a requirement of this fault row. |
| Chosen sleep survives a crash | Put the lamp to sleep by a real 3 s hold, then the owner repeats the crash. | The chosen sleep remains muted after restart until the normal fresh wake action. Record the observation window and wake separately. |

For the crash row, an ordinary service restart or SIGTERM follows clean-stop
cleanup and does not substitute for SIGKILL after delivered capture. Require
`mute_restored_after_fault` or the exact journal line from that restart:

> `lamp-voice: muted by the restart guard after a fault, not by choice`

An already-unmuted restart does not pass that row. A lamp without a physical
privacy slide cannot qualify the slide path; state that limitation. A sleep
mute and an explicit microphone privacy mute are different states: the
existing rule that a tap cannot open a chosen microphone mute still holds.

## Result to post

For every row retain build/flags, attempted action, observed result, raw
trace/recording location, timing boundaries and uncertainty, and missing
observations. Record first use and repeated use separately. Preserve failures
and restore the lamp's prior state, then release the lease. Post the result
on #3276 and the Queue; First owns acceptance and the issue's closure.

Existing evidence remains separate: Fifth's attended #2857 trial; the
21 October attended c0a4 recovery (~370 ms to readiness); the 23 October
injected recovery (~300 ms); and the tap-only −30/−25/−20 level comparison.
None substitutes for this coherent product-build run. The louder sleep
breath is heard in the hold row; other shared hum paths retain their own
acceptance work and are not qualified by the tap comparison.
