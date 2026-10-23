# Corrected updater prerequisite review

PR #3269 at `46f5f384ad94f9c2dcc5f7050991ef4325621384`, on
`810e05109bced3beca88a5b602930cfed5fba4e5`. Independent Intel Mac run,
Rust 1.96.0, clean detached checkout and a fresh checkout-specific target.

```sh
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/private/tmp/third-3269-46f5-target cargo test -q --locked --offline -p lamp-update --test round head_touch:: -- --test-threads=2
```

**28 passed, 0 failed**, exit zero, 504.10 seconds of test runtime.
[Raw output](corrected-head-tests.txt) and [hashes](corrected-head-hashes.json)
are retained. The original fourteen tests remain an unchanged byte prefix;
the corrected head also retains both independently reported regression cases.
No source or test edits were made in this review checkout.

Both previous P2 findings are resolved:

- The effective Controls, Expression, Voice head and enabled physical-privacy
  roots must agree. The parser follows each runtime's flag/environment/default
  precedence and refuses mode overrides and expanded semantic paths it cannot
  determine. The original split-bus refusal control now passes.
- Projection follows active build-owned product pins; verification follows
  the active Controls pins path. Unused fixed product pins do not veto a
  build-local update, and custom active pins remain untouched and checked.
  Verification runs again after build-path relocation and before starts.
  The original build-local-pins control now passes.

The independent parser and active-pins/recovery source reads found no further
P1 or P2. Third's two P2 HOLDs on this prerequisite are lifted on this head.
The separate #2948 combined-carry HOLD remains pending its updated head and
existing product acceptance requirements.

Official [ARM64 39956841003](https://github.com/autonomous-ai/lampOS/actions/runs/39956841003)
and [Linux Clippy 39956840831](https://github.com/autonomous-ai/lampOS/actions/runs/39956840831)
completed successfully. ARM64 reports 11,067 passed, 0 failed, 69 skipped,
with one passing Voice LEAK; docs 4/0 and process harness 12/0. Sign/publish
and failure reporting were skipped. The checked merge `690ab0d43` has parents
`810e05109` and `46f5f384a`; its tree `c069dbad50071717a8cab4827d39ebc08bcac08b`
equals the authored tree. These workflow statuses and tree equality were
independently checked; the full native suite was not rerun locally.

The author's actual previous-helper receipts for releases 6408 and 6439 each
report 6/0 using the old ARM64 helper with a current Mac coordinator and fake
systemd. They were read, not rerun here. Supported-pair selection remains with
First; positive signature verification awaits Migrate's trusted public key.
The earlier Skills-dependent Mac failure remains disclosed and unwaived.
No ARM64 linking on this Mac, lamp operation, actual power cut, acoustic
latency measurement or physical privacy acceptance is claimed.
