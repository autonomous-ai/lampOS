# Dropped-chance journal masking controls

This test-only follow-up closes the surviving-mutation finding on [PR #3007](https://github.com/autonomous-ai/lampOS/pull/3007#issuecomment-6341473959). The previous coverage only required the word `dropped`: exposing arbitrary text or removing the entire held-condition suffix could still pass. Runtime behavior and the two original tests are unchanged.

Five appended Engine tests check arbitrary text (multiword, long ASCII, non-ASCII and empty), lists, objects, two valid faces, and current null/boolean/number/short-word descriptions. Each private fixture produces exactly one refused drop event with no action, policy request or queueing. The face fixture is parsed and validated as `lamp_msgs::Faces`; its five-second condition remains unmet when the two-second chance expires at logical time three seconds. It checks both the safe count and the exact elapsed/required duration. No process, live bus or wall-clock wait is needed for these new cases.

The `listening` and `idle_state-2` cases record the existing permissive single-word behavior only. The separate short-word disclosure finding from #3007 remains unresolved: those controls do not establish an agreed privacy policy or prove arbitrary short words safe. A future condition-value restriction must update these current-behavior expectations. This follow-up qualifies the masking and missing-explanation faults described above without changing that runtime rule.

## Local qualification

Executed on `409bf72685ec1046ffc2fbfd51654cb2693e1297` with the appended fixture identified in `inputs.sha256`, Rust 1.96.0, aarch64-apple-darwin, one Cargo build job, incremental compilation disabled and an owned target. All 7,122 tracked lampOS source entries were fixed within each phase. The verification files were added afterward.

From the lampOS root:

```sh
cargo +1.96.0 fmt --all --check
cargo +1.96.0 clippy -q --locked --offline -p lamp-skills --all-targets --message-format=short -- -D warnings
cargo +1.96.0 test --locked --offline -p lamp-skills --no-run --message-format=json
cargo +1.96.0 test -q --locked --offline -p lamp-skills --no-fail-fast -- --test-threads=1
```

Formatting, strict Clippy and compilation pass. The full crate reports **522 passed, zero failed, nine ignored** across six summaries: 157/0/0, 20/0/0, 312/0/1, 2/0/8, 31/0/0 and 0/0/0 (passed/failed/ignored). Artifact hashes were captured before the full run and checked afterward.

The corrected focused command `cargo +1.96.0 test -q --locked --offline -p lamp-skills --test it welcome_stuck:: -- --test-threads=1` passes **7/0**, with 306 filtered. A cross-crate grep of all new helper/test names and `welcome_stuck` finds only this private Skills test module and its existing registration. No downstream crate imports these helpers.

## Separate negative controls

A bounded private controller retained the qualified integration executable, verified a fresh artifact for each mutation, ran the single exact test `welcome_stuck::drop_mask_hides_arbitrary_text`, and restored original runtime bytes and mode in each cleanup. Each mutation was attempted once. Compiler errors, setup failures and timeouts did not count as a detected fault.

| Variant | Expected observation | Actual result |
| --- | --- | --- |
| Qualified baseline | All module cases pass | 7 passed, 0 failed, 306 filtered |
| Return raw text instead of `a text` | Exact held-condition assertion rejects the exposed synthetic sentence | 0 passed, 1 failed, 312 filtered; exit 101 at the intended assertion |
| Suppress the non-daily held-condition suffix | Exact suffix assertion fails while the ordinary drop note remains | 0 passed, 1 failed, 312 filtered; exit 101 at the intended assertion |
| Restored source, rebuilt artifact | All module cases pass again | 7 passed, 0 failed, 306 filtered |

The restored full source inventory equals the positive candidate. `results.json` records sanitized commands, exit codes, counts, source checks, log hashes and executable identities. Original logs and compiled artifacts are retained privately. No controller or Python code is added to the repository.

## Earlier observations and limits

The first local full gate, on `a6ccb1aa3`, failed all five new fixtures before their assertions because the required policy declaration was missing; its 517 existing passes, five failures and nine ignored remain recorded. Focused repairs then exposed an unsupported `present = false` fixture and a missing duration in the expected explanation. Those failures remain in the evidence; they are not the qualified negative controls. The final fixture uses the loader's real policy, light-action and timed-face contracts.

These tests cover Engine input and journal descriptions. Generic unexpected JSON is not typed-bus acceptance; the face payload is explicitly typed and validated. No actual face, person, hardware, live service, acoustic latency or physical behavior is involved. Exact 32/33-byte boundaries, a trigger rearm cycle and hardware acceptance remain outside this change. Runtime switches, defaults, cancellation, forget handling and topic schemas do not change. No local full-workspace or ARM64 gate ran; the PR's native ARM64 and Linux Clippy results are recorded separately after publication.
