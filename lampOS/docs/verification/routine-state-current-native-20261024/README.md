# Routine state: current native qualification

This receipt qualifies [PR #3323](https://github.com/autonomous-ai/lampOS/pull/3323) head `6c8c52e371353b20fc729ffcdbcccaf839a46321` on base `9ed44f88c573837c50451c94f4d88f010e8a6721`. All three checkout logs identify merge `4c4c34bd5aac5d389af9f29463fdff04b6fb2423`; its API parents are exactly that base and head. No later rebase is qualified here.

| Run | Result |
|---|---|
| [Dedicated released-binary cases 40056204113](https://github.com/autonomous-ai/lampOS/actions/runs/40056204113) | 4 passed, 0 failed, 0 ignored, 0 filtered; 1.99 seconds |
| [Ordinary ARM64 40056204150](https://github.com/autonomous-ai/lampOS/actions/runs/40056204150) | 11,164 passed, 0 failed, 75 skipped; 199.161 seconds |
| [Strict x86 Clippy 40056204205](https://github.com/autonomous-ai/lampOS/actions/runs/40056204205) | Passed across the workspace and every target; no Rust warning/error diagnostic |

The ordinary total comprises **11,163 PASS rows and one LEAK-marked pass**, `same_voice::tests::each_line_the_check_says_is_said_to_its_journal`. That leak is included in 11,164, not an additional clean pass. Four doctests, the separate 12-case process harness, the relay check and Python publisher/artifact/real-CLI suites of 35/12/7 also passed. Nested captured output after the nextest summary is not counted again. Skipped names were not emitted. The publication job was skipped. A watch command had a TCP reset; final run metadata and the independently downloaded log confirm CI success.

The dedicated fixture used Linux aarch64 and Rust 1.96.0. It ran:

```text
cargo test --locked --release -p lamp-skills --test release_pair -- --ignored --nocapture --test-threads=1
```

The two actual published `lamp-skills` readers/writers are distinct:

| Release | Source | Bytes | SHA-256 |
|---|---|---:|---|
| 6408 | `ee53765c8d014eb00e41c1a5a7054d2546c18bdb` | 8,807,104 | `2c6bc8f34c9fd4a56784a54fa2d03b1e76be9d1833ccf37db905f88dba67d3d5` |
| 6498 | `846b3d203fc00ec3886d71f7673d8cc29cc4a634` | 8,786,048 | `87e500127793db88df8bcd7af793879faeee95f36a8e0d2ef8483c59fef26984` |

Both directions cover daily claims and on/off state. Daily schema-2 consumed sets retain previous entries while growing from one to two to three claims; repeated claims are explicitly refused. On/off reads preserve exact state/manifests, opposite writes change them, and the original release reopens and restores the intended state. The case records contain 22 matched spawn/reap pairs: 10 SIGKILL exits and 12 exit-zero commands. All cases finish with unchanged executable digests. Six explicit persisted-state refusals and 16 snapshot manifests were checked; all 68 present snapshot files match their recorded SHA-256 and size. Snapshot copies are deliberately written mode 0600; their manifests record the original live modes, which must not be inferred from the retained copy permissions.

All **6,972 captured source files** were compared against the exact authored Git tree, not only the fixtures. `git ls-tree -rz HEAD:lampOS` supplied every path and blob; one `git cat-file --batch` read all 5,861 distinct blobs (184,487,233 bytes), and every SHA-256 matched both captured manifests: zero missing, extra or mismatched files. Before and after manifests are byte-identical. The authored and tested root tree is `ef31b78bbeba5f6eae9cda6da8d32474af62b230`. The captured manifest digest is `c3771a235891a1604f43bbc9638dc5e772b8fec36b979ad6de03728e8c33b7a7`; the independently generated authored map digest is `3f0e5e5704a909486975111577d9f2833954c974633c7667d045a79408d84677`.

The dedicated evidence ZIP, artifact `12545124617`, was downloaded and verified against its API size and SHA-256: **620,605 bytes**, `f54e3987e9d3576508888590c33a49352622580afe8f7c0d8ed0735cde0744f7`. All regular UTF-8 case evidence, including state JSON and manifests, is copied byte-for-byte under `pair/`. `skills-tests.log` becomes `skills-tests.txt` without changing bytes. `evidence-map.json` binds every copy; `excluded-case-files.json` explicitly lists omitted binary case storage. No executable, database, archive, Python helper or identity-bearing API object is included.

The ordinary build packaged 47 current binaries and two selected examples. Its artifact metadata is retained in `ci.json`; that payload was not downloaded in this audit. Certificate setup, Node deprecations and Cargo example filename-collision warnings remain recorded. This is synthetic bus/trigger and Policy-refusal evidence: no Policy approval, actuator, rested-state acceptance, real provider, updater installation, device reboot or physical power-loss durability is claimed. Published executable pins were checked by the CI fixtures and retained evidence; this audit did not independently fetch the release binaries again.

The [earlier failed routine attempt](https://github.com/autonomous-ai/lampOS/tree/a2e456672/lampOS/docs/verification/routine-state-failed-20261024) and [successful prior-head proof](../published-state-pairs-passed-20261024/README.md) remain separate evidence. The current result does not erase earlier failures or retroactively qualify another source revision.
