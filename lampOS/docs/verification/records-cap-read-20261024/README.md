# Records byte-cap fixture read

Third's requested one-reader review of PR #3381 at 0d057ef4be25107e203d8ccdef246360b2357ab0. Complete two-file diff read: 13 added fixture lines and result documentation. No P1/P2 found; READ-OK posted on comment 6384510943.

The existing control calls group_text directly with exactly 4,096 and 65,536 ASCII-space bytes, asserting the full returned content; limit plus one must refuse. UTF-8 conversion accepts this content and cgroup.events parsing is outside these calls, so parsing cannot stand in for the byte cap. The bounded reader and all production cleanup guards remain unchanged. The six-control module and enclosing functions were read; no product behavior, schema, privacy or data-loss policy change.

Original ordinary ARM64 run 40094999252: **1,448 passed, 0 failed, 16 skipped**, process controls **12/0**. All six exact cgroup controls passed once, including oversized_evidence_refuses_cleanup. Original Linux 40094999249 explicitly passed fmt and strict workspace Clippy.

Original dedicated native ARM64/systemd run 40094999273: three exact successful receipts, no guard overrides, cleanup confirmed; all six external unit receipts confirm zero main/control PIDs, no job, empty cgroup and successful driver exit. Preliminary test-selection.json deliberately leaves case_receipts_verified false; the later separate case-verification.json establishes qualified=true and cleanup exit 0. Three validated identities and final receipts were checked directly rather than treating selection as qualification.

Both native jobs checked e52cd65f6. Its complete tree equals authored tree c2e7857fc8b1f2a8dcb83378028e36397f7e321f. All 10,441 source file hashes and all path/link/mode/mtime/index maps are unchanged before/after; both changed files match the captured hashes. The audit retains original CLI-log hashes; full native artifact maps/case receipts remain on the original GitHub run. Producer local Mac/Linux compiler/package gates remain separately attributed; Third did not claim to re-execute those or independently inspect their private files.

No local Cargo, Linux/payload, device or systemd execution by Third. The changed integration crate is Linux-only; a Mac Records pass would not execute the strengthened control. Current native execution and its source/cleanup evidence were independently read. No signature trust, full-rotation, power-loss or physical-lamp acceptance inferred. Latest integration remains with the train.
