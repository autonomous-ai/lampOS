# Caller-bound face consent: completed host gate

Ready: no; native, current-base and physical conditions remain.

[Published code and source-scoped proof](https://github.com/autonomous-ai/lampOS/tree/2ced256ffdc19dfb5aa84a8adf75638a0da94420/lampOS/docs/verification/onboarding-speaking-face-20261025) records the one-commit P2 fix. A name alone cannot save a face with the speaking-face switch on: the current caller needs an explicit face-save request or a fresh own-name reply after the fixed consent question is played whole. Stops, privacy, other output and caller/session changes revoke it; mixed old/new enrollment refuses without fallback.

On frozen Main f903659bb, tested tree ab92a0a49 passed formatting, metadata, strict workspace/all-target Clippy and workspace build. Runtime passed **4,563 unique selected cases**: 29 new controls first, then 4,534 disjoint cases. All **29 new Voice / 16 retained Voice / 11 Eyes** controls passed once; [named identities](named-controls.json). Four workspace doctests passed separately. Discovery's 27 ignored declarations and 482 filter mismatches may overlap and are not passes. [Receipt](receipt.json) binds actual completed runs and ledgers.

The first runtime runner stopped before any Cargo command or test; that zero-operation failure remains recorded. [Earlier compiler failures](../onboarding-consent-compiler-history-20261028/README.md) remain failed. Only proof documents changed between the tested tree and publication tree 7e3ad6028; no tested code changed.

The current-Main rebase has its own fresh gate. Native ARM/Linux checks on the final pushed head, independent review, and the normal-light intended-person within-two-tries/two-person/cancellation/privacy lamp trial remain required. These host fixtures do not activate enrollment or measure hardware latency.
