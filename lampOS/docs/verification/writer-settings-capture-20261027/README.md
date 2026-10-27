# Raw capture: retained failures and separate executions

Part of #3689. These packets record actual source scopes, not current-Main qualification.

- `410-compile-failure`: initial new test map-type compiler error; runtime unrun.
- `410-first-runtime-failure`: 488 passed / 1 failed / 6 ignored. A new fixture wrongly expected an unrecorded existing empty file to be overwritten; production correctly kept it.
- `corrected-face-11a`: corrected fixture separately covers owned-empty replacement and unrecorded-empty retention; full Update 491/0/6, all fifteen controls. The production policy was unchanged.
- `carry-c388-3b9`: fresh full Update 509/0/6, all fifteen observer/raw controls and eighteen already-delivered foundation controls.

No old failing source was replayed or waived. Current base carries, native CI, privileged execution and actual restoration remain separate. Text metadata binds private raw source manifests/compiler JSON/actual preserved executable bytes; no binaries are committed here.
