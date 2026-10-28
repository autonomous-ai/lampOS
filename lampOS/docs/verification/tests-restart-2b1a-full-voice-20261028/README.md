# Same-voice restart fixture: full Voice library run on 2b1a

[PR #3572](https://github.com/autonomous-ai/lampOS/pull/3572), head
`2b1a8ad1953799d8edb3f7581a46b87ee438a46e`, on lamp-voice
`9bde7441477c7d61e1d523fc9694aa00a69c966e` after Reader #3543 was delivered
(train 276, #3964). Test fixture only.

Taken on the office Mac (x86_64), 28 Oct 2026, started 07:48:55 GMT+7, in this
order, each once, from `lampOS/`:

| Step | Command | Result |
|---|---|---|
| Format | `cargo fmt --all --check` | exit 0 |
| Strict Clippy | `cargo clippy` over Voice and every crate that depends on it, `--all-targets -- -D warnings` | exit 0 |
| Workspace | `cargo build --workspace` | exit 0 |
| Compile | `cargo test -p lamp-voice --lib --no-run` | exit 0 |
| Inventories | the test binary's `--list` and `--list --ignored` | exit 0 |
| Full run | `cargo test -p lamp-voice --lib`, default test threads | **3,898 passed, 0 failed, 15 ignored**, 242.90 s |

All 21 `same_voice::tests` cases passed once inside the full run, with no filter, retry or rerun. Each step's command, start time
and exit are in its `cmd-*.json`, and its output is in the `.stdout.txt` and
`.stderr.txt` beside it, unedited.

The head's own checks are green on their first attempt:
[ARM64 40611972727](https://github.com/autonomous-ai/lampOS/actions/runs/40611972727),
[clippy 40611972766](https://github.com/autonomous-ai/lampOS/actions/runs/40611972766),
[CI 40611968450](https://github.com/autonomous-ai/lampOS/actions/runs/40611968450).

The earlier failures keep their own records:
[a17 full run 3,882/1/15](../tests-restart-a17-full-lib-failure-20261028/), and the
[matched Main a17 control](../tests-restart-main-a17-control-20261028/), which
failed on the same Reader assertion without this change. Reader #3543 has since
landed, and this run passes that case. That is not a claim about the cause of
the earlier failure.

Not shown: a run on a lamp (none was done; this is a test fixture), and the
merged tree, which the train's ARM64 run checks.
