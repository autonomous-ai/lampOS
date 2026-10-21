# Claude Code entry point

Treat this directory as the standalone lampOS repository root. Read
[AGENTS.md](AGENTS.md) for project rules and [HANDOFF.md](HANDOFF.md) for the exact
implementation, device, test and unfinished-work checkpoint.

The owner's scope is Rust-only desk interaction V2, with English-only notes and
documentation inside lampOS. Parent legacy Python/Go and Vietnamese documentation
requirements do not expand this release. Preserve the installed rollback and the
motor inhibit; do not commit or push without an explicit request.

## Communication Language

Use English by default for all user-facing communication, including progress
updates, questions, and final responses. Switch to Vietnamese or another
language only when the user explicitly requests it; do not infer a language
switch solely from the language of their message.

## Command Output

Keep command output small: long build and test logs use up an agent's
usage limit. Build and lint quietly (`cargo build -q`,
`cargo clippy -q --message-format=short`), and test with
`cargo nextest run --status-level fail --final-status-level fail` or
`cargo test -q … | tail -40`, with `set -o pipefail` so the pipe keeps the
test's failure. Send a whole log to a file and read only its result lines
(`grep -E '^test result|FAILED|panicked'`). On a lamp, read `journalctl`
through `grep` or `tail`, never a whole log.
