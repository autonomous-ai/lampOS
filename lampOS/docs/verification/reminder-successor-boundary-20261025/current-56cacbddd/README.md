# Current workflow and documentation carry

Head `82358489b` contains release base `56cacbddd`. Both own commits compare
equal to their ad9 versions. The ad9-to-56c base delta adds the separate
`.github/tools/lampos-promote` crate, workflow wiring and documentation.
All lampOS crate, manifest/lock, toolchain/config, third-party, model and
deployment inputs are byte-identical to the qualified ad9 composition.
The full d7 **4,674/0/21** and current ad9 affected **1,169/0/8** executions
remain scoped in their own records, not relabelled as new executions here.

Fresh current Mac formatting, strict all-target Memory/Voice/VoiceEval/Update
clippy and workspace build passed. The Update integration tests that actually
read the changed CI workflow paths also ran and passed **2/0**. Other targets
reported filtered selections and are not counted as test coverage. All commands
and literal results are recorded; the source remained clean throughout.

No lampOS runtime source changed in this carry. The separate promotion tool's
own tests and native train qualification belong to its base change; they are
not counted as new author executions here. Original offered-head Linux ARM64
and clippy remain separate gates. No lamp, acoustic latency or activation claim.
