# Reader current 1c readiness

Ready for lamp-voice: yes (#3527)

Current head 9abfa0c4e on 1c884f16c is pushed and non-draft. Both current-head
readers posted READ-OK. [ARM64 40262032102](https://github.com/autonomous-ai/lampOS/actions/runs/40262032102)
passes 7,483 unique cases, zero failures/leaks/timeouts and 53 skips across
70 executed binaries in 155.552 seconds. The affected subset is not a full
workspace census. All fourteen ordinary Reader tests pass; privileged DAC
remains unexecuted. Actual preview parents and tree equal the authored offer.
[Linux Clippy 40262032163](https://github.com/autonomous-ai/lampOS/actions/runs/40262032163) passes.

Current Mac formatting, actual reverse-twelve strict all-target Clippy and
workspace build pass after changed-package cache invalidation. A separately
recorded Voice-eval embedded-document clean/check/workspace rebuild also passes.
These are compile-only gates; all source and 99 prior frozen executable guards
remain equal. The 26c and a26 native results and failed historical full/baseline
runs retain their original source scopes and outcomes.

The separate #3528 sentinel failure remains open: its unscoped prepare refused
a ctime change during ACL inspection. The actual ancestor and historical cause
are unproven. This PR scopes rollback/early-refusal/root controls, not that case.
A separate private-root sentinel patch preserves all assertions and unscoped
security coverage; it has not yet been built or run. No device action is claimed.
