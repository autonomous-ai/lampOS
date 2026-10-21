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
