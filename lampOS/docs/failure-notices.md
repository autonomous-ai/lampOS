# Spoken failure notices

Scope: `crates/audio/src/notice.rs`, the `lamp-audio notices` check, and the
optional same-owner integration in `lamp-voice`. The loader plays nothing by
itself. Live playback requires explicit configuration and a still-valid owned
failed turn. No recording is in this source: the product's four are in the
`notices-2026-10-20` release, placed by `lamp-update install` (below).

## The rule

When the voice service cannot be reached there is nobody to ask for a voice.
What Lamp may say about an outage therefore has to be on the device already,
recorded once in the approved voice. The runtime reads those recordings at
startup and never makes one: no synthesis request, no download, no fallback
voice, no resampling. A notice that is missing or fails a check is reported
unavailable and Lamp stays silent.

This replaces V1's behavior only in part. V1 kept a cache keyed by a hash of
provider, voice, model, speed and text, stayed silent when the rate-limit
notice was not in it, and for any other cached phrase synthesized on a miss.
The silence on a miss is kept. The synthesis on a miss is not.

## Directory

One directory, outside the source tree, given to the runtime by configuration:

```
manifest.json
provider-unavailable.en-US.wav
turn-lost.en-US.wav
```

`manifest.json`:

```json
{
  "schema_version": 1,
  "notices": [
    {
      "kind": "provider_unavailable",
      "language": "en-US",
      "transcript": "I can't reach my voice service right now.",
      "voice": "Aoede",
      "voice_revision": "gemini-live-2026-09",
      "approval": "production",
      "file": "provider-unavailable.en-US.wav",
      "wav_sha256": "64 lowercase hexadecimal characters",
      "frames": 52800
    }
  ]
}
```

The transcript, voice and revision above are placeholders showing the shape.
No wording or voice has been approved.

| Field | Rule |
|---|---|
| `kind` | `provider_unavailable`, `turn_lost`, `service_refused`, `back_online` (#830), `voice_service_unreachable` (#830, `LAMP_SERVICE_DOWN_LINE`), `signed_out` (`LAMP_SIGNED_OUT`), `wifi_lost` (#1959: the Wi-Fi helper's no-Wi-Fi line, played only when asked through the say service as `recording`, `setup.md`) or `wifi_recovering` (#3743: the recovery hotspot's line, asked the same way by `lamp-setup recover`, `setup.md`, "When the saved network is gone") |
| `language` | A BCP 47 tag, matched exactly. `en` does not satisfy `en-US` |
| `transcript` | The words in the recording, 1 to 240 bytes, no control characters. Kept for records and review; never sent anywhere to be spoken |
| `voice`, `voice_revision` | What produced the recording, 1 to 64 bytes each |
| `approval` | `production` or `test` |
| `file` | A bare `.wav` name in this directory: letters, digits, `.`, `-`, `_`; not hidden; at most 64 bytes |
| `wav_sha256` | SHA-256 of the whole file |
| `frames` | Mono frames at 24 kHz, 7,200 to 360,000: 0.3 s to 15 s |

Unknown fields, an unknown kind or approval, a second entry for the same kind
and language, more than 8 entries, or a manifest over 16 KiB make the whole
manifest invalid.

An unknown kind refuses the whole manifest, so one with `wifi_lost` goes onto
a lamp only with, or after, the build that knows it (#1959), as for #830's
kinds. The same holds for `wifi_recovering` (#3743): a voice before it
refuses a manifest that lists it, and with it every notice, so the notices
release that adds it goes onto a lamp only with, or after, a build that
knows it.

The longest notice was 10 s before #3743; the no-Wi-Fi line with the reset
pinhole runs about 13.4 s in the lamp's voice. An older loader refuses an
entry over 10 s (`invalid_entry`), and the Wi-Fi helper's fallback, words for
the model, can't be spoken without a network. So a notices release with a
recording over 10 s goes onto a lamp only with, or after, the build that
allows 15 s.

Each recording must be a regular file opened without following a link, a
complete RIFF/WAVE container of mono 24 kHz 16-bit integer PCM with exactly
the stated number of frames, matching its hash, unchanged in size while read,
and not all silence. Worst case held in memory: 8 notices of 15 s, 5.76 MB.

## What the runtime gets

```rust
let notices = Notices::load(directory, Approval::Production);   // never fails
match notices.get(NoticeKind::ProviderUnavailable, "en-US") {
    Ok(notice) => notice.samples(),   // &[i16], 24 kHz mono, as recorded
    Err(missing) => /* stay silent; report `missing` */,
}
notices.capability();   // what can and cannot be said, for a status report
```

`Notices::none()` is the same with no directory configured.

| `Missing` | Meaning |
|---|---|
| `no_directory` | None configured, or it does not exist |
| `no_manifest` | The directory has no `manifest.json` |
| `invalid_manifest` | Not a bounded regular file of the expected shape |
| `not_provisioned` | No entry of this kind in this language |
| `not_approved` | Provisioned only as a test asset |
| `invalid_entry` | Name, transcript, voice, hash or length malformed |
| `asset_missing` | The named file is not there |
| `asset_unreadable` | Not a bounded regular file, reached through a link, or changed while read |
| `asset_hash_mismatch` | The file is not the one the manifest describes |
| `asset_format` | Not complete mono 24 kHz PCM16 of the stated length, or silent |

One bad entry refuses only itself. A bad manifest refuses everything. Reasons
are fixed categories; no path or file content is carried.

## Test assets

A recording marked `test` is refused by `Approval::Production` with
`not_approved` and loads only when the caller asks for `Approval::Test`. The
tests in this repository write tones and mark them `production` only to
exercise the checks; a tone in a temporary directory is not a voice asset and
nothing here ships one.

## Checking a directory

```sh
lamp-audio notices /path/to/notices              # as a production runtime would
lamp-audio notices /path/to/notices --accept-test
```

Prints the capability as JSON: what loaded, with transcript, voice, hash and
duration, and what was refused and why. Run it on the device after
provisioning and keep the output with the provisioning record.

## For the integrator

This crate cannot speak. The coordinator owns that:

- Samples go through the existing guarded speaker path, in 240-sample blocks,
  under a current output owner. Nothing in this module grants one.
- A notice is not an answer. It must not count as a completed reply, improve
  an answer-latency figure or be attributed to the lost request.
- Privacy, stop and a new admission still cut it, like any speech.
- Choosing whether to say anything is a product decision. Staying silent is
  valid. Saying the same notice at every reconnect is not: V1 debounced its
  rate-limit notice for 300 s.
- Publish `capability()` in a status report so a missing notice is known
  before the outage in which it would have been needed.

## Offline: the lamp says it cannot reach the internet (#830)

With `LAMP_OFFLINE_VOICE=on` in a serving voice's environment (and
`--failure-notice-dir` / `--failure-notice-language` as above), speech heard
while the provider link has been down for 5 s or more is no longer dropped:
it is admitted as a **local-only turn** (`commit_offline_input`). The turn
has an owner, is listened to and ended like any other, and the ring shows
the offline breath (ring-choreography.md); but its request is failed locally
from the start (`CloudRequest::local_only`) and nothing of it goes to the
provider (`queue_input`/`queue_input_end` send nothing for it). When it has
settled, `select_failure_notice_in` chooses:

- the `provider_unavailable` recording ("Sorry, I can't reach the internet
  right now. I'll be back soon."), once an outage, only if what was heard
  was speech: not a sound let through (`LAMP_SHORT_SOUNDS`) and not a scrap
  too short to be a request. A cough during an outage does not make the
  lamp apologise;
- nothing for any other try in the same outage: the breath alone shows it
  was heard.

The line counts as said only once the speaker has taken some of it (the
turn's playback has begun) or it has retired, never when it is merely
chosen: a turn cancelled before that (a Stop) leaves the line owed, and
"back online" is owed only after a line that was said (the reviewer on
#884). A block accepted and cancelled within one loop of the coordinator is
missed, and the line may then be said once more: it errs toward saying it,
not toward silence. The outage ends when the link is ready again; the next
outage says the line again. The recording goes through the same speaker path, permits and
retirement as `turn_lost`. With no recording provisioned the turn is
`Skipped` and the lamp is silent, as before.

**Back online.** When the link is ready again after an outage in which the
line was said, "I'm back online." (`back_online`) is owed for 60 s
(`BACK_ONLINE_OWED_US`). It is said once by a turn the lamp takes for itself
with no input (`commit_back_online`, local-only like the offline turn), only
when nothing holds the lamp (no turn, nothing queued or pending, not paused,
the microphone allowed) and the room is quiet as for a reminder. Someone
answered first, a new outage, or the minute passing let it go unsaid. Both
recordings are reported in `failure_notice_capability` (`offline`,
`back_online`). The other-language rule needs speech-to-text and does not apply
with no internet; only the local sound rule does.

## The model service refusing (#4265)

With `LAMP_SECOND_REFUSED_HOLD=on`, a turn that fails because the model
service refused it or could not be reached (`coordinator/model_refusing.rs`:
a quota, key or rate refusal, any other 4xx, a 5xx, a refused or timed-out
connection; never a turn that was only slow) starts a refusal: a quota or
key refusal at once, any other on the second refused turn in a row, and never
a turn cut while its answer was already playing. Until the model answers a
turn again (its generation completes; an onboarding line's own words, said on
a failed chat, #4263, are no answer), speech is admitted as
a local-only turn, as in a service-down outage below: the
`voice_service_unreachable` recording once a refusal, the offline breath,
nothing sent. A minute after the last refused turn, the lines are sent
again to find the service back, until one's chat is refused and holds the
turns for another minute. A line sent then that asks the model nothing (its
speech to text heard no words, or it was no speech) is no try: on
lamp-52e6 on 30 Oct the room's short sounds took each minute's try, and
every ask after them was held. The
journal says each kind once a refusal, and the voice's health warns with
the kinds and minutes (`bus_out::model_refusing`). The fault carries no HTTP
status, so a 400 counts with the other 4xx. Setup waits too
(`Onboarding::set_model_refusing`): no step is begun while turns are held,
the one being led is not failed. A step offered as an invitation (privacy,
how_you_sound, where_you_sit's opener) is re-offered each minute, that
invitation being the minute's try; a step asked as a note (who_you_are,
preferences, touch, where_you_sit's ask) is asked again only after its 90 s
give-up and quiet, twice at most. A step that would end failed or not
available meanwhile is held as `later`, offered again only 30 minutes on,
so a long refusal leaves setup's steps `later`. The model's own answer ends
the refusal; an onboarding line said in its own words (`OwnWordsSaid`) does
not. A skill's say (the lead's call on #1711): while turns are held, one
with fixed words (`text`) is said as they are with no chat asked and ends
nothing; a `note` or `quoted` line, which the model must word, waits as a
spoken line does, for the minute's try or its own expiry
([speaking-first.md](speaking-first.md)). On in the product
(`deploy/product/lamp-voice.service`); off unless set.

## Service down: the internet answers, the voice service doesn't (#830)

With `LAMP_SERVICE_DOWN_LINE=on`, beside `LAMP_OFFLINE_VOICE=on` and
`LAMP_REACH_PROBE=on`, a link down past its 5 s grace while
`lamp-health --reach-probe on` finds the internet answering (a relay outage),
and nothing else able to answer (no second route answering), is its own
outage (`service_down_now`). Speech heard then is admitted as a local-only
turn exactly as offline speech is, with the same breath, instead of being
dropped without a word (`accepted_without_cloud`). It says the
`voice_service_unreachable` recording once an outage, only for speech, by the
same "said once the speaker took some" rule; other tries get the breath
alone. No "back online" follows it: the internet never left
(`Outage::service`). Proposed words, for the owner to approve: "Sorry, I
can't reach my voice service right now. I'll be back soon." With no
recording the turn is `Skipped` and silent. Reported in
`failure_notice_capability` as `service_down`. Off unless set.

**Ship the recording only with a voice that knows the kind.** The manifest
is read whole: a `lamp-voice` from before this kind, given a manifest that
lists `voice_service_unreachable`, refuses every notice (`InvalidManifest`),
the offline line included. `back_online` was the same. So a manifest with
the new kind goes onto a lamp only with, or after, the build that knows it.

**Unlike offline, the internet is up here.** With
`LAMP_STOP_WORDS=heard-always` and a second route that is set but not
answering, speech over the service line's playback can reach the second
voice's speech-to-text, as speech over any of the lamp's speech can; that is
inside heard-always's own scope.

## Signed out: the lamp's login is no longer accepted

With `LAMP_SIGNED_OUT=on`, in `lamp-voice`'s and `lamp-cloud`'s environments,
a lamp removed and added again says why it has gone quiet. `lamp-cloud` decides
(`cloud.md`, "Signed out") and says so on `account/state`; the voice reads it
once a second (`coordinator/signed_out.rs`). A spoken reply needs that same
login, so the words can only come from a kept recording: "I've been signed out.
Please set me up again in the app."

- **Once a spell, with nothing asked, to someone at the lamp.** A turn the lamp
  takes for itself (`commit_own_line`, as "back online" is), when someone is
  there or spoke just now (`reminders::someone_here`), nothing holds the lamp,
  the microphones are allowed and the room is quiet as for a reminder. So it
  isn't said at night or to an empty room: if nobody comes within a minute
  (`UNASKED_OWED_US`) it is let go, and the line said when someone talks covers
  the rest. It counts as said once the speaker has taken some of it or it
  retired: a Stop before that leaves it owed. A state that goes missing and comes
  back in the same spell owes nothing more. A spell already 5 minutes old when the voice first reads it
  (`SAID_UNASKED_WITHIN_S`) is a voice restarted in it, and is not said again
  unasked; `lamp-cloud` keeps the spell's start across its own restarts.
- **Again when someone talks, at most once in 5 minutes** (`HEARD_LINE_EVERY_US`).
  Each line of speech heard is a local-only turn; the first in that time says
  it, so a TV beside a lamp that can't sign back in doesn't get it after every
  sentence. Other lines, a sound let through and a scrap too short to be a
  request get the offline breath alone.
- **Nothing is sent.** No turn goes to the service, and nothing the lamp starts
  itself (a reminder, a setup step, a skill's ask) begins: they wait as while
  paused. Unlike a refusal (above) there is no minute's try: only `lamp-cloud`
  ending the spell lets turns go again.
- **Ends** when `account/state` says signed in, or stops being said: a stopped
  or older `lamp-cloud` never holds a turn. No "back online" follows.
- With no recording, a person who spoke gets the sigh below, under the same
  bound, and the lamp's own turn is silent and let go. `failure_notice_capability` says `signed_out`. The
  voice's health warns while it lasts (`bus_out::signed_out`).

**Not yet on a lamp.** No clip is rendered or pinned: a notices release with
`signed_out` goes onto a lamp only with, or after, a build that knows the kind,
as for the kinds above. In the owner's language the line comes from the speech
cache when it was kept before the login was refused, and from the file
otherwise. Off unless set.

**What is not stopped:** with the Live route, the provider link's own
reconnecting; and the speech cache's retries of lines it has not rendered, which
stay bounded as they are. The product's second route opens its connections only
as a line begins, so a held line opens none.

## Verified, and not

Host, `crates/audio/tests/it/notice.rs`, 7 tests: an exact load; every
manifest-level failure; test assets refused in production; one faulty asset
refusing only itself; format, length and silence checks at both bounds; a
linked asset; malformed names and text.

Not done: native qualification of the optional live playback integration.

## The product's recordings (#1844)

Four lines, `en-US`, `approval: production`, worded by the first builder
(16:29 on #1844) and rendered by El on lamp-06c1 with #2300's
`render_line_second` in the voice every lamp now speaks with: the second
route's ElevenLabs `eleven_v4_turbo`, its default voice (the manifest names
it Rachel), 24 kHz mono 16-bit. A first render in Live's voice (Kore) was
held, since a lamp that goes offline would suddenly sound like someone else
(the first builder, 17:46). The person at lamp-06c1 listens to them before
the install change merges.

| kind | words | file |
|---|---|---|
| `provider_unavailable` | "Sorry, I can't reach the internet right now. I'll be back soon." | `provider-unavailable.en-US.wav` |
| `back_online` | "I'm back online." | `back-online.en-US.wav` |
| `voice_service_unreachable` | "Sorry, I'm having trouble thinking right now. Give me a moment." | `voice-service-unreachable.en-US.wav` |
| `wifi_lost` | "I can't reach any Wi-Fi. If you changed your Wi-Fi's name or password, push a paperclip into the hole marked reset on the back of my base and hold it for five seconds. That erases me and starts setup again." | `wifi-lost.en-US.wav` |
| `wifi_recovering` | "I can't reach my Wi-Fi, so I've opened setup. Set my Wi-Fi again in the app." | `wifi-recovering.en-US.wav` |

The `wifi_lost` words above are the new ones (#3743), naming the RESET
pinhole on the back of the base. The clip `wifi-lost.en-US.wav` pinned in
`deploy/product/models.json` still has the old words ("If your Wi-Fi has
changed, hold the reset button for five seconds."). A lamp plays the clip, so
the new words arrive from the next notices release. The new line renders at
13.37 s in the lamp's voice, over an older loader's 10 s, so that release goes
out with or after the build that allows 15 s (above).

**In the owner's language (#4473 (c)).** On a lamp whose owner's language
is not English (second-voice.md, "The fixed lines"), `provider_unavailable`,
`voice_service_unreachable`, `back_online`, `wifi_lost` and
`wifi_recovering` are said from the voice's speech cache in that language
when it keeps them (rendered at start in the lamp's voice, read again each
time the line is chosen), and from the files above otherwise. The release
and its manifest are untouched and no kind is added: the cached line is
recorded under the release's kind, its `wav_sha256` `speech-cache:<key>`.
`turn_lost` has no words of its own and is unchanged. An English lamp plays
the files exactly as before.

`wifi_recovering` (#3743, the words agreed with Sixth on #3020) is new: no
clip is rendered or pinned yet. It is under 10 s, but its kind is new, so its
release waits for a build that knows it (above). Until then
`lamp-setup recover` asks for it, the voice answers `not_provisioned`, the
journal says so and nothing is heard.

They and `manifest.json` are in the release `notices-2026-10-20`, pinned by
size and SHA-256 in `deploy/product/models.json`. `lamp-update install`
places them in `/opt/lampOS/voice/notices`, the folder the product unit's
`--failure-notice-dir` names. A lamp moved from V1 gets no install, and its
package carries no notices, so `lamp-keeper prepare` places the same five
files, by install's own fetch and check, in `/var/lib/lampOS/voice/notices`
(#2943, [keeper.md](keeper.md)): only a missing one, and a failed fetch is
said, never a refusal. The generic voice unit (`deploy/units`) names that
folder, in the product unit's language, held to both by `lamp-update`'s
unit test. Not the product's folder: the package's packager takes every
`/opt/lampOS` path in a unit for one it carries. The manifest lists every kind, so it goes only
onto a voice that knows them all: `wifi_lost` is known from `a441fea06`,
build 4142 on.

## When a line is missing: the lamp's sigh (#2943)

With `--failure-notice-dir` given and a line that can't be loaded (no
folder, no manifest, not provisioned, or a file that fails its check), the
lamp no longer meets a failed answer with silence. Seen on lamp-dbda on
22 October: 26 lines admitted, every turn `provider_failed`, every notice
`no_directory`, nothing said. In the missing line's place it plays its own
sigh, the hand's sleep breath (`head_cue::breath`), at `FALLBACK_DBFS`
(-26 dBFS, under its speech, not yet tuned by ear). It goes through the same
selection, permits and cancellation as a recording. That holds for the
failed turn's line (`turn_lost`), the offline line and the service's line;
"back online" is never stood in for, since nothing failed. The evidence says
it: `failure_notice_selected` carries `"fallback_for":"<the missing
reason>"`, and `failure_notice_capability` carries `"fallback":"sigh"`. With
no `--failure-notice-dir` nothing stands in, and the lamp stays silent as
before. The sigh says only that something is wrong, not what. The recorded
lines, which say why, are still the answer: the product's come from
`notices-2026-10-20` by install, and a moved lamp's by `lamp-keeper prepare`.

## Optional same-owner live integration

`lamp-voice directed` and `lamp-voice serve` accept the paired options
`--failure-notice-dir ABS_PATH --failure-notice-language TAG`. Both must be
explicit. Omission preserves silent failure behavior. Fixture-provider and
child-worker commands reject these options. This integration selects only
`turn_lost` in the exact requested language, requires `approval: production`,
and loads it once before starting audio workers. It never synthesizes,
downloads, resamples, or opens a second speaker. A missing or refused recording
is visible in `failure_notice_capability`; active ticks perform no asset I/O.
The production recordings aren't in this source; see "The product's
recordings" above.

A validated failure of the current owned request may select one notice only
after the real input endpoint and retirement of all prior provider audio.
A failed Start does not become a fake provider `Started` receipt: local notice
PCM has a distinct origin. Provider PCM retains its existing Started check.
The original Controller speech plan, short permits, speaker writer and exact
acceptance/final-retirement receipts govern both sources. The notice creates
another playback occurrence under the same owner, never another model request.
Its attempted latch is set before transport dispatch. A failed or ambiguous
send is not retried. No worker or cue queue is added.

`failure_notice_selected`, `failure_notice_attempted`,
`failure_notice_first_write` and `failure_notice_final_sample_retired` describe
local notice delivery. Selection grants no output authority; ALSA acceptance
and retirement are not proof of audibility. Final notice retirement must match
the fully accepted final chunk, playback epoch and sample cursor exactly. The failed turn retains
`provider_failed`; notice audio is excluded from provider arrivals, answer
first-write events and answer gap counts. Raw playback gap evidence is retained
with its output origin while the matching reply is retained, and notice gaps
are recorded separately. Late gaps after owner retirement remain raw evidence
with unknown origin; they cannot taint a successor’s answer metrics. Cancellation
retains the selected/attempted evidence rather than treating the notice as an
answer. Observation loss cannot authorize playback or repeat the notice.

Privacy, Stop, hard authority expiry and a new admitted owner invalidate old
notice permits just as they invalidate an answer. A cancelled notice is never
restored after unmute, a later owner or process restart. This is at most one
local notice attempt for a still-owned failed turn, not exactly-once audible
speech. The existing baseline own-playback mask still suppresses prospective
input over the notice; this change does not qualify natural barge-in.

Already-unavailable input has no TurnOwner and receives no notice in this
batch. A successful complete reply, idle link event, old-owner failure or
reconnect alone does not trigger speech. A missing asset produces one skipped
result and permits normal failed-turn retirement. The local scheduling target
is first notice dispatch within 20 ms after all eligibility conditions hold
(real endpoint, prior retirement, available owner and no pending chunk),
excluding the failed provider wait. This is a target, not a measured hardware
latency; the active path adds no file/network I/O or deliberate delay. Mandatory audio/privacy/
reference faults still terminate safely. Terminal provider output still ends
serve after retained obligations settle; provider replacement and truthful
fresh-context recovery are separate work. No new readiness promise is made.

Qualification for this integration uses test-only PCM, the production
coordinator/dispatcher and local IPC. Test PCM is never a production-approved
asset. Hardware audibility, native cancellation timing, approved voice quality
and production asset provisioning remain unverified until separately measured.


## Integration verification, 2026-10-11

The composed source was checked from a source-only standalone export on macOS
and Linux/ARM64 emulation. The proposal's host gate passed 221 focused tests;
independent review added regressions for cancellation between chunks, late
acceptance after cancellation, and provider audio followed by one notice through
the real baseline conversation loop. The combined ARM gate passed 229 selected
tests: coordinator 189, options 17, playback 9, fixture provider 2, notice loader
7, and process/privacy 5. None failed or was ignored in this selection. This is
an affected-path gate, not a new full-workspace or real-Lamp result.

Commands, from the exported `lampOS` root (Rust 1.96.0):

```sh
cargo fmt --all -- --check
cargo clippy --locked --offline -j2 --target aarch64-unknown-linux-gnu -p lamp-voice -p lamp-audio --all-targets -- -D warnings
cargo test --release --locked --offline -j2 --target aarch64-unknown-linux-gnu -p lamp-voice --lib coordinator:: -- --test-threads=1
cargo test --release --locked --offline -j2 --target aarch64-unknown-linux-gnu -p lamp-voice --lib options:: -- --test-threads=1
cargo test --release --locked --offline -j2 --target aarch64-unknown-linux-gnu -p lamp-voice --lib playback:: -- --test-threads=1
cargo test --release --locked --offline -j2 --target aarch64-unknown-linux-gnu -p lamp-voice --lib fixture_provider:: -- --test-threads=1
cargo test --release --locked --offline -j2 --target aarch64-unknown-linux-gnu -p lamp-audio --test it notice:: -- --test-threads=1
cargo test --release --locked --offline -j2 --target aarch64-unknown-linux-gnu -p lamp-voice --test it -- process:: privacy_state:: --test-threads=1
cargo build --release --locked --offline -j2 --target aarch64-unknown-linux-gnu -p lamp-voice --bin lamp-voice
```

The 407-file source manifest is
`38f9f212bd1701129cb4e36df89d4f1316bbe7961e4ce0cf22e7ba3eeb313c5e`.
The ARM receipt is
`172900fda694afb5826b217db2af5678246a33e252e070510a84c87284ec683c`;
private logs are under `/private/tmp/lamp-notice-integrated-2evnkvbr/arm` on the
integration Mac. The resulting 8,678,808-byte AArch64 binary is
`e37505d747f6aca3f4e86fbd5f04d12569eb6068d93dc09f60523f918f0ecba2`.
Source hashes were checked before and after the gate; extraction matches the
built artifact. Documentation was updated afterwards. This artifact has not
been deployed by this checkpoint. The installed stopped pilot still uses the
previously qualified `738503a803d3...` binary. Notice speech remains disabled
until a real approved voice asset is provisioned and the native path is checked.
