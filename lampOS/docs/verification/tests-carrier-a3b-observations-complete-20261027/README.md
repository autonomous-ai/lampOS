# Carrier on a3b: ordinary observations complete

For head `63a2a9579f1434ee9c4f323209c89073d918c614` on
`a3b9c46814bb2b2452f0d6b6cc13bb3a16992c33`, all **448** frozen ordinary
inventory identities now have outcomes across separate observations:
**442 passed, 0 failed, 6 ignored**. This was not one full Cargo command.

The separately authorized final invocation observed only the previously unstarted
`self-process-cuts::persisted_self_switches_survive_sigkill` declaration once.
It was **ignored**, as declared on Mac because it requires Linux syscall tracing.
Its command and native controller both exited zero. No completed case was replayed.

The [original reserve abort](../tests-carrier-a3b-reserve-failure-20261027/)
remains FAIL/-15 and its interrupted install observation remains INCONCLUSIVE.
The separately labelled recovery observation passed once. The
[parser-failure disposition](../tests-carrier-a3b-parser-disposition-20261027/)
retains controller FAIL/1 while reconciling the six successful commands’ 300 PASS / 3 IGNORE.
Those records were neither replaced nor waived.

The new completion binding combines original 142 PASS / 2 IGNORE, the subsequent
300 PASS / 3 IGNORE and final 0 PASS / 1 IGNORE. All source, 258 sealed-copy and 10 actual
executable guards remained intact, all owned groups closed and the lease was
released. Update doc tests and standalone promoter gates are still unrun;
current native CI and device acceptance are not claimed. This is ordinary-test
coverage evidence, not complete current-source qualification or a performance result.

[audit.json](audit.json) binds the final case/command/native exit, aggregate
coverage and original immutable failure/disposition receipts. Public preparation
used existing receipts only; no compiler, runtime, binary copy or source/target
inspection occurred.
