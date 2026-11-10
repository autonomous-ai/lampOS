# Bare kept-light claims: punctuation and streaming

Fourth carried stalled #5910 after posting the takeover claim 6751128287.
The two authored commits are preserved on base `a2260fd1b85` as
`cc8f5e29a1` and `1f5710603738c3e4bff924cb11a1f441ec803269`; both range-diff
rows equal the original `09111a5a99` / `d5931437c6` on `f61b92b86d`.
Twelve's branch is untouched. This receipt describes the carried repair's
native qualification; Linux CI and fresh reviews remain separate.

## Boundary and change

Streamed reply text reaches `claim_as` before pending speech is released.
A false claim can request another model round and ultimately say the action
was not done. A premature nonclaim releases the held speech permanently.
The repair therefore preserves uncertainty while a clock, decimal or suffix
can still continue. Evaluation itself targets under 1 ms on this host;
there is no new I/O, polling, debounce, task or timer.

The original punctuation repair used the new complete-time and closing
slots only for the three bare Keeping/Leaving runs. The duration-state follow-up
below extends these slots to state replies with "for". Generic time/end slots retain their rules. Numeric punctuation joins
only adjacent components; whitespace after a period preserves a completed
first sentence even when the next sentence begins with a number. Punctuation
tokens carry this adjacency flag, which existing matchers and limit parsing
ignore. Compact numeric suffixes retain the existing digit-start convention.
A bare comma stays uncertain until a following token or model completion;
a following descriptive word rejects these narrowly closed patterns.

A trailing numeric period with no following text may begin a decimal, so this
opening waits for lookahead or model completion. That extra streaming boundary
is not a measured end-to-end or device delay. It adds no timeout and changes
no product switch. The WORDED switch remains off in the product.

## Executed standalone evidence

Rust 1.96.0, optimized standalone binaries on Mac. The complete production
prefix of action_claim.rs (before its test module) is copied byte-for-byte.
Real serde_json is linked from the existing target; only unrelated
`tools::is_light_or_volume` and `rules::KEEP_RULE` dependencies are stubbed.
This executes matcher code, not Engine, package, acoustic or lamp acceptance.
The controller/source/logs stay outside the repository under
`/tmp/fourth-worded-matcher-probe/`.

The final probe has 1,483 assertions across nineteen complete replies, every
character prefix, switch-off equivalence and explicit ambiguous/decisive
boundaries. These are probe assertions, not 1,483 repository test cases.

- Original carried production: 176 failed assertions, exit 1.
- First repair draft: 38 failed assertions on the new sentence-boundary
  controls, exit 1. Source review found that digit tokens could be joined
  across whitespace; this failed draft is retained separately.
- Corrected production: zero failed assertions, exit 0.

Earlier 1,020- and 1,263-check probes passed the first draft but lacked the
sentence-boundary cases. They are not substitutes for the expanded probe.

Matched final-probe timing boundary is one `claim_as` call, with the first call
measured before other calls in that probe process, then 1,000 prefix calls:

| Source | First | Median | p99 | Maximum |
| --- | ---: | ---: | ---: | ---: |
| Original | 66.093 us | 79.554 us | 86.375 us | 142.197 us |
| Corrected | 67.484 us | 81.917 us | 93.815 us | 436.101 us |

Both are below the 1 ms host target in this sample. This is not process startup,
model completion, speech readiness, slow-service behavior or a device measure.

## Native gates and remaining limits

Two repository tests contain the full-string/prefix regressions. Whole-workspace
formatting and diff checks pass; documentation links pass (192 documents, 1,898
local links, zero broken; verification READMEs are outside the counted set).
Strict workspace all-target Clippy and the focused `action_claim::` filter
passed: forty-nine native tests, including existing Engine controls, zero
failures or ignored cases (0.71 s test body). The full Voice package also passed: 5,754 passed, 22 ignored, zero failures
(771.532 s command). Its library contributed 5,464 passed/16 ignored;
integration targets 279 passed/six ignored, binary one and examples ten.
Package docs contained zero tests. Focused cases are included, not added again.
Workspace build passed (106.408 s); current-head ARM validation remains pending.
The actual GO-159 replies and authorized replay remain needed.
Reviewer-lead's habitual-state finding and other previously recorded P3s are
outside this repair; no switch-on, ledger repair or device claim is made.

Native commands from `lampOS/`, Rust 1.96.0, DEBUG0 for dev/test,
incremental off, two build jobs, retained target cache and owned short TMPDIR:

```sh
cargo +1.96.0 fmt --all --check
cargo +1.96.0 clippy --offline -q --workspace --all-targets -- -D warnings
cargo +1.96.0 test --offline -q -p lamp-voice --lib action_claim:: -- --nocapture --test-threads=1
cargo +1.96.0 test --offline -q -p lamp-voice -- --test-threads=2
cargo +1.96.0 build --offline -q --workspace
```

The controller checks action_claim.rs and switches.md hashes before and after
each stage. Both remained unchanged through all five successful stages:

- action_claim.rs: `f66e8dfeb12ea9ef93fc4bc093acc171ff6be5a93d47c73f1e17889253dc926e`
- switches.md: `58900c2229367c4fffaf0aa028f48a66289e50cc46b64768ced5fcfdb8b99528`

Production-prefix SHA-256 values:

- `baseline_action_claim.rs`: `460c1c3320f26d0860aac8cc2b9c5021e705403536e71a5cb0b63d69fe07f071`
- `pre_adjacency_action_claim.rs`: `b398b4b2f5ccd5b162e1a3a7fe42d3ea03e0f8294735fa4f1b407180bfc931d8`
- `fixed_action_claim.rs`: `2bd6a7472c5526b00cfb69f9f553e0ef957038ca9aafb6360adcc0d29d28a9ae`


## Duration-state review follow-up (10 November, in progress)

Fourteen's non-blocking review 6756810932 reproduced explanatory state replies
being classified as kept-light claims. An executable built with Rust 1.96.0
from the original production prefix failed all three explanatory controls
(exit 101); the repaired prefix passes them and the new repository test's thirteen
explanations, every character prefix, strict-only controls and ten positive
claims (including their prefixes). These are standalone matcher checks, not a completed native gate.
Only the unrelated call-family helpers are stubbed; serde_json is linked from
the retained native target. No Cargo job overlaps the cancellation gate.

All twelve duration-state shapes now close at the time and optional unit,
using the existing complete numeric time and sentence-end slots. Commas and
clock/decimal punctuation cannot end the match early. Explicit "I kept" or
"I'll keep" claims and all "until"/"till" state runs are unchanged. The bare
"It remains on for half an hour." remains a claim: it has no explanatory
continuation. The same phrase followed by "after a tap" is no claim.

The same optimized host probe measured one first call and 1,000 streamed
prefix calls before and after this change (microseconds):

| Source | First | Median | p99 | Maximum |
| --- | ---: | ---: | ---: | ---: |
| Before duration closure | 79.193 | 85.546 | 148.279 | 271.822 |
| With duration closure | 170.301 | 139.138 | 242.187 | 662.156 |

Both samples stay below the 1 ms per-call host target. This measures only
`claim_as`, not model, speech, cold process startup or a lamp. The additional
patterns increase matcher cost; no new I/O or waiting timer is introduced.
The probe source and output are `/tmp/fourth-state-duration-timing-*`.
Package gates and current-head ARM validation are pending for this follow-up;
the successful native checks above belong to the earlier repair. Documentation
links pass: 192 documents, 1,898 local links, zero broken.


The earlier 1,483-assertion bare-keeping punctuation probe also passes against
this follow-up with zero failed assertions. An additional repository Engine
control covers the three reported explanatory replies both whole and split
immediately after the duration: each must match switch-off output, be said
once, run no call and request no retry. That Engine test awaits the native
package gate; no Engine result is inferred from standalone matcher checks.

An additional standalone differential probe passes 8,102 comparisons against
the original production matcher: full replies and every prefix, both ended
and streaming, across unchanged until/till state runs and explicit first-person
keeping/leaving duration runs. This checks preserved decisions, not new Engine
coverage or device behavior.

Twenty-one matcher module tests also pass in an extracted Rust 1.96.0 test
binary (0.85 s). All these test bodies and the production matcher are unchanged
copies; the actual `APP_CALLS`, `is_light_or_volume` and `KEEP_RULE` declarations
are copied from their source modules, and real serde_json is linked from the
retained target. The two cross-module executive/timer contract tests are
excluded rather than mocked; they and the Engine tests remain part of the
pending native Cargo gate. The initial harness failed to compile because its
constant extractor stopped at the array type's semicolon; correcting only the
harness extraction produced the 21-test pass. This was not a product compile
failure. Controller: `/tmp/fourth-duration-state-module-tests.py`.


## Combined Twelve follow-up (10 November, not yet native-qualified)

Twelve returned and published `53c7cb6f75006331ec09afa8c82ae9d3cc8474cb`
(regressions) and `3cf025efa3739b6b91488936b1d05f4b8be4bc28` (general
clock/end-marker repair). Both commits are carried with their author retained
as `1e86fae813` and `cbe92fceaa`. Conflicts preserved both regression sets and
adapted the generic end slot to the carry's punctuation tokens. This is not
an equal-patch rebase and earlier reviews do not qualify the combination.

Before the general repair, all four reported clock/comma questions claimed;
the duration-only repair still claimed three. The initial combined source
failed two extracted tests (20 passed): a trailing `10:30.` decided early,
and the narrow bare-time rule rejected the explicit "as you asked" claim.
Those failed results are retained in `/tmp/fourth-twelve-carry-integration-red.log`.

The combined correction keeps every adjacent numeric clock/decimal component
in a word, including uncertainty at a trailing numeric mark. Generic end slots
wait past commas for the sentence's end. Bare keeping and duration-state runs
remain narrow but allow the bounded acknowledgements "as you asked" and
"as requested"; the final generic end slot still rejects a trailing question.
The intended shape has no recursive acknowledgement grammar; the later
self-review below found and corrected an exception in `as requested`.
Descriptive comma continuations
such as "as a habit, wastes power" remain nonclaims. Limit extraction keeps
its original tokenization.

Current standalone module result: 23 passed, zero failed/ignored (1.50 s),
with both external contract tests still excluded. All four reported questions
are now nonclaims. The Engine control now covers seven explanations/questions,
whole and split after a duration, numeric mark or comma; it remains unexecuted
until the native gate. The native controller must use the new carry head and
combined source. Earlier duration-only comparisons and timing above describe
their original source, not this combined revision.


## Shared duration suffix and latency limits

A broader combined-source sample exposed wall-clock outliers: the existing
1,483-assertion probe passed functionally but reached 7.265358 ms at one call.
A diagnostic measured both elapsed time and this thread's CPU clock over the
same 1,000-prefix workload. The worst 16.057378 ms elapsed call consumed only
373.555 us of thread CPU. That excludes matcher computation as most of that
sample's delay; it does not identify a root cause or establish device latency.
The strict under-1-ms elapsed target was not met and is not reclassified green.

The duration-state implementation also repeated each full prefix three times
for its optional unit. A shared `DurationEnd` slot now checks those exact three
suffixes after one prefix match, reducing the word-run table from 98 to 74.
A separate differential executable passes 15,288 comparisons against the
pre-refactor combined source (every prefix, streaming/completed and worded
on/off). The 23 extracted module tests and earlier 1,483 assertions also pass.
No new duration spelling or acknowledgement semantics are introduced by this
refactor.

One sample each, optimized Rust 1.96.0, same 1,000-prefix workload; microseconds:

| Source | First elapsed | Median CPU | p99 CPU | Max CPU | p99 elapsed | Max elapsed |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Before this follow-up | 80.458 | 84.193 | 156.870 | 186.713 | 196.778 | 480.290 |
| Combined, repeated prefixes | 138.480 | 198.213 | 272.725 | 373.555 | 983.953 | 16057.378 |
| Combined, shared suffix | 72.460 | 98.031 | 186.174 | 210.599 | 234.114 | 1495.430 |

The revised sample reduces measured CPU work, but still misses the strict
1-ms elapsed maximum (1.495430 ms; that call consumed 209.081 us CPU). No
repeated run is used to erase either outlier. Sources/logs are the
`/tmp/fourth-{original,combined,optimized}-matcher-cpu-probe.*` artifacts;
the equivalence controller is `/tmp/fourth-duration-suffix-equivalence.py`.
Native package gates, current-head CI and any lamp/model/speech measurements
remain separate and pending. No readiness or device-latency claim is made.


The current native plan is workspace formatting and strict all-target Clippy,
the `action_claim::` filter (matcher and actual Engine controls), the full
Voice package, then the workspace build. A source-callsite audit finds every
`action_claim` consumer in Voice (second provider, worker, language and process).
The earlier generic plan to repeat unrelated dependent-package suites is
therefore narrowed to the package containing all callers; workspace Clippy and
build still check every dependent target. The cancellation carry's broader
protocol gate remains separate. Current queued source is rebased onto the
reset-runtime base `90d9948c4a`, with unchanged repair bytes, but no native or
CI qualification of that rebased combination is claimed yet.

### Bounded acknowledgement self-review, train 767

The first current-base gate passed formatting and strict workspace/all-target
Clippy, then was deliberately interrupted during focused-test compilation
(exit 130) before a result. It is not a focused/full Voice pass. Its logs and
receipt remain `/tmp/fourth-duration-state-native-*`.

Self-review found that the `as requested` acknowledgement used TimeEnd again,
whereas `as you asked` used the ordinary End slot. A standalone probe of the
actual module reproduced the asymmetry: `Keeping it on until ten, as requested,
thanks.` was not recognized, although the corresponding `as you asked` form
was. The repaired form uses End for both, preserving rejection of `right?`
without recursive acknowledgement matching. Both trailing-thanks forms are
now regression cases. `/tmp/fourth-matcher-ack-{before,candidate}.log` retains
the observed before/after results; this probe is not Engine qualification.
Fresh corrected-source gates use `/tmp/fourth-duration-ack-native-*`.

## Corrected combined source: completed train-767 native gate

On 44dc707f83c7b12aee826b148adb84e627dee194 plus the five-file repair,
based on b3e2d0903fb9a9a5fd34c41d84af761f9db3413c, workspace formatting
passed in 21.023 s, strict workspace/all-target Clippy in 123.272 s, and
54 focused matcher/Engine cases passed with zero failed/ignored (2.70 s body;
152.047 s command). This includes the seven whole/split Engine examples that
were still unexecuted in the earlier source epochs above.

The full Voice package passed 5,802 cases / 24 ignored / zero failed:
5,512 library cases / 18 ignored (942.43 s body), one binary case, 279
integration cases / six ignored, ten example cases and zero package doc cases.
The full command took 1,047.495 s; workspace build passed in 80.087 s.
All five guarded source/doc hashes remained identical through every stage.
These are results from the corrected source, not a pass for the intentionally
interrupted prior gate. Exact commands, hashes and logs are recorded in
`/tmp/fourth-duration-ack-native-receipt.json` and its stage logs.

The switch remains off, the strict mixed-prefix elapsed target still missed,
and actual GO-159 reply samples and authorized replay remain outstanding.
Train 768 landed during this run; its Voice delta is confined to a shared-lock
fix in a test fixture, alongside camera scheduling and documentation changes.
A current-base carry and focused/workspace verification follow separately.

### Corrected acknowledgement-path timing

A separate optimized standalone probe measures the corrected acknowledgement path itself, with identical source before and after except its final slot. On “Keeping it on until ten, as requested, thanks.”, the first result changes from nonclaim to LightKept. First / median / p99 / maximum elapsed time: before 134.517 / 95.543 / 174.901 / 371.834 us; after 140.293 / 41.186 / 92.161 / 165.070 us, over first use and 1,000 repeats. Thread CPU median/p99/max: before 96.050/170.064/265.174 us, after 41.663/85.923/139.334 us. This path stays below 1 ms in this sample; it does not erase the earlier mixed-prefix elapsed maximum failure or establish a device bound. Native compilation ran concurrently; no scheduling cause is inferred. Source hashes and commands: `/tmp/fourth-matcher-ack-timing-receipt.json`; logs and source `/tmp/fourth-matcher-ack-timing-{before,candidate}.{rs,log}`. The candidate prefix matches the frozen current production prefix.

## Train-768 carry: current-base gates

The completed combined patch was committed as 959bf6d699, then carried to
3a8f37d0b106053e5e65f81c03a7a1f518bc5005 on
75da83b3447dbd233a952b67d7effee652104277. All six authored commits are
range-diff equal; all five changed files retain exactly the same hashes.
The upstream Voice diff only locks and documents one shared-state test fixture;
no Voice production change intervenes. Camera scheduling and docs changed in
other parts of the workspace. The full Voice pass above remains bound to its
train-767 source; it was not repeated merely to restate the result.

Current-base workspace formatting passed in 19.296 s and strict all-target
Clippy in 143.927 s. All 54 matcher/Engine controls passed, zero failed/ignored
(2.70 s body; 173.606 s command). The changed upstream fixture
`second_provider::worker::tests::a_forget_confirmation_cannot_reenter_the_next_turns_history`
passed once with `--exact --test-threads=1` (0.02 s body; 1.722 s command).
Workspace build passed in 105.902 s; all guarded hashes remained identical.
Exact commands, source hashes and logs are in
`/tmp/fourth-matcher-train768-native-receipt.json` and its stage logs.
Docs pass 192 documents / 1,899 local links / zero broken. Fresh published-head
ARM/Linux CI and both readers remain required. The elapsed target miss, the
missing exact GO-159 replies and the authorized lamp replay remain open;
WORDED stays off.
