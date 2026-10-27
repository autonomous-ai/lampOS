# Mood a364 on 99b: full host command failed

The full serial Expression command for [#3623](https://github.com/autonomous-ai/lampOS/pull/3623), source `a364a97dc1dd6c0a711d0bb81d3a4f4e04da9500` on pinned Main `99b5161531791de7ed57ce57f6cacf988d509f25`, completed with **337 passed, 3 failed, 0 ignored**; documentation tests reported **0/0/0**. Cargo exited **101** and the native controller/wrapper exited **1**. This qualification failed.

The four actual test drivers reported 27/0, 4/0, 281/0 and 25/3. All 340 ordinary identities reconcile with their actual inventories, with zero filtering. The affected Journal case passed. The three failures, using one-based `cmd-tests.stdout.txt` line numbers, were:

| Full identity | Observed assertion |
| --- | --- |
| `sleep_name::a_clock_not_set_gives_no_night_and_a_set_one_does` | `runtime.rs:791:13`, `counts.refused == 0`: left **2**, right **0** (raw lines 365–368; refused-input lines 362–363). |
| `sleep_name::sleeping_name_requires_validated_speech_through_the_local_bus` | `runtime.rs:993:9`, `has(&output, "waking")` (raw lines 374–375). Its block reports lost ownership, readiness or freshness; it supplies no refused-input count. |
| `sleep_name::the_service_breathes_the_head_with_moods_breath_from_the_bus` | `runtime.rs:791:13`, `counts.refused == 0`: left **1**, right **0** (raw lines 381–384; refused-input line 379). |

The affected Mood case reached the common refused-input assertion before its amplitude comparison. The logs do not establish a cause. Formatting, strict all-targets Clippy over the actual twelve-package reverse closure, workspace build, full Expression `--no-run` compilation and all eight inventory commands exited zero. The final Cargo stderr contains no `Compiling` or `Checking` line after `--no-run`; that observation is not a freshness guarantee.

The completed read-only integrity audit found the **10,677-entry private source and official owner unchanged**, all **270 inherited plus 8 new frozen preimages** unchanged in SHA-256, byte length, mode, mtime, device and inode, and all eight actual executable hashes, lengths, modes, mtimes and devices unchanged. All four test-driver inodes also match. Only the four legacy non-test aliases (`lamp-expression`, `recent_playout_cost`, `rest_cost`, `replay_cost`) have replacement inodes. Frozen modes remain 276 copies at `0555` and two at `0755`.

The prospective alias policy allowed such inode replacement only after Cargo exit zero. Actual exit 101 therefore also left the original post-command boundary and final guard **FAIL**. The independent read-only classification preserves those failures and all Rust failures; it grants no waiver. The terminal records owned groups closed and the lease released.

At the failure-packet snapshot, the single unmodified-99b, 28-case runtime control was **DISABLED / UNALLOCATED / unrun**. Its plan and review are snapshotted privately. Root subsequently authorized and started that one-shot control, with exact private-source restoration required. This failure packet contains no control outcome or current-Main qualification.

The [earlier Mood paired fixture control](../tests-sleep-mood-clock-control-20261027/), [original full host baseline](../tests-mood-90ca-composed-20261027/), [4bff compile/runtime-reuse record](../tests-mood-4bff-reuse-20261027/), and [Ready 338/2 failure plus separate base control](../tests-ready-bdc-failure-20261027/) retain their own source and execution scopes. The [original Mood native checks](../tests-mood-f4419-native-20261027/) and [original three-PR native checks](../tests-original-native-three-prs-20261027/) likewise apply to their recorded heads/previews, not this failed a364/99b command. No historical, host-load or dependency-change causality is inferred.

[audit.json](audit.json) contains sanitized results, artifact metadata and SHA-256/byte receipts. Every entry in its private publication-input manifest is verified after the publication writer closes. Raw logs, source contents, binaries and archives remain private; no candidate test was replayed for this failure audit.
