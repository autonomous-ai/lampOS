# Third: speaker-bound requested face enrollment, 24 October

PR [#3318](https://github.com/autonomous-ai/lampOS/pull/3318), authored
`46c072c98c509f8a8357d8e27887dd9cda28cc2f`, based on `4a84fa5ab`.
First assigned this on Queue #3126 at 08:06; Third claimed it at 08:09.
Only Third's branch was pushed. The switch remains off by default.

## Local gates

The development Mac ran `cargo fmt --all --check`, strict Clippy with
`--locked --offline --all-targets --message-format=short -- -D warnings`
for both lamp-voice and lamp-eyes, and the full applicable test suites.

- Voice: `cargo test -q --locked --offline -p lamp-voice -- --test-threads=2`:
  **3,977 passed / 0 failed / 15 ignored** on base `88b1ad796`.
- The carry to `4a84fa5ab` changed only Eyes and its docs. All fourteen
  changed/new Voice files and their dependency manifests stayed identical
  to the tested bytes. Formatting and both Clippy gates passed again.
- Eyes on that current base: `cargo test -q --locked --offline -p lamp-eyes`:
  **326 passed / 0 failed / 5 ignored**.
- Focused Voice: **16/0**, including a microphone interval before STT starts,
  privacy close/reopen while STT is held, stop-before-publication, count-only
  exclusion, byte caps, mixed-version refusal and actual socket cancellation.
- Source hashes before/after each full gate are identical. Exact commands,
  timings and hashes are in `local-gates.json` and `current-base-gates.json`.

The original socket-binding tests were sandbox-refused and reran with local
Unix socket access. An interim test fixture used the other route's ToolCall
and failed Clippy compilation; that was corrected before the final gate.
Its diagnostic is retained separately, not presented as a product failure.
No ARM64 linking on the Mac or lamp action.

## Reproductions before repair

- Eyes' distinct guarded protocol: **1 passed / 1 failed** before wiring.
- Voice's old majority helper accepted one face despite a briefly speaking
  competitor: **0 passed / 1 failed**.
- Eyes' reset-crossing request could choose a later occupant after camera
  hold change or forget: **0 passed / 1 failed** before the cutoff check.

The corresponding red outputs and final controls are retained here.
Independent audit found the cache-eviction/reset seams, final write fences,
count-only exclusion and cancellation publication boundary; these are covered
by the final code and tests. Current speaking-classifier accuracy still needs
physical qualification. The separate #3313 score-inflation finding is on that
PR and owned by Sixth; this change does not enable that switch.

## Pending platform and physical results

Native ARM64 run **40052552152** remains pending at this update. Linux
Clippy **40052552144** passed. Second's exact-head source-only Linux
formatting passed with 6,967 files unchanged and container removal verified;
`linux-format-reported.json` is the published receipt, read and matched to
the authored repository tree by Third. [Original receipt](https://github.com/autonomous-ai/lampOS/tree/86c2c1ec1/lampOS/docs/verification/face-speaker-guard-linux-format-20261024). reviewer-three and reviewer-lead
are the assigned readers. No ARM64 test pass, merge, physical identity accuracy,
acoustic latency or readiness claim is made here. Existing four-second Eyes
capture and six-second tool deadlines remain; no added inference or wait.

## Latest carry and requested P3 coverage

Current head is `8b42bb15f26169f9d148940ac42dc8f745a65887` on `59c4ac795`.
The original implementation is an equal range-diff carry to `11c75e024`.
One following commit adds a recognition-policy/cache regression for both
phases of a 1.8-second line, explicit cadence/classifier limits, and the
unexecuted camera-backed main-loop boundary. It adds no runtime behavior.

Current-base fmt and both strict Clippy gates pass; full Eyes is now
**334 passed / 0 failed / 5 ignored**. The tested Voice source/manifests
are unchanged from **3,977/0/15 ignored**. The carry gate hashes every
tracked Voice/Eyes file plus workspace manifests and changed docs before
and after execution; all remain equal. See `cadence-carry-gates.json`,
`cadence-carry-eyes.txt` and `cadence-carry-range-diff.txt`.

The original head completed ARM64 **11,153 passed (one existing Voice
journal LEAK), 71 skipped**, process **12/0**, on an identical authored
and preview tree; its result is `original-head-native.json`. These are
historical passes, not carried-head acceptance. Fresh carried-head ARM64
**40053707125**, Linux Clippy **40053707118**, Linux formatting and carried
reads remain pending. Third posted the addressed P3s and current head on
[the PR](https://github.com/autonomous-ai/lampOS/pull/3318#issuecomment-6378682327)
and Queue #3126. Physical qualification remains unexecuted by Third.

## Current carry native result and name-ownership follow-up

Head `8b42bb15f` passes ARM64 run 40053707125: **11,161 passed (one
reported Voice journal LEAK), 71 skipped**, process checks **12/0**.
Linux strict Clippy run 40053707118 passed. The preview tree equals the
authored tree; see `cadence-carry-native.json`. No lamp result is claimed.

Reviewer-three’s separate Mac whole-library run at the original head
reported 3,693 passes and four failures, each passing twice in isolation.
Those outcomes remain recorded on #3318; isolated success does not prove
a failure’s cause. Third’s earlier full two-thread gate passed.

At 09:25 Third claimed the guarded name-ownership P2 and unusable rename
prompt P3 on `third/remember-own-name`. This follow-up preserves #3318 as
read, rejects third-person name mentions, and keeps the switch off pending
those corrections and physical qualification.

## Multi-face-base carry

`207358456` on `090847e59`: only a help-string conflict, preserving both
switches. The upstream patch remains exact after removing the unchanged
enrollment paragraph from the mechanical comparison. Voice is unchanged.
Mac formatting, strict Eyes Clippy and full Eyes **337/0/5 ignored** pass;
all recorded source hashes remain unchanged. Source review also checks the
detector-index mapping into `bus.saw` and the one-near-face enrollment fence.
Current native gates and renewed reads are requested. See `multiface-carry-*`.

## Landed guard and remaining physical boundary

#3318 landed as `9ed44f88c`, whose tree equals `207358456`. Both public
readers renewed their exact-head reads. Second’s exact-head Linux formatting
receipt reports pass, all 6,967 files unchanged and cleanup verified; Third
checked both repository and lampOS subtree hashes against git. The receipt
is retained as `multiface-linux-fmt-reported.json`. Native latest-head and
landed runs remain watched separately from the prior green run.

The [physical trial sheet](physical-trial.md) is prepared, **not run**.
It covers short-line cadence, wrong-person names/faces, cancellation, saved
name refusals and observable latency boundaries; First/lamp holder owns it.
The separately claimed name-ownership follow-up is still being gated.
