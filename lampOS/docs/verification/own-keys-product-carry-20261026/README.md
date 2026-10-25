# Own-keys product carry and exact-template retirement

This carries #3004's authored proposal, retaining its author and one-line commit. Product activation remains **HOLD** for #2094's broker/account/TLS dependency and fresh current-base acceptance. Cloud settings are already on, so this proposal also opens app keys.set, keys.remove and keys.get. Earlier lamp evidence does not qualify current activation.

Only the exact 919-byte historical regular-file drop-in is retired. Edited/off overrides and links stay; I/O errors are visible; reads are bounded to 920 bytes, checked again before unlink, and the parent synced. Reload debt survives a failed enable. The directory integration test proves preflight refusal with files, state and host calls unchanged. FIFO coverage is a private matcher unit test on Linux, not successful full-installer handling.

[inputs.sha256](inputs.sha256) binds the final owned changed source. [results.json](results.json) separates the two source inventories, exact identities, commands, counts, log hashes, rebase evidence and artifact-manifest identity.

## Original full run (failed)

Base 0a40aa50b, authored carry 4674cd4ba, plus the frozen repair and documentation inventory: 7,126 entries, identified by its SHA-256 in results.json. Rust 1.96.0 on aarch64-apple-darwin, one compiler job and serial tests. Formatting, strict all-target Clippy and test compilation passed for Update, Voice, Selftest, Setup, Cloud, Health, Controls, Skills and Voice-eval. The complete selected test run **FAILED**, exit 101: **6667 passed / 1 failed / 44 ignored**. Its only failing target was Voice's library; the other eight selected crates completed successfully. Focused own-keys integration passed **7/0** and the directory matcher passed **1/0**. The Linux-only FIFO case is excluded from the Mac count.

This failed result belongs only to the original frozen source. It is preserved as failed evidence, not a passing nine-crate qualification or a full test run of the later checkout.

The failure was second_provider::worker::tests::with_no_sign_of_a_second_person_a_line_let_out_joins_the_one_it_cut_short, at worker.rs:17231, with Unknown room evidence and a missing second look. The retained original candidate executable's exact replay passed **1/0**. The separately compiled clean 0a40aa50b baseline exact replay returned **1 passed / 0 failed**, exit 0. Each observation is bound to its own source inventory, command, log and artifact identity. These are observed replay results, not a claim that the baseline failed or that the original failure is inherited.

The first failure's root cause remains **unproven**. Look freshness is an investigative hypothesis; no causal connection to this change has been established. No clock or message-header capture was collected, and the worker source and assertion remain unchanged across baseline, original candidate and continuation. One complete current-base Voice run is required below because the original failure remains unresolved; unaffected suites are not repeated merely to replace their results.

## Current-base continuation

Current base 3d9722f38, authored carry 9dd71279d, checkout 4c2ecaeea, plus the separately frozen continuation inventory: 7,132 entries. Final source equals that inventory before this proof is generated.

The continuation passes formatting and strict Clippy, the full Voice crate tests (**4065 passed / 0 failed / 21 ignored**) and Controls crate tests (**516 passed / 0 failed / 3 ignored**), own-keys integration **7/0**, directory matcher **1/0**, the exact Update install documentation reader **1/0**, and CI-path scanner/sample controls **2/0**. Together with the original run's eight completed passing suites and the verified unchanged inputs, these are the local qualification evidence. This is a bounded continuation, not a full nine-crate test rerun. The original Voice failure remains disclosed rather than erased by the follow-up.

The three documentation corrections concern per-unit disabling, preflight versus matcher scope, and Selftest's moved unit reference. In particular, install.md is compiled by the Update test every_part_of_the_lamp_waits_for_setups_marker_itself; that reader is rerun against the continuation. The ci_paths.rs include occurrence is a raw-string sample; its scanner/sample tests are separately rerun. The audited own-keys.md and selftest.md corrections have no content reader.

The recorded upstream diff and retained range-diff are verified. The 15 declared owned blob comparisons check the source after the three documentation corrections but before rebase against the source after rebase; all pass. install.md is excluded from that comparison because it also changed upstream, and its overlap is recorded in both the upstream and owned path sets. Separately, the original-gate-to-continuation inventory comparison differs only in the three declared documentation corrections (install.md, own-keys.md and selftest.md) and recorded upstream changes. Every other inventory entry is unchanged. Runtime source differences are limited to the recorded upstream Controls paths. Unexpected inventory differences refuse publication.

## Retained failed fixture and deliberate fault

The first focused run remains **6 passed / 1 failed** with its original fixture and executable identity. The fixture incorrectly expected a directory to pass full installation. Existing head-touch preflight refuses that unreadable .conf before cleanup. The corrected test expects refusal; runtime was not weakened.

The deliberate existence-only matcher is bound to the original full-gate candidate, before documentation corrections and rebase. Its freshly compiled same-length edited-file control reaches the intended preservation failure: **0 passed / 1 failed**, exit 101. Baseline and restored-source exact controls each pass **1/0**. Compiler failure and deadlines do not count as detection; the complete source inventory is restored. This is one deliberate fault, not exhaustive power-cut or interleaving testing.

Raw logs, inventories, the first failed fixture artifact, original failing Voice executable, exact replay observations and negative-control receipts remain retained privately. These are author observations, not an independent rerun. Comparison and unlink are separate operations; no atomic protection against arbitrary privileged writers is claimed. No local Linux ARM64, actual systemd, provider, real-key, device-latency or activation acceptance is claimed. Native PR CI belongs to its eventual exact head and is recorded separately.
