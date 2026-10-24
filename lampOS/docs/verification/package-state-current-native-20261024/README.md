# Package and installed-folder state: current native qualification

This receipt qualifies [PR #3330](https://github.com/autonomous-ai/lampOS/pull/3330) head `22e5f48736fe9b0e796b3783edee76522b0185c2`, tested against integration base `9ed44f88c573837c50451c94f4d88f010e8a6721`. All three checkout logs identify merge `18c505a60f3ec3dbe416abec60f4b2ac02e2a427`; its API parents are exactly that base and head. The authored commit's immediate parent is the routine fixture head `6c8c52e371353b20fc729ffcdbcccaf839a46321`. No later revision is qualified here.

| Run | Result |
|---|---|
| [Dedicated released-binary cases 40058259278](https://github.com/autonomous-ai/lampOS/actions/runs/40058259278) | 6 passed, 0 failed, 0 ignored, 0 filtered; 6.34 seconds: four existing routine cases and two new package cases |
| [Ordinary ARM64 40058259337](https://github.com/autonomous-ai/lampOS/actions/runs/40058259337) | 11,164 passed, 0 failed, 77 skipped; 199.911 seconds |
| [Strict x86 Clippy 40058259706](https://github.com/autonomous-ai/lampOS/actions/runs/40058259706) | Passed across every workspace crate and target; no Rust warning/error diagnostic |

The ordinary total comprises **11,163 PASS rows and one LEAK-marked pass**, `same_voice::tests::each_line_the_check_says_is_said_to_its_journal`. That leak is included in 11,164, not an extra clean pass. Four doctests, the separate 12-case process harness, the relay check and Python publisher/artifact/real-CLI suites of 35/12/7 passed. Captured nested output after the nextest summary is not counted again. Skipped names were not emitted. Publication was skipped.

The dedicated Linux aarch64 fixture used Rust 1.96.0 and ran:

```text
cargo test --locked --release -p lamp-skills --test release_pair -- --ignored --nocapture --test-threads=1
```

Both actual published executables were checked before and after execution:

| Release | Source | Bytes | SHA-256 |
|---|---|---:|---|
| 6408 | `ee53765c8d014eb00e41c1a5a7054d2546c18bdb` | 8,807,104 | `2c6bc8f34c9fd4a56784a54fa2d03b1e76be9d1833ccf37db905f88dba67d3d5` |
| 6498 | `846b3d203fc00ec3886d71f7673d8cc29cc4a634` | 8,786,048 | `87e500127793db88df8bcd7af793879faeee95f36a8e0d2ef8483c59fef26984` |

The package implementation is identical across these releases despite their different executable payloads. This proves compatibility of these exact artifacts, not a package-format code change or arbitrary future releases.

Each package direction runs 30 actual CLI children: 16 successful commands and 14 intentional exit-1 refusals. Across both cases, all **60 spawn/reap/command records** match, with null stdin, empty environments, direct private executable paths and before/after executable hashes. Fast commands are not claimed to have live `/proc` identity verification. Every stdout/stderr digest and exit receipt was checked; no failure-cleanup record occurred.

The actual released commands pack/install version 1.0.0 plus a disabled companion, cross-install unchanged, replace with a separately frozen version 1.1.0, then reopen/reinstall through the original release. All eight positive package payloads match their manifests and source files. Forty install-input checks preserve package bytes and recorded mode. Ten installed-tree snapshots contain 80 verified file hashes; same-package reinstalls and refusal groups preserve exact file bytes, directory membership and recorded live modes. Source folders remain unchanged. Evidence copies do not reproduce the original live permissions; those are recorded in the manifests.

Both installers refuse all seven inputs: newer package format, damaged digest, injected hidden provenance, unknown TOML field, unknown kind, missing speech permission and the tool reserved by the disabled companion. All **28 expected refusals** have exit 1, empty stdout and the specific diagnostic. Twelve inventory checks verify exact rows/functions, including the disabled tool's absence and a mixed valid/unsupported inventory where exit 0 still contains a required `not loaded` row. Installed `.package.json` bytes match the actual package hash and manifest; **Cloud's provenance reader was not executed**.

The four routine cases also passed again: daily consumed claims and on/off state in both directions, with 22 matching process reaps (10 SIGKILL and 12 exit-zero commands), six explicit persisted-state refusals and 68 verified snapshot file hashes. Policy never approves and no actuator runs. These routine results are separate from the package command counts and do not increase the six top-level test count.

All **6,974 captured source files** match the authored Git tree before and after execution. `git ls-tree -rz HEAD:lampOS` enumerated every path; one `git cat-file --batch` read all 5,863 distinct blobs (184,523,668 bytes). Every SHA-256 matched: zero missing, extra or mismatched files. The authored and tested root tree is `66161bec372a8c7774881851e0f9776657714d3e`. The captured manifest digest is `a85dfa2be99cf6a178a94a9b0dce85f744776b3ea0e4ec2eec8187e2b7d7e04a`; the independently generated authored-map digest is `2a7d2f817cc1b58450afb320dc55d4bd6a4c96710a27e2257f0a8469489b6413`.

The dedicated ZIP, artifact `12546092757`, matched GitHub's recorded **646,762 bytes** and SHA-256 `30b391925f4298c88282a4c30d47cc1917332094a1f25127395b9a00eeca17f7`. All 610 regular archive members were extracted with path/type checks. Safe regular UTF-8 case files, including source files, installed snapshots, state JSON, stdout/stderr and manifests, are copied byte-for-byte under `pair/`. The dedicated `.log` becomes `.txt` without changing bytes. `evidence-map.json` binds each copy; `excluded-case-files.json` binds the omitted fixture package archives. No executable, archive, Python helper or identity-bearing API object is published.

Certificate and Node warnings, plus ordinary Cargo example filename-collision warnings, remain recorded in `ci.json`. Ordinary ARM64 packaged 47 binaries and two selected examples; its artifact was checked as metadata only, not downloaded here. There were no validator errors or failed test attempts in this qualification.

This is local unsigned package and selected routine-state evidence. Store signatures/trust, Cloud provenance consumption, the complete product skill catalog, updater folder restoration, rested state, real providers, live reloading, physical output and power-loss durability remain outside scope. The [prior routine qualification](../routine-state-current-native-20261024/README.md) and [earlier failed routine attempt](https://github.com/autonomous-ai/lampOS/tree/a2e456672/lampOS/docs/verification/routine-state-failed-20261024) remain separate. Passing this row does not erase earlier failures or qualify a later source revision.
