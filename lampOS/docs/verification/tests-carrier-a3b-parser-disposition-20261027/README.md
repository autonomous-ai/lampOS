# Carrier on a3b: parser failure and read-only disposition

The separately supervised continuation for head `63a2a9579f1434ee9c4f323209c89073d918c614`
on `a3b9c46814bb2b2452f0d6b6cc13bb3a16992c33` stopped with **controller FAIL/exit 1**
after six test commands had all exited zero. Its private parser incorrectly required
a semicolon and duration after `filtered out`. The custom `self-cli-exec` harness
legitimately ends its summary there. Read-only parsing now accepts that exact format;
the original controller failure remains unchanged. No Rust assertion failure occurred.

The completed new observations are **300 passed, 0 failed, 3 ignored**: install 91
passed; the separately authorized interrupted-case recovery 1 passed; it 13 passed;
release-pair 2 ignored; round 195 passed; self-cli-exec 1 ignored. The round command
completed once in 1,579.932 s. No completed command or case was retried. These are
separate observations, not one full Cargo command or a performance comparison.

Together with the retained original 142 PASS / 2 IGNORE rows, **447 of 448** unique frozen
inventory identities have outcomes: **442 PASS / 5 IGNORE**. The sole unstarted identity
is `self-process-cuts::persisted_self_switches_survive_sigkill`, a default-ignored
Mac declaration. Update docs and standalone promoter gates are still unrun. Full
current-source qualification is not claimed.

The [original reserve-abort proof](../tests-carrier-a3b-reserve-failure-20261027/)
retains the interrupted install observation as **INCONCLUSIVE**. Its one separate
infrastructure-recovery observation passed; that does not rewrite the interrupted
record. All source, 258 sealed-copy and 10 actual-executable guards passed across
the stopped continuation, all owned groups closed and no lease remains held.

[audit.json](audit.json) contains all 303 newly observed identities/outcomes and
sanitized receipt hashes. Preparation parsed existing private receipts only;
no source/target inspection, compiler, runtime, binary copying or CI operation
was performed. The final unstarted declaration requires a separately reviewed
controller and allocation before it can run.
