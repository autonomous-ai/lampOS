# Onboarding steps and words in the skill

Part of #2824. This changes the existing onboarding path where
`LAMP_ONBOARDING=on`, with no new switch. The shipped skill preserves the
previous walk data. The lamp-52e6 acceptance walk remains open.

## Data and mechanism boundary

`skills/onboarding/skill.toml` owns the eight-step order, each first and
re-offer note, the re-offer interval, optional nudge words and answer counts,
and the coaching and completion words. Voice retains stable mechanism IDs,
physical evidence, privacy priority, cancellation and interaction timeouts.
Its production built-in order and words are removed. The old prompt bytes
are an independent `cfg(test)` oracle, absent from production.

A root audit compared 40 prompt fields against the original code on
`3095487ad6330c89566e7d6db6208dcc882f47e9`: all matched byte for byte,
as did the eight-step order and 86,400-second re-offer interval. The source
captures used for the comparison also match that Git base. The runner tests
exercise changed order, words, nudge count and re-offer interval; the
unchanged semantic tests continue to compare against the original bytes.

Voice and Skills share the checked data schema. It requires all eight IDs
exactly once, complete first/re-offer words, bounded interval and nudges,
nonempty bounded notes and exact slots. The privacy location's maximum
size is included in the written-note budget. Name confirmation checks its
expanded size and omits an oversized note without cutting the name or
claiming confirmation. Slot data is filled once.

## Loading and activation

Voice reads the installed regular manifest once before audio opens, bounded
to 65,536 bytes, without following its leaf symlink or waiting for a FIFO.
All walks and mute/reopen cycles in that service invocation share the same
immutable snapshot. Turns perform no skill-file I/O. The service lifecycle
test replaces/restores the file between real cycles, checks the automatic
walk through its loaded snapshot, and checks activation in a fresh service.

A later skill-only edit, update or restoration needs the next
operator-managed Voice restart. The existing updater installs product
skills before restarting changed binaries, so the initial binary/data
rollout has its data in place first. Skill-only updates restart nothing.
No updater lifecycle change is part of this PR.

Missing, unreadable, disabled or invalid data disables the automatic walk
and invitations for that service invocation, with one fixed diagnostic
and no built-in fallback. Physical privacy waiting and cleanup and the
storage owner continue. The existing environment-controlled `setup_step`
and `redo_setup` provider tools remain declared as before.

## Host validation

The complete host gate passed on host-tested base
`22061b427805e4f6131cd5b15f2f1f287107fdf2`. Raw commands and terminal
results are retained under
`/private/tmp/fourth-onboarding-skill-checks-20261022/`.

The initial strict Clippy errors were fixed (Darwin FIFO fixture API,
unused imports and an owned-string test adaptation). Formatting and strict
workspace Clippy subsequently passed. The first sandboxed focused run had
118 passes and ten failures: eight socket permission denials and two new
fixture defects. One fixture observed the runner before asynchronous
storage settled; another marked a measured step done without its required
evidence. Both fixtures were corrected. The current-base authorized
focused run passes all 128 checks, zero failed or ignored. Initial failures
remain in `initial-clippy.log` and `onboarding.log` and are not waived.

The complete gate passes **5,351 / 0 failed / 23 ignored**, counted once:
Voice 3,509/0/14 (library, binary, serial integration and two test-enabled
examples); Skills, Memory, Cloud and Update 1,219/0/9; Voice-eval 623/0/0.
Six doctest target groups passed with zero doctests. A target audit found
one result for each of 50 Cargo targets, with no nested-harness duplicate.
The 128 focused and two isolated timing checks are repeats and are not
added to this total.

Exact commands, durations, target counts and checked source hashes are in
[`validation.json`](validation.json). They include `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, the affected crate
tests and `cargo build --workspace`. Voice and Voice-eval integration
binaries were serial from the outset. The long Update recovery fixtures
passed; the dependent group took 827.681 s. Native ARM64 and Clippy on the
published head remain the readiness gate.

## Rebase integration

The complete authored patch rebased unchanged onto `ebc5e5c9f` and then
`fdf8914f3`, including the independently owned Skills management, worker
switch census and memory retries. Source-only range-diffs:
`ee8d1d7bc = 134df3b21` and `134df3b21 = 03adaafdf`.
The final evidence amendment changes documentation only.

Fresh formatting, strict workspace Clippy and workspace build pass on
that base. Fresh tests: **128 onboarding / 467 Skills / 5 switch-census**,
all with zero failures or ignored cases. These repeats are not added to
the original 5,351 total. `post_rebase` in the JSON records exact commands,
timings and current source hashes. Raw:
`/private/tmp/fourth-onboarding-skill-checks-20261022/post-memory-rebase-20261022/`.

## Current-base replay after native qualification

The qualified `f6431ebcb386a25b806e0db87b5842607c8d93af` head was
rebased onto `4f82e1f1bee098e5524298f1307102e22391508e` as
`73890de01886027dbafb69114a283e05c8f42c31`, before this evidence amendment.
The sole conflict was the adjacent rows in `docs/skills.md`: the onboarding
row was inserted and the current base's reminder-budget row was preserved
byte for byte. All 14 authored Rust/manifest changes are identical after
ignoring only Git index IDs and hunk offsets. Later coordinator sound
continuation, Skills kept-off store and made-skill management changes are
retained. The current switch census and add-on source are byte-identical
to the parent.

Fresh formatting, strict workspace Clippy and workspace build pass. Tests:
**128 onboarding / 477 Skills / 5 switch-census**, all with zero failures;
Skills retains one ignored fixture writer. These 610 passes are focused
repeats and are not added to the full host total. Exact commands, timings
and current source hashes are recorded in `current_base_replay` in
`validation.json`; the earlier 5,351 host total
and native 10,238/0/63 qualification retain their original heads and bases.
Raw source audit, full range-diff and command logs:
`/private/tmp/fourth-onboarding-skill-checks-20261022/rebase-latest-20261022/`.
The replacement head needs its own ARM64 and Clippy runs before readiness.

## Replay after the task-news table changed

After the parent advanced, `468d655f98b989b95f25631cacba0b206e1c393d`
conflicted with the task-news table row; GitHub scheduled no native run for
that conflicting head. The branch was replayed onto
`fb5d82a8782ac46df9b5ac7df5e1834582c0d9d0` as
`7e90bcc91799493b7a7307b0ef217e633f8fc16f` before this evidence amendment.
The current news and reminder rows are preserved byte for byte. The full
onboarding schema and lifecycle information now has its own subsection
outside the table, avoiding this repeated adjacent-row conflict.

All 14 authored Rust/manifest patches remain unchanged. New parent Skills
guard-tool wording and Task-news optional folder-failure words are retained;
neither changes the onboarding validator or its shared word-check helpers.
Fresh formatting and focused Skills onboarding/guard tests pass on this
parent; exact commands, counts, timings and source hashes are recorded in
`conflict_free_replay`. The 610/0/1 focused count, strict workspace Clippy
and workspace build remain attributed to `4f82e1f1b`. New native whole-workspace
ARM64 and Clippy runs gate this replacement head. Raw audit and range-diff:
`/private/tmp/fourth-onboarding-skill-checks-20261022/rebase-fb5d82a87-20261022/`.

## Consent boundary after Reviewer-lead's P2

Reviewer-lead's P2 on `a3f13a013c1ad850d6abf51704a039b0c6a8c0d3`
found that structurally valid replacement notes could omit consent.
`Definition::checked()` now retains two exact consent clauses in private
code constants. Both `first` and `reoffer` notes for `who_you_are` must
include `only after a clear yes call remember_my_face`; both notes for
`room_sounds` must include `done only after a clear yes`. A missing or
altered clause is refused by the shared validator before Voice uses the
installed plan or Skills loads it. Packaging reaches this validation too;
valid core data still receives the existing separate core-package refusal.
The shipped manifest and rendered notes remain byte-identical. Surrounding
prose stays editable. There is no new public API, I/O or turn-path work.

Two new test functions exercise the same eight bad note mutations across
Voice parsing/installed-file loading and Skills loading/packaging: two
steps, each first/re-offer, with the clause removed or altered. Package
failures must name the consent validation rather than pass vacuously on
the core-package policy. Four positive surrounding-prose edits still load;
the restored shipped core manifest loads and receives the existing package
policy error. These table cases are not counted as separate test functions.

Fresh host gates on authored parent `fb5d82a87`: formatting, strict workspace
Clippy, workspace build, **129 onboarding / 479 Skills / 5 switch-census**,
all with zero failures. Skills retains one inherited ignored fixture writer.
These **613 / 0 / 1** focused results are not added to the historical full
5,351 total. The earlier ARM64 10,289/0/66 receipt belongs only to the
pre-fix `a3f13a013` head; the replacement head needs fresh native gates and
a P2 reread.

Initial failures are retained: the new fixtures first used TOML display
support not enabled by Voice, then wrongly expected a valid core skill to
be packageable. Both test defects were corrected. An unchanged Cloud
compile saw `MessageOut` fields from the newer callback worktree's shared
target artifact; a targeted `cargo clean -p lamp-msgs` resolved it, with
strict workspace Clippy passing and no Msgs or Cloud source edit. The source
paths, hashes, definitions and diagnostics are retained in the cache audit.

The prior structural-only validator is checked separately as a red control,
then the exact passing source is restored for two focused green repeats and
an isolated startup/cache measurement. Both regressions fail against the
entire validator file restored byte for byte to `a3f13a013`: the first
`who_you_are` note without its consent clause is accepted. Both pass after
the fixed source is restored byte for byte. The isolated post-fix read and
validation takes **2.661 ms**; a cached selection pair averages **154 ns**
over 10,000 iterations after file removal, meeting the same local targets.
The two green controls and timing case are repeats, excluded from 613. Exact terminal results and source
hashes are recorded in `consent_boundary_fix` in `validation.json`. Raw:
`/private/tmp/fourth-onboarding-skill-checks-20261022/consent-p2-20261022/`.
The fixed content-free service diagnostic and missing/invalid-file no-walk
behavior noted as P3 remain unchanged and documented. The authorized
lamp-52e6 acceptance walk remains open; no device action was performed.

## Latency and product limits

The local targets are startup read/validation under 50 ms and a cached note
selection under 1 ms. Isolated test-thread measurements meet those targets:
manifest read/validation **37.063 ms**, service entry through loaded plan and
storage to the active callback **13.697 ms**, and the missing-file service
case **1.448 ms**. A pair of cached note selections averaged **195 ns** over
10,000 iterations after the file was removed. These are separate single
host observations, not a sum, cold-device measurement or acoustic boundary.
Parallel test output interleaves timing lines and is not used as a timing
receipt.
There is no new turn-path disk read, polling loop or worker.

Fourth has no lamp and performed no SSH, install, restart, movement or
played speech. Host tests do not prove spoken behavior, device latency or
the same-steps acceptance walk on lamp-52e6. That operator gate is required
before #2824 is complete; this evidence does not close it.
