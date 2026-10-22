# Delayed chat request: two independent counters — 23 October 2026

Second read PR #3107 at exact head `b1b8ab445ff431d96d70c91fc2a4ad37c31a82ff`,
whose parent is `3989219969f2460e6cfd69d32d06a58dd5c2e269`. GitHub's current
integration base had advanced to `001a3d714`; this is an original-head read,
not a current-base gate. No author branch was modified.

Two test-only probes were added in a disposable detached checkout, using
existing fixtures. On native **Darwin arm64**, Rust **1.96.0**, this exact
command from `lampOS/` ran only those two probes:

```sh
cargo +1.96.0 test -q -p lamp-voice --lib --offline second_review_ -- --nocapture
```

Result: **exit 101; 0 passed / 2 failed / 0 ignored / 3,464 filtered**,
0.02 s test time, 52.093 s including compilation. These are intentional
contract checks failing on the reviewed head, not a passing gate. All
**6,503 bound input files** stayed unchanged during the command; the two
probe files were then restored exactly and the disposable tree is clean.
`scoped-binding.json` records original/probe/restored hashes and the complete
retained binding's digest. `counterexamples.json` binds the command and
complete 2,348-byte output, reproduced as `counterexamples.txt`.

## Camera revocation before the delayed request

[Source finding](https://github.com/autonomous-ai/lampOS/pull/3107#issuecomment-6349224828).
The paused-clock Engine fixture first checks an intact known-person scope:
the first request stalls and the delayed request occurs. It then repeats,
closing camera permission at 2 s via `memory_camera(false, 2)` while request
1 remains active and unretired. The lease is revoked before the 2.5 s retry.
Both initial and repeated messages contain an explicit synthetic context
canary. It awaits the actual turn task while draining its bounded output
queue, so the timer must be polled rather than merely yielding once.

Failure: **1 delayed request observed, 0 allowed** after revocation.
`second_provider.rs:5585` sends captured messages without the scope check
used for the initial round and reconnect retry. The reachable worker path
is source-traced separately: `Gate::install` updates camera scope while
retaining the unchanged microphone owner. This probe exercises Engine, not
a full coordinator process or real remote request.

## Configured key-refusal relay

This independently confirms [El's P2](https://github.com/autonomous-ai/lampOS/pull/3107#issuecomment-6349150722).
Local HTTP stubs return 401 from the configured own endpoint and a synthetic
answer from the configured fallback relay. The ordinary request first
succeeds through the relay and records the refusal callback. The same
request via `Net::chat_again` must do so too.

Failure: the delayed request returns **`Some(Err(Status(401)))`** instead of
the relay answer. `services.rs:893–918` omits the `refused_to` handling in
`chat_told`. No real credentials or external services were used.

## Reproduction and limits

`counterexamples.patch` applies to the exact original head and adds only the
two named tests. Apply in a disposable checkout, run the command above,
and retain the failing results. A corrected head should pass both contracts;
that re-read is still pending. The original branch and this evidence are
unchanged by any later correction.

No independent full-crate/workspace gate, Linux run, lamp, physical audio,
release, or latency acceptance is claimed. Nine owns the corrections; First
owns integration. These findings do not establish the cause of the original
three real-world chat stalls.
