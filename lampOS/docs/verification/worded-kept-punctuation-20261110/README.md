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

Only the three bare Keeping/Leaving runs use the new complete-time and closing
slots. Generic time/end slots retain their rules. Numeric punctuation joins
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
