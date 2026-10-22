# Known-face name: product preparation, 22 October 2026

Part of #2539. The feature implementation #2573 is already merged and
remains off by default in the binary. This draft proposes one product
setting: `LAMP_SECOND_FACE_NAME=on`. It adds no function declaration and
changes no runtime code. It does not enable the different speech-to-text
setting `LAMP_SECOND_KNOWN_NAMES`.

Ready for lamp-voice: no

## Required acceptance

The issue requires First's lamp-4ace run with its person: switch on and off,
tell a name and consent to keeping the face, look away, return and ask the
name, then room and off-camera controls. Fourth has no lamp and has taken
no device action. First's review and this person run must precede product
activation. The memory build row remains 0.40; no host check raises it.

The speaking-face path narrows a positively detected other speaker to the
room. When no face reads speaking, it leaves the admitted scope unchanged
(`crates/voice/src/speaking_faces.rs`); an off-camera speaker can therefore
inherit the visible person's name. This proposal does not repair that
scope limitation. A known file can also carry a just-forgotten name for
up to three seconds. Both boundaries remain part of the required review
and lamp controls.

## Immediate feedback and latency

There is no new capture, output or input handling path. A second-route
turn already admitted for a person reads the local eyes known-face file
and appends a name note only if the fresh observation matches that
person. No network request is added. The existing helper returns early
when off; the feature's file read remains synchronous on the worker.

The host target is less than 5 ms from `known_name_told` entry to return,
including file open, metadata/bounded read, freshness/name parsing, person
mapping/comparison and note formatting. The existing test measures the
first call and immediate repeat with a freshly written synthetic file;
there is no wall-time assertion. This is neither a cold filesystem
measurement nor provider, acoustic, or device end-to-end latency. Slow
storage and the person run are unverified.

## Product prerequisites

The product voice already sets scoped context and speaking-face tracking,
its memory socket, and its eyes who socket. The worker derives the eyes
known file beside the who socket when memory is configured. The product
eyes already publishes matching known-face and speaking observations.
No extra core-memory flag is needed. Private host contract checks read
the actual two units and require the exact flag once, the existing scope
flags, matching eyes socket/known-file arguments and memory socket.

## Qualified check receipt at 19:29

This receipt belongs to source `a77af5e85acffd138a2025f4cee68a6e85bb0709`, parent
`8e274b5e394b09ef5e5f18cb0d2382fbaddec95a`, draft #3014. It is
published on `lamp-fourth` to retain the exact qualified feature head.
The feature commit's README records its earlier draft-publication status.
No source, unit or test was changed after the completed scoped gate.

- Local: **656 passed / 0 failed / 0 ignored**, eight disjoint existing-test
  commands. Formatting, strict workspace/all-target Clippy and workspace
  build pass. Exact commands, five source/unit/reference hashes, timings
  and log hashes are in [local-receipt.json](local-receipt.json).
- Helper entry to return: first **169 us**, repeat **43 us**, each one
  call/one note. Both raw occurrences were captured, including the first
  prefixed by the Rust harness. Only the synthetic host boundary above
  was measured; no cold/slow storage, provider, device or acoustic claim.
- [ARM64 39774976573](https://github.com/autonomous-ai/lampOS/actions/runs/39774976573):
  **10,450 passed / 0 failed / 67 skipped**, zero selected unrun,
  185.277 seconds in nextest. Four doctests, twelve separate process
  checks and relay signals pass. One unchanged same-voice leaky pass;
  no CI rerun. Known-name, installed-unit and worker-role controls have
  explicit native PASS lines in [ci-receipt.json](ci-receipt.json).
- [Clippy 39774976453](https://github.com/autonomous-ai/lampOS/actions/runs/39774976453):
  green, first attempt. API/current PR head and both run heads agree.
- Actual CI base `8e274b5e394b09ef5e5f18cb0d2382fbaddec95a`, checkout
  merge `63277593fc2e421d394cbf42ef785b933ef655df`.
- [Artifact 12425542033](https://github.com/autonomous-ai/lampOS/actions/runs/39774976573/artifacts/12425542033),
  `lampos-arm64-a77af5e85acffd138a2025f4cee68a6e85bb0709`. API and upload-log SHA-256 agree:
  `af37f3321537a9383efb4168fb727f85cbbfc710f1b68b2e825e1b752640ca8f`.

The earlier interrupted draft had fmt only and contributes no completed
Clippy or test pass. Raw evidence remains in the private
`fourth-face-name-product-checks-20261022` and
`fourth-face-name-product-ci-20261022` folders. The committed JSON receipts
retain exact source, counts, commands and hashes without raw journal words.

Ready for lamp-voice: no

First's review and the authorised lamp-4ace person row remain open;
Friday owns the following 20-run EN/VI product probe. Existing off-camera
and just-forgotten-name limits above remain explicit. No product
acceptance or memory-row score increase follows from these checks.

## Controlled model probe, 19:33

[Friday's source-bound probe](https://github.com/autonomous-ai/lampOS/pull/3014#issuecomment-6340799154)
uses a local request fixture at a77, a freshly written known file,
synthetic memory snapshots, the relay, and 13 functions. Each arm has
20 English and 20 Vietnamese requests. It is a model/context measurement,
not an installed product request/tool census, actual face/speaker capture,
or an acoustic/device result. It does not replace First's person row.

- Known person's correct name: switch on38/40, off0/40.
- Unknown face and explicitly narrowed other speaker: known name0/80.
- No speaking-face evidence, scope retained for the visible known person:
  known name36/40. This is the documented open wrong-person boundary,
  demonstrated in the request fixture, not an Engine/device reproduction.
- The setup-name placeholder also reaches unknown/narrowed turns; that
  comes from the existing character wording, unchanged by this switch.

First's privacy/release decision and person row remain required. A full
product-build probe after that row remains distinct from these synthetic
request fixtures. No score increase or product acceptance is inferred.

First's 22 October19:33 decision
([6340841769](https://github.com/autonomous-ai/lampOS/issues/2602#issuecomment-6340841769))
accepts the controlled probe and holds merge until the owner's face is
re-recorded at10:00 on23October and the owner's name check passes. Two
face-recording requests were refused in today's session. This is a
scheduled person gate, not a successful run. The0/80 result concerns the
unknown and explicitly narrowed arms; the no-speaker inherited-scope
36/40 remains an explicit limitation, without an Engine/device claim.
No additional probe is assigned to Friday here; First has given Friday
the separate setup-name scope correction. Product acceptance and score
remain unchanged.
