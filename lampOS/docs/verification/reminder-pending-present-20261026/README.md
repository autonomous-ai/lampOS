# Ask again when pending reminder work changes

The Reminders skill asks once on first eligibility, selected queue change or
restored eligibility. Unchanged held heartbeats do not repeat requests.
Voice owns occurrence selection and delivery; a skill opportunity is not
permission to move or proof that words were heard.

## Source and gate history

The original positive-edge failure after a conversation is retained in
`negative.json` and `negative.patch`. The initial `present` repair passed
full Skills 526/0/9 (`fixed.json`) but had two review faults: a reply-delay
fixture race and sixty requests for sixty unchanged held heartbeats.
`review-fixture.json` records the explicit-release fixture correction on its
original source. `held-negative.json` reproduces the request churn on
unchanged `present` production at current0c1.

The initial change-trigger implementation passed full Skills 535/0/9
(`changes-initial.json`) but an additional source review found that an
invalid selected predicate or choice could create a false eligibility edge.
`selected-negative-initial.json` retains one intended predicate failure and
one fixture-name setup error; the latter is not a reproduction.
`selected-negative.json` records both intended failures after the fixture
name correction. The final fix ignores malformed selected inputs while
allowing a genuine false predicate, empty queue or independent conversation
to rearm. `changes-final-0c1.json` records full Skills 539/0/9 with that fix.

Final qualification is on **1c884f16c** plus the frozen task patch:
`final-1c884.json` binds formatting, strict Skills all-target Clippy, workspace
build, full Skills **542 passed / zero failed / nine ignored**, eighteen
focused integration controls and two parser controls. The intermediate
current-base 539 gate is retained separately. `while-negative.json` and
`scalar-negative.json` observe acceptance of configurations now refused at
load: selected/same-topic-while path overlap and compound trigger comparison.
Those are parser-acceptance negatives; the related malformed-data rearming
was a source finding, not a separately executed runtime claim.

The eighteen reminder integration controls, two parser controls and all-recorded-
skills control each pass once in the final full suite. Focused counts overlap
that suite and are not added. Source manifests and actual executables were
preserved before the next compilation. The four recordings plan desk 2,
after-talk 2, each-reminder-once 6 and unavailable 2 calls; these are not speech
or movement counts. The explicit-release runner fixture proves two busy
refusals before its expired reply and no look/light without Voice-begun.

## One-build-apart update checks

An old parser rejects the new `changes` rule. The old-daemon reload probe
observed both skills loaded, only Reminders removed on the new manifest, and
both restored on the byte-exact old manifest in the same process. The child
was terminated and reaped; no Voice or device was run. `compatibility-cli.json`
records the final new parser listing both skills and replaying four cases
with each of the new and old reminder definitions.

`update-crossings.json` records three passing controls using hash-pinned
actual old/new native parser executables: bundle-only update refusal before
writes or host actions; complete new-parser/bundle switching; and rollback
restoring the old parser and exact old bundle. The new files/link are observed
before the forced rollback failure. Four existing product-skill Update
controls also pass, separately counted. Formatting and strict Update round
Clippy pass. The Update library, signatures and filesystem work are real;
service lifecycle, health, clock and channel are simulated. A private,
nonportable test overlay supplies exec-only parser adapters and is restored
byte-exact afterwards; no such overlay or binary ships in this commit.

The complete update can expose new files to the old daemon before restart;
there is no atomic deployment or uninterrupted skill-look claim. Normal
rollback restores files before restarting the old binary, and crash recovery
requires Update to run again. The existing Voice ten-second wait concerns an
eligible offer, not a guaranteed end-to-end delivery bound.

## Limits

Native Mac and native Linux results keep separate source/run identities.
No physical latency, actual words, gaze, or live rolling-update acceptance is
claimed. First owns corrected-build replay for the shipping reminder faults.
#3523’s interruption-before-retirement repair is a separate runtime change.
Source-ask eligibility remains capped at 20 seconds, while the socket's 180-second
per-read timeout and connect without an explicit deadline do not bound total
run duration. No worker, I/O, timer or model round trip is added by this rule.
