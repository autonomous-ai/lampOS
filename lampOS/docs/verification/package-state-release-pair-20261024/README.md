# Published local-package and installed-folder readers

This is the narrow package continuation of #2809, based on the routine-state
fixture at `6c8c52e371353b20fc729ffcdbcccaf839a46321` (integration base
`9ed44f88c`). It adds two native-only cases to the existing four-case Skills
`release_pair` target. It changes no runtime code or fixture assertion in the
four routine cases.

**The original native qualification passed.** Head `22e5f4873` executed
all six selected cases, including both package directions, in dedicated
ARM64 run **40058259278**. Ordinary ARM64 **40058259337** passed
**11,163 PASS + 1 LEAK**, zero failures, 77 skipped; Linux Clippy passed.
The Mac host gate passed **514/0/7 ignored**. These results are historical
evidence for that head, not new-base execution or product acceptance.

| Host command | Result |
|---|---|
| `cargo +1.96.0 fmt --all --check` | Passed; 5.87 s |
| `cargo +1.96.0 clippy --locked --offline -p lamp-skills --all-targets -- -D warnings` | Passed; 21.12 s |
| `cargo +1.96.0 test --locked --offline -p lamp-skills` | 514 passed / 0 failed / 7 ignored / 0 filtered; 62.76 s |

The test total is 157 + 20 + 306 + 31 passing tests. The six actual-release
cases and one existing test were ignored. Complete command, output and exit
records are retained locally for the final publication. The package source
was not changed after these gates; this status paragraph was added afterward.

| Case | Direction |
|---|---|
| `release_6498_installs_6408_packages_and_6408_loads_the_replacement` | 6408 → 6498 → 6408 |
| `release_6408_installs_6498_packages_and_6498_loads_the_replacement` | 6498 → 6408 → 6498 |

The driver is [`package.rs`](../../../crates/skills/tests/release_pair/package.rs).
It reuses the original fixture's pinned artifacts, bounded child ownership,
exit receipts, transcript and final artifact checks. All six cases remain
ignored in ordinary host tests; the native target executes them explicitly.

## Frozen inputs and executable identity

| Release | Source | Bytes | SHA-256 |
|---|---|---:|---|
| 6408 | `ee53765c8d014eb00e41c1a5a7054d2546c18bdb` | 8,807,104 | `2c6bc8f34c9fd4a56784a54fa2d03b1e76be9d1833ccf37db905f88dba67d3d5` |
| 6498 | `846b3d203fc00ec3886d71f7673d8cc29cc4a634` | 8,786,048 | `87e500127793db88df8bcd7af793879faeee95f36a8e0d2ef8483c59fef26984` |

These are different published executables. Their `skills/src/package.rs`
and package-management path in `main.rs` are identical. The loader difference
in `lib.rs` is presence-duration bookkeeping, which these offline tool-only
fixtures do not invoke. No storage implementation change is claimed.

Each case checks byte length, SHA-256, ELF64 little-endian format and AArch64
machine type, then copies both artifacts into its unique private `kp-*`
directory. Copied executables are mode 0555; the containing directory is 0700.
Every command directly executes the pinned path, with before/after byte
comparison, exact argv and an empty environment recorded. Fast CLI processes
can finish before `/proc/<pid>/exe` is inspected, so this fixture makes no
live-process `/proc` identity claim. Every spawn is owned immediately and
reaped; failure cleanup is bounded.

## Sequence and counterexamples

Before execution, the fixture creates separate immutable version-1.0.0 and
version-1.1.0 source folders, a disabled companion and a duplicate-tool
candidate. Each has synthetic English metadata, one inert text asset and a
speech step that is never executed. Source directories are made 0555 and files
0444; modes and hashes are retained before and after the case.

For first executable A and opposite executable B:

1. A runs `pack --out packages/pack-a sources/v1/pair-kit` and
   `install --dir installed packages/pack-a/pair-kit-1.0.0.tar`. A also packs
   and installs the disabled companion.
2. B runs `list --dir installed`. Exact rows must show version 1.0.0 and the
   disabled companion; only the enabled fixture tool reaches the summary.
   B then installs A's package, leaving the entire installed tree unchanged.
3. B packs the separately frozen version-1.1.0 source and installs it. A reads
   the replacement with `list`, then installs B's package. Exact bytes and
   modes must remain unchanged by that same-package reinstall.
4. Both released installers receive seven invalid/conflicting inputs: package
   format 2, a damaged file digest, a hidden `.package.json`, an unknown TOML
   field, an unknown skill kind, a missing speech grant, and a tool already
   reserved by the disabled companion. Every command must exit 1 with the
   specific refusal, emit no successful stdout, and preserve every installed
   file's bytes/mode and the complete directory membership/modes. Fresh valid
   inventory after each group prevents refusal-only success.
5. A separate inventory combines an actual installed valid package with a
   synthetic unsupported-kind folder. Both readers must report exactly that
   `not loaded` row and only the valid function. `list` returning exit 0 alone
   is not treated as successful loading.

Only actual released commands write the positive package archives and live
installations. Generic tar/JSON handling in the driver constructs the negative
inputs and inspects retained bytes; it never substitutes today's Skills reader
for either released reader. No manifest or source folder is rewritten between
releases to make a positive compatibility case pass.

## Execution and retained evidence

Use an isolated native Linux ARM64 runner. Set `LAMP_COMPAT_SKILLS_6408` and
`LAMP_COMPAT_SKILLS_6498` to the pinned files and `LAMP_COMPAT_OUTPUT` to an
existing private directory such as `/tmp/pair-output`. The harness downloads
nothing and needs no network, account, signing key, lamp service, bus peer,
provider or device access.

```sh
cargo test --locked -p lamp-skills --test release_pair -- --ignored --test-threads=1
```

Each package case has a 90-second overall deadline, each command at most
15 seconds, and failure cleanup at most five seconds. There is one child at a
time, null stdin and private stdout/stderr. There are no daemon startups or
socket namespaces. These are process bounds, not measured lamp latency.

Retained evidence includes artifact/source identities, all command/exit
receipts, raw stdout/stderr, positive and negative package bytes, immutable
source manifests, installed-file snapshots, and final unchanged executable
hashes. Failure evidence is retained too. Exclude the two copied executables
from publication and review/redact any private temporary path while retaining
an explicit redaction receipt. Fixtures contain no personal content or audio.

## Limits

The installed `.package.json` is compared with the actual package's SHA-256
and manifest. That establishes the released writer's bytes only. The Skills
CLI inventory does not consume this provenance record; Cloud's `skills.list`
does, and its actual released reader remains outside this pair.

Local file installation explicitly accepts unsigned packages. It does not
qualify Store signatures, HTTPS/catalog handling, caller authentication,
rollback prevention for signed Store packages, the full product/core folder
set, updater installation/restoration, live runtime reload, output, clock
behavior, or physical power-loss durability. A package version is not a
minimum runtime schema declaration. Unknown fields/kinds/permissions retain
their strict refusal behavior; this test does not make them tolerant.

## Parent integration carry

Parent #3323 landed as `14cdabec5` in train 70. This continuation is now
rebased onto `d7b85b1dd`, with unchanged fixture and workflow. Fresh
current-base gates are required before integration.

[Original native receipts](https://github.com/autonomous-ai/lampOS/tree/fd111929a/lampOS/docs/verification/package-state-current-native-20261024)
retain both package directions, 60 matching child exits (32 successful
commands and 28 intended refusals), 40 unchanged installed-input checks,
80 snapshot file hashes, 12 exact inventory checks and eight actual positive
archives. The four inherited routine cases add 22 owned exits separately.
No Cloud consumption, signature trust or physical update is established by
those package cases.

## Composition over the rested continuation

The package continuation is now stacked over corrected #3327 `e4005f25a` after a
virtual merge exposed conflicts in the shared test entry and documentation.
Both rested directions and both package directions remain selected, giving
eight explicit artifact cases including the four original routine cases.
The fixture module merges both helpers without changing either assertion
sequence. Fresh composed-source gates and native execution are required.
This draft must not integrate before #3327; the final carry onto its landed
base remains the author's responsibility. Earlier six-case receipts remain
separate rather than being relabelled eight-case execution.

The shared absolute reply deadline correction is inherited unchanged from
#3327. Its two ordinary complete/dribbling controls run on the Mac and are
filtered by the dedicated eight-case native selection. Prior separate
six-case receipts do not qualify this composition or deadline correction.
