# Skills runtime permission qualification

The d5dc carry `5aa34f187` passed formatting, strict Clippy for the actual
32-package transition closure and workspace build. Its first full Skills run
exited 101: **398 passed, 124 failed, 9 ignored**, with repeated local Unix-socket
binds refused as EPERM. That failure is retained in the audit.

One separately recorded permissions-correct full runtime used identical source
and actual compiled executables: **522 passed, 0 failed, 9 ignored**. The changed
case ran once. Complete inventories, source, actual binaries, independent copies
and 127 inherited witnesses match. This establishes the result in that permission
context; it does not attribute every original failure or alter a product gate.

The latest carry `d689eddaa` on `1ec33957c` has an equal range-diff. Fresh compiler
gates and two Update include-reader checks are running. Runtime reuse requires
identical rebuilt Skills binaries, profiles and inputs; it is pending here.
The separate privacy lease-reader failure remains held under #3315.
