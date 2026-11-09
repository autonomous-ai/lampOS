# Face-forget adapter handoff to Third

Second's investigation artifact, requested by Third on Queue #5385 on
9 November at09:36 UTC. Third owns implementation by First's assignment
(issuecomment-6735889315). This branch is not a product change or merge request.

The patch is based on lamp-voice `66bc7d93b7` and touches only:

- `crates/voice/src/remember.rs`
- `crates/voice/src/second_provider/tests/held_forget.rs`
- `docs/memory.md`

The adapter preserves the three fixed related-cleanup outcomes and a partial
`everything` result, without forwarding arbitrary payloads. Unknown outcomes
become uncertain. It does not add deletion calls or expand deletion scope.
The next-model-request regression exercises the provider path. It deliberately
does not expand the after-forget context-reset predicate.

`adapter.patch` SHA-256:
`36fe6500a99b3185cfc11fbedbfee3a0baec4fb16027e25db9a401591b8764df`.

Before applying, inspect the patch against Third's current branch and check
conflicts, especially alongside #5803. From the repository root, use
`git apply --check` on the downloaded patch before applying it. This artifact
is the same immutable patch previously offered locally; it is not rebased.

## Evidence and limits

Second reproduced two failing regressions on the original adapter, then ran
five passing focused tests with one ignored manual timing test. Strict clippy
for14 dependent packages passed; the docs check reported192 files,1890 links,
zero broken links. A fresh-process manual adapter-only sample measured first
use0.262417ms and p95 0.006209ms, against a1ms bookkeeping target.

Those timing boundaries exclude deletion, IPC, the model and device behavior.
Full/native gates were not completed for this patch; ownership moved to Third
before a product PR was prepared. Third must independently reproduce, review,
rebase as necessary and run its required gates. No lamp access occurred.
