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
