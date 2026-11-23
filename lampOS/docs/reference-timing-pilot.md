# Reference timing pilot

The microphone and speaker use separate USB clocks. The retained native run03
software counters drifted by approximately157.235 ppm across377 reports and
reached the fixed reference bound after378.6 seconds. These are software-count
observations, not an acoustic or oscillator measurement.

The first native candidate selects `--reference-clock capture` with
`--turn-taking v1-baseline`; reference recovery remains off, and duplex admission
remains off. The bounded resampler follows the capture clock without a periodic
canceller reset. Its rate is clamped to plus/minus500 ppm, target unread reference
is1200 frames at24 kHz, and maximum unread capacity is2400 frames. Original
privacy, packet identity, loss, freshness, queue and clock faults remain faults.
There is no retry, inserted reference silence or automatic mode switch.

`--reference-recovery bounded` is a separate opt-in requiring a policy that masks
own playback. In fixed mode it can re-centre a depleted margin after checking the
current capture status. In capture-clock mode it can temporarily bypass an
untrusted canceller and reseed from fresh reference within the original1-second
deadline. At most3 reseeds are permitted per60 seconds. New DSP evidence stays
withheld until300 blocks of own sound have warmed the canceller. Those are policy
bounds, not a measured acoustic guarantee. It is not selected in the first pilot.

Independent review reproduced three recovery defects in a39d04d84: a delayed
capture was classified using its previous backlog, reseeding preceded the
untrusted deadline check, and future queue timestamps were accepted. Agent1
corrected these at5df4248c3 and retained the three reproducing tests. Native
qualification must name the exact composed artifact; the earlier candidate was
never deployed. No V1 benchmark or human-overlap success is claimed.

The bounded native test uses a single runtime, eight cached questions, idle, and
eight further questions after1210 seconds. Input scheduling still requires fresh
original session/capture/provider evidence and actual prior-turn completion.
Three approved local room recordings cover the full test with measured overlap.
A runtime fault, unknown reset, extra admission, unconfirmed cleanup or missing
evidence fails the qualification and withholds remaining prompts. Stationary
rollback and exact installed/configuration identities are retained.

The capture-clock path still needs real hardware continuity and echo-quality
checks. Tone interpolation tests and offline synthetic mixes do not establish
speech cancellation depth, human interruption quality or acoustic response time.

## Certified prime preparation

Native04 failed before input with `InvalidStart`: capture-clock mode correctly
reported accepted cursor 480 and analysed cursor zero, but the speaker required
analysis cursor 480. Earlier virtual reference tests bypassed that speaker
handshake. The corrected internal protocol distinguishes prime preparation from
analysis. `ReferenceAssembly::prime` mints the typed acknowledgement only after
its existing epoch/discard/privacy/freshness checks and successful DSP/ring
preparation. `PrimeBarrier` is the same portable validation path called by the
Linux speaker immediately before its guarded start. It is one-shot and retains
the original deadline: 1 s for epoch 1, the first start after the voice
starts (#8391), 100 ms for every later epoch. Reset requires a new certified prime; it cannot
reuse the old acknowledgement.

The fixed-mode default and DSP counters are unchanged. In capture-clock mode,
480 accepted prime frames remain held and zero remain analysed until normal
capture-paced resampling consumes them. No reference, capture or acoustic
freshness is fabricated. This changes an internal wire shape; speaker and
capture must use the same runtime build, with no legacy-ack fallback.

Host regressions use actual bounded Unix control channels and the production
capture-preparation/speaker-validation helpers for fixed and capture-clock
startup/reset, plus malformed, stale, duplicate and privacy cases. They do not
open ALSA or prove Linux worker wiring, native readiness, physical clock
continuity, or echo quality. Native qualification still needs the exact new
binary, successful matching startup and first retained input, and a bounded
continuity run with recovery off. Keep the failed Native04 evidence separate.

Reuse boundary: pinned V1 main `d5efe9d7b73cc529b34cd4abe97624682a82ca94`,
`hal/drivers/voice/aec.py:51–65,130–147`, already keeps queued reference separate
from the later reverse-stream DSP call and clears it on reset. This correction
preserves that distinction within Rust's existing explicit two-process startup
barrier; it does not port V1's missing-reference padding or uncancelled fallback.
No new dependency, queue, retry, mode default, or admission policy is introduced.
