# Send receipt reconstructed after interrupted face forget

Status: reproduced failure, no production repair in this commit. This is an
integration HOLD, not a release or device qualification.

The tested source is parent `02fae6d762808ade6095dea29614fba2b874dbd6`
on train759 `caca127702`, plus the test in this commit. The parent composes
provider/playback cancellation (#5856/#5858), rename cleanup (#5792), face
outcome/context handling (#5874/#5878), the admission-time carried-round clear,
and the face speech extension. The new train contributes #5863's send records.

## Actual Engine sequence

1. The person says “Go ahead and send it, then forget my face.” The model calls
   `service_write`; its synthetic argument and result contain separate canaries.
2. A wordless line cuts that request while the send runner is held. The runner
   then returns `sent: true`; the resumed model receives that completed receipt.
3. The resumed model calls `forget_my_face` with `name: A`. The test waits for
   actual runner entry, then interrupts its pending deletion future with another
   wordless line. This is an admitted call, not an unasked model suggestion.
4. The next resumed model explicitly requests another `service_write`. The
   existing guard refuses it: the runner's write count stays exactly one.
5. The test inspects every model request after that second interruption. Both
   the earlier argument and result canaries have returned in raw tool messages.

The test fails on the argument-canary assertion after its one-write assertion
passes. The same resumed-request panic also contains the result canary. Result:
**0 passed, 1 failed; exit 101; 5.05 seconds**. The canaries are synthetic and are
not in the retained current human words. No real service, account or device is
used; services and deletion are controlled runner fixtures.

## Reproduction

Run from `lampOS/`, using Rust 1.96.0 and local socket permission:

```sh
TMPDIR=/tmp \
CARGO_TARGET_DIR=/tmp/fourth-face-carry-target \
CARGO_PROFILE_TEST_DEBUG=0 \
CARGO_PROFILE_DEV_DEBUG=0 \
CARGO_INCREMENTAL=0 \
/Users/autonomous/.cargo/bin/cargo +1.96.0 test --offline -q \
  -p lamp-voice --lib \
  second_provider::tests::sent_write::a_send_then_cut_forget_then_cut_never_restores_payload_or_repeats_send \
  --jobs 2 -- --nocapture
```

Raw output is retained locally as `/tmp/fourth-face-carry-send-forget.log`.
An earlier compile was stopped with exit 130 to correct the fixture's missing
required face name; it is setup evidence only, not a product fail-first result.

Formatting and strict `cargo clippy -p lamp-voice --all-targets -- -D warnings`
passed for this test-only source. The intentional failing regression is not a
green product gate; no production repair or readiness is claimed.

## Repair boundary

Clearing carried rounds is insufficient: `SentWrite` retains the old call and
receipt, and `sent_with_words` reconstructs the round when it is absent. Dropping
that record outright would also remove protection against repeating the send.

An admitted forget needs sticky content redaction for the affected attempts,
while preserving content-free sent, uncertain and refused accounting. Recheck
that boundary after waits and before publishing carried content: already-cloned
calls, watch receivers and a late `services::send_state` result must not restore
payloads. A late result may update status only. Redacted notes must not tell the
model that discarded calls/results are available or invite reconstruction.

Before lifting the HOLD, cover late completion/fallback, unadmitted forget,
unknown-send accounting, multiple attempts, latest-origin selection and reused
model call IDs. This one failing Engine case does not establish those controls,
whole-suite success, real-model behavior or the separate acoustic stop target.

## Additional Engine controls

The unadmitted-forget control starts from words asking only for a send. The
model proposes a face forget after the send completes, and the test waits
until the refused call has actually appeared in a subsequent model request
before cutting again. It requires no forget-runner admission or speech cut,
retains the original receipt canaries, and still prevents a repeated write.

The late-receipt control holds the original send beyond its modeled deadline,
then waits for actual face-forget runner admission before releasing the send.
Only the retained send timestamp is moved back to model expiry; this does not
measure a real twenty-second timeout. Before release, the resumed request must
say the send is unconfirmed. After the genuine receipt arrives, content-free
status may become confirmed, but original argument/result/raw-call-ID canaries
must remain absent and the write count must stay one. A note must not claim
that discarded call/results accompany the words. The test waits for the real
watch receiver to observe completion before the second interruption.

These controls exercise controlled runner futures, not a real service or a
physical lamp. The complete sent_write module returned nine passed and two failed in 3.17s
(exit 101, no ignored cases). The primary regression still fails at its original
argument canary. The late-receipt case fails at the raw call-ID canary after
its one-write assertion; the same panic contains the argument and result
canaries. The unadmitted-forget control passes. Raw output is retained at
`/tmp/fourth-face-sent-write-controls-status-policy.log`. The exact final command was `cargo +1.96.0 test --offline -p lamp-voice
--lib second_provider::tests::sent_write:: -j 2 -- --test-threads=1`, with the
same cache/debug environment above. Formatting and strict Voice all-target
Clippy also passed (exit codes 101 / 0 / 0); Clippy used
`cargo +1.96.0 clippy --offline -p lamp-voice --all-targets -j 2 -- -D warnings`. These do not replace the additional fallback/multiple-attempt
and latest-origin checks required for the production repair.

## Independent native Linux evidence

[ARM64 run42661995793](https://github.com/autonomous-ai/lampOS/actions/runs/42661995793)
checked out exact `6c82603f93eb4f28c0e333b8575f694053cb2a46`, tree
`0a6ec1e36553040ed7fb8360e6b6b7eee6c82e70`. Release compilation passed.
The Engine case above executed and was the sole failure: both payload canaries
were restored while the duplicate-send guard held. Nextest reported12,934
passed, one failed and111 skipped, with2,034 tests not run after fail-fast;
execution time272.587s, test step exit100. Later doc/own-harness/export stages
were skipped. This is independent failure reproduction, not full-suite success.
[Linux Clippy42662002329](https://github.com/autonomous-ai/lampOS/actions/runs/42662002329)
passed whole-workspace strict all-target checks at the same source. Local raw
logs are `/tmp/fourth-repro-arm64-42661995793-job.log` and
`/tmp/fourth-repro-clippy-42662002329-job.log`.

The remaining repair-specific checks include a private `services::send_state`
fallback after admission, unresolved/asked/refused outcomes after forget,
truthful content-free status after late success, and admission crossing a
previously cloned receipt before publication. Existing multiple-attempt,
latest-origin and reused-call-ID controls do not themselves include forget;
they must continue passing and the redaction must remain bound to each attempt.
