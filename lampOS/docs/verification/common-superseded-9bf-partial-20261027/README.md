# Superseded Common 9bf partial runtime

Ready for lamp-voice: no

[Selected receipt](receipt.json) records a failed, deliberately cancelled host
attempt on tree `9bfda85e9a5152cb41df1825e0ddd1e0aee12aac`, based on
`ed9aff39b609676535343f8979631c909e2a430a`. Actual controller exit **125** and
nextest exit **100** remain failures. Host test debug information was disabled;
optimization, assertions and overflow checks were unchanged, with two slots
and zero retries.

Of 11,495 selected cases, **6,558 passed, 12 failed and 4,925 were unstarted**;
none remained started without a terminal. The failures are nine Eyes cases,
one Health case and two in-flight Update cases deliberately interrupted with
SIGINT. The 110 required controls have **40 passes, nine failures and 61
unstarted**. The 100 natural ignores comprise 44 JSON ignored terminals and
56 start-only identities with a matching human SKIP; those 56 are not executions.

The run was stopped because the superseded source already had fixture and
disk failures, and latest Main plus the fixture repairs require fresh gates.
The rollover regression passed once within this historical partial attempt;
its earlier RED exit 101 remains a failed expected negative control. Neither
observation qualifies later source or completes the required-control set.

Failure retention exited 0 after inspecting all three independently retained
drivers. Actual dsymutil and independent Mach-O N_OSO inspection each found
zero backing inputs for every driver. Source and owned-process closure were
verified before release. The receipt binds the original terminal, private
partial audit, both ledgers and retention release by SHA-256.

The latest-Main carry and fixture repairs are staged and remain unqualified.
This packet establishes neither full-workspace qualification nor physical
acceptance; fresh current-source gates are pending.
