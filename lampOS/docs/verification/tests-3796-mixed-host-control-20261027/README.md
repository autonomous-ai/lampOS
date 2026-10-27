# Mixed host/product template negative control

Ready for lamp-voice: no (fixed-source full gates and native CI pending).

For #3796, six new ordinary Round tests were compiled over unchanged updater product code at `3b9bfb628`. One serial selected invocation produced **2 passed / 4 failed / 0 ignored**, Rust exit **101**. Both manifest orders fail for the real part switch and for signed updater dry confirmation, each with the exact current-link refusal. The compatible linked-host and genuinely pinned-installation controls pass. This is an expected negative control, not a green suite.

The actual Cargo Round executable and updater helper were identified from compiler JSON and separately frozen. All six identities were verified from the executable inventory before that single runtime. All tracked source remained unchanged; all 407 protected witnesses and actual executable bindings passed their guards, and all owned process groups closed. Native controller exit was zero because it observed the declared negative control. [Raw output](control.stdout.txt) and [counts, signatures and source/artifact bindings](audit.json) retain the failing Rust result.

The fix, those exact six cases and eight safety regressions were subsequently carried onto `db3d44d73` for the full affected gate. Those later results are not part of this record. No ARM64 binary was built or executed on the Mac. No lamp was accessed for this control; 52e6 remains released and its production acceptance belongs to the separate device round.
