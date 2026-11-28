# Sensor 5: preparing the Monday lamp-footage pilot

This is the offline preparation for [#8848](https://github.com/autonomous-ai/lampOS/issues/8848).
The owner approved the [240-clip protocol](https://github.com/autonomous-ai/lampOS/issues/8848#issuecomment-7227484417);
the [team lead confirmed preparation and the person split](https://github.com/autonomous-ai/lampOS/issues/8848#issuecomment-7235605689).
Only the team lead operates an allocated free lamp. These tools start no service,
capture, playback, training or integration by default. The speaking-face sensor
stays off. A source build or a synthetic test is not permission to install it.

## What is reused

The published AVA evaluation found that fitting either released model increased
speaking recall at a substantial cost in silent-face rejection. The Monday pilot
uses frozen weights and existing windows: Light-ASD T50 (two seconds) and TalkNet
T25 (one second). It tests the actual desk/video situation with labelled local
footage. It does not train either model. Published thresholds are reported first;
any lamp-specific threshold fit uses the tuning people only and is locked before
test results are opened. The separate AMI check reuses the AVA-fitted thresholds
without fitting on AMI.

The [AMI second-set EVAL](https://github.com/autonomous-ai/lampOS/issues/8848#issuecomment-7236189292)
did not meet the 95% bar. On the common complete inputs (A55/B104), the frozen
AVA-fitted settings gave Light-ASD 50/55 speaking recall and 71/104 silent-face
rejection; TalkNet gave 48/55 and 51/104. The EVAL keeps all planned denominators
and unknowns separately. AMI uses a transcript/camera proxy, not adjudicated lamp
footage; it does not qualify either model for this pilot or for release.

Use FFmpeg/ffprobe for reversible offline decoding and lossless synthetic fixture
generation; use native PNG timestamps rather than deriving time from frame index.
Use a static HTML labeller so no private media requires a service, cloud account
or upload. Hardware capture must reuse the existing Eyes and Ears permission
owners. A Controls observation alone does not grant camera/microphone access;
see the [privacy path audit](../tools/sensor5-pilot/privacy-guard/README.md).

## People, consent and the fixed split

Four distinct consenting team members fill these slots before any recording:

| Slot | Role | Background playback speaker |
|---|---|---|
| P01 | Tuning | P02 |
| P02 | Tuning | P01 |
| P03 | Test | P04 |
| P04 | Test | P03 |

The alias-to-person map is local only. Nobody is moved between slots after
capture or because of results. The playback voices stay within their split too.
The team lead arranges the actual people; the tool cannot establish that aliases
represent distinct people or verify a person's consent.

The consent line is:

> I voluntarily agree to recording my face and voice for the sensor 5 pilot,
> including local background playback of my source video, and indefinite local
> retention of recordings and derivatives for this evaluation; identifiable
> media and private labels will not be uploaded to GitHub.

The generated full form explains access, retention and stopping further capture.
It defaults to **Consent accepted: no**. Obtain the participant's actual
confirmation, retain the signed/confirmed text locally, and record the real UTC
acceptance time. No tool supplies consent on a person's behalf.

From `lampOS/`, create a new local plan (Node 20 or newer):

```sh
node tools/sensor5-pilot/schedule.mjs --out /absolute/new/private/plan
```

This writes the frozen protocol, 240 opaque clip IDs, per-clip operator staging,
split, empty private identity template, four unsigned consent forms and a checksum
manifest. It opens no device and plays nothing. Preserve this original plan;
keep completed identity/consent records in a separate private directory rather
than overwriting its hashed blank forms.

After actual consent, make the native recorder's compact record:

```sh
node tools/sensor5-pilot/consent.mjs \
  --text /absolute/private/P01-confirmed.txt \
  --participant P01 --operator O01 \
  --accepted-at ACTUAL_UTC_RFC3339_TIME \
  --out /absolute/new/private/P01-consent.json
```

The output binds the accepted record to the exact text hash and aliases. Blank
forms, mismatched aliases, missing confirmation, duplicate acceptance and
overwrites are refused. This is evidence of operator-supplied consent, not
biometric identity verification. Keep the text and JSON together.

## Capture conditions and accounting

Each person records five A/B pairs at each of 0.6, 1.0 and 1.5 metres, under
daylight and lamplight. Each clip lasts ten seconds. A is audible speech from the
visible participant across the five-second midpoint; B is that participant
silent with natural movement while another consenting teammate speaks in the
nearby video. The screen stays outside the camera view. Matched A/B pairs use
the same playback level and source segment; playback runs during A too.

There are 120 A and 120 B clips: 40 minutes, plus four 30-second playback sources,
42 planned recorded minutes total. Each split contains A60/B60. Allow about
90 active minutes across the lighting sessions. The operator checks the private
schedule row before every take. Intended staging is not ground truth.

Only full camera frames and microphone data from the authorized capture owners
may be retained. Never stop an unrelated service, take over a permit socket,
impersonate an existing camera reader or use an unguarded direct-device FFmpeg
command. Direct live FFmpeg remains refused. Synthetic capture is available for
testing the preparation/labelling workflow without opening hardware:

```sh
bash tools/sensor5-pilot/capture.sh --help
bash tools/sensor5-pilot/prepare-labels.sh --help
```

The native source candidate is
[`lamp-sensor5-recorder`](../tools/sensor5-pilot/privacy-guard/README.md).
Its guide gives the private link preparation and exact recorder arguments.
After two privacy reads and separate operator setup, use `--seconds 10` for
each scheduled main clip and `--seconds 30` for a playback source. It accepts
only the dedicated service identity and accepted local consent, waits for
50 ms of continuous valid Voice authority, and then starts the requested
capture interval. It grants no permissions and installs no service.
The source candidate also needs Ears' explicit
`LAMP_BUS_AUDIO_CAPTURE_PERMIT=on` alongside its existing privacy-gated PCM
export. This carries each block's original clock, source era and deadline;
the recorder refuses an old/default export without that metadata. Neither
switch changes a product unit. A later Voice heartbeat cannot extend the
original block's retention deadline.

Prepare a completed native archive locally:

```sh
bash tools/sensor5-pilot/prepare-recorder.sh \
  --archive /absolute/private/S5-0001-take1 \
  --clip-id S5-0001 --out /absolute/new/private/S5-0001-preview1
```

This checks the exact inventory and terminal counts, retains all observed
frames, and produces one `segments/segment-NNNN` folder per continuous PCM
segment. Open a segment folder in the labeller. Audio-only segments remain
retained without a loadable manifest; `segments.json`, `all-frames.json` and
`unknown-frames.json` expose gaps and unmatched frames. Every native preview
says **unverified alignment** and `scoring_allowed:false`. The adapter does
not authorize scoring or erase the original timestamps. It supports the
pilot's 10/30-second recordings; longer or incomplete archives are refused.

Every take has a fresh private directory. No original or failed attempt is
overwritten or deleted. Keep source timestamps and clock domains, frame/audio
sequences, gaps, authority metadata, process status and hashes. A driver timestamp
and a microphone block's read-completion time are different measurements.
Receipt/dequeue alignment is an explicit preview estimate, not measured A/V sync.
Before scoring actual footage, retain a separate pre-score alignment record:
operator and Reviewer check visible/audible anchors at both ends, recording
offset, residual and uncertainty. Unsupported, drifting or ambiguous alignment
remains unknown. Never choose alignment from model scores or hide gaps with CFR
conversion, frame duplication or PCM filling.

## Labelling locally

Open [the labeller](../tools/sensor5-pilot/index.html) directly in a local browser.
It uses no CDN, network request, server or browser persistence. Synthetic practice
contains generated shapes and silent audio. For a take, choose the prepared local
folder; PNG and WAV hashes are checked against its manifest before labelling.

Use native previous/next frame controls or a frame index, listen to the associated
audio, and enter a pseudonymous rater ID. Assign explicit no-face/uncertain scenes
or stable per-face track IDs and boxes. Each face can be speaking audibly,
speaking inaudibly, not speaking or uncertain; audible source labels separately
record visible/off-screen/playback/no speech/uncertainty. Every frame starts
unlabelled. Range assignment is explicit; the tool does not interpolate labels.

Team lead and Reviewer label independently without model scores, then reconcile
before inference. Export each rater's JSON into a new local file and retain all
versions. Imports must match the source capture, manifest, frame index/PTS and
media hashes. Disagreements and unresolved labels remain visible. The prepared
preview and annotations never replace original media or timestamps.

Score one predeclared five-second midpoint per main clip, using the same labelled
tracks for both models. Missing input, unresolved truth/alignment and model
abstention remain unknown in the planned A/B denominator. Report primary counts,
conditional complete-input diagnostics, unknowns, and breakdowns by person,
distance, lighting and native face size. At least 57/60 correct in **each** held-out
class (95%) justifies requesting a larger body/two-face/latency test; it does not
qualify a release. Two test people and correlated clips cannot support broad
generalization claims.

## Retention and validation

Keep every original frame/audio stream, playback source, derivative, failed take,
consent map, label version and scoring input on encrypted local storage outside
cloud sync. Access is limited to the team lead, Reviewer and Builder #3. README
and SHA256 manifests cover every archive. GitHub receives source, aggregate
results and hashes; no identifiable media, real names or private case joins.

Tool logic checks run with `node --test tools/sensor5-pilot/*.test.mjs`; shell
entry points are checked with `bash -n`. Synthetic tests cover VFR timing,
preserved A/V offsets, immutable outputs, failure/interruption retention,
permission refusal, media identity and explicit unknown labels. Native recorder
checks and operator commands are documented with its implementation.

Not shown by these host checks: real participant consent, hardware capture,
physical privacy closure, actual A/V synchronization, camera/microphone gaps,
human usability or audible playback latency, lamp-board inference timing or qualification.
These remain separate evidence gates; no simulator or host result substitutes for
them. Native live capture must stay refused unless its complete permission path
and operator slot are in place.

Host validation on 28 November used an isolated headless Chrome profile on
Mac 1, with generated practice frames and the native recorder's synthetic
archive. Folder selection, hash verification, label application, export and
re-import passed; screenshots were visually checked and the app made no HTTP
request. Practice loading took 64.68 ms and three frame steps 24.51–25.02 ms;
the 10-frame native fixture loaded in 23.35 ms. These are host DOM/image-ready
boundaries, not audible playback, hardware or board timings. An initial test
incorrectly expected confirmation to enable on an unchanged export; its
failure and successful corrected check are both retained. All tool tests use
synthetic data; no participant has been recorded.
