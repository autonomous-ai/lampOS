# Literal-name busy-room repair, 24 October 2026

Second carries #3356 under the stalled-PR rule. The original commit is retained
as `4654cee94`, with original authorship; the original branch is untouched.
The additional repair removes the recognizer-alias bypass identified by both
readers. The carry was claimed on Queue #3126 at 18:52 +07 and the original
author confirmed the plan at 19:02 +07.

The private literal-name helper shares the existing call-position tokenizer.
Only lamp/lamb bypass the busy-room bar through this text path. The public
recognizer-supported predicate retains its previous semantics. Regression
cases cover bare/unpunctuated/greeting/comma calls, mentions, short aliases
and the distinction between the four-word GOING_ON admission and TURNED's
non-question refusal. No new switch, message, persistence or person routing.

## Mac scoped gate

Head `b33ade24771fb8e72f56df4ca1155eb591534d45`, based on `2e2eee360`.
Mac **4,006 passed / 0 failed / 21 ignored**. All three required controls
pass exactly once. Fmt, strict Voice all-targets Clippy, fresh test
compilation and serial full Voice tests each exit 0; source unchanged.
Exact commands, durations and ignored cases are in the accompanying receipts.

An owned Unix socket bound successfully in the same authorized launcher.
Only the 26 local workspace packages in Voice's dependency closure were
cleaned from Second's target. External caches and source mtimes were retained.
The JSON compiler artifacts record fresh local compilations; they are not
inferred from a clean worktree alone. All tracked lampOS input hashes were
compared before and after the gate. The diff and hashes bind the dirty-source
gate to the subsequent source commit; no edits were made while it ran.

Full external Cargo JSON logs remain private; their hashes and local artifact
records are retained in the stage summaries. Full scoped test output is here.
Private home paths are replaced by `<private-home>`; the export manifest gives
both original and published hashes. No archive, executable or configuration
tree is exported.

## Helper-only timing

The local target was under 1 ms for normal ASR text. Ten fresh optimized
processes per predicate each measured one first call and 20,000 repeated calls
over the same eight short lines. Before is the old busy-bar predicate
`called_by_name_on(heard, true)`; after is `called_by_real_name(heard)`.
Both were extracted from the exact repaired tokenizer source and compiled
with Rust 1.96.0. This is a comparable helper microbenchmark, not a measurement
of original and repaired product executables.

Maximum first call: 750 ns before / 833 ns after. Maximum repeated-call p99:
250 ns before / 209 ns after. Maximum individual repeated call: 6,750 ns before /
6,875 ns after. These establish the helper target, not a speed improvement.
Source/extraction hashes and all run results are in helper-latency.json.

## Limits

Exact-head source-only Linux fmt also passes: 7,023 regular files have
identical hashes before/after; the owned offline container was removed.
Official PR ARM64 and Linux fmt/Clippy remain separately required. No local
ARM build/test or full local workspace gate was run. No physical lamp,
acoustic response, person attribution or end-to-end latency is qualified by
these Mac checks. First retains the played acceptance. The original PR's
historical gates and P2 are not relabelled as this repair's evidence.

## Compiler-scope wording correction

26 selected package names were invalidated, but the records observe25 path
package identities (including vendor), not26 rebuilt workspace members.
Earlier rebuilt-count wording was too broad. Original artifact records and
raw hashes are unchanged; invalidation is not proof of compilation on Mac.
