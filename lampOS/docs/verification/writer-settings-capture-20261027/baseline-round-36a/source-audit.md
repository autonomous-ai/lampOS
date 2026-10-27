# Raw gate lock failure: source audit and one baseline control

Observed failure, not a diagnosis: full Update on raw head
`4f25de19abc3d98780a5796cf6cf96539b3863d6` / Main
`3f4b87969113a89795c814051501c4ba581418e9` completed 509 passed,
1 failed, 6 ignored. The sole failure is the second round in
`tests/round.rs:825–835`, `a_round_with_nothing_to_change_only_checks`:
`Failed("another lamp-update is at work")` instead of `UpToDate(2)`.
The failed driver remains preserved at its original SHA
`22bcce49ece2bd6059f496ce2579d064331f97948a3d731174ee86ed4645b3a9`.
It has not been executed again. Queue 6441965369 already routes the issue
to Tests / First; this audit changes no production or fixture code.

## What the source establishes

- `src/records.rs:287–300`: make/open errors retain their own errors, but
  **every error returned by flock** is replaced with WouldBlock and the same
  "another lamp-update" message. The retained output does not identify the
  original errno, lock owner, descriptor or process. It cannot distinguish
  actual contention from another flock error. `Lock` owns a private File;
  there is no custom Drop, explicit unlock, clone or public FD export here.
- `src/round.rs:428–431` acquires the records lock and returns the displayed
  failure on an error. The lexical lock remains inside the round. Each
  `Lamp::new` uses a new TempDir (`tests/round.rs:426–428`); the failed test's
  two rounds use the same Lamp, with no explicitly held competing lock.
- `tests/round.rs:372–405`: FORKS is process-local. `forking` takes its write
  guard; `no_fork` takes its read guard. `rounding` takes write on Linux,
  read on macOS/other targets. `Lamp::round_observing:617` holds its guard
  across the complete round. Custom direct-round wrappers in asleep_quiet,
  restart_quiet and product_carrier also take rounding before their call.
  `Lamp::confirm:2804` takes write for its complete operation; back:2828,
  self_cuts::back_error:212, provisioning:716 and install:2559 take read.
- All direct ordinary fixture subprocess launch sites found by the full
  round.rs / round/*.rs census have a forking write guard: bounded query
  control:1170; direct skill list:1842 and listed:1915; immediate-exit
  updater:2898; self_cuts::executable:68; every runnable_event_controls test;
  setup_lifecycle mkfifo:156. Controller threads there do not launch a
  subprocess and are joined while their enclosing guard remains held.
- `round/process_cuts.rs` is Linux-only, including the explicit libc::fork.
  It asserts a sole-thread custom-driver precondition; its custom entrypoint
  is not called by the ordinary libtest binary. It is absent from this Mac
  control. Its explicit command launch also takes forking:302.
- Production `src/round.rs:3051–3060` directly spawns `lamp-skills list`.
  It is reached under Mac's **shared** rounding guard. The project comment
  explains this with an atomic-Mac-spawn assumption. No platform syscall or
  inherited-FD observation was made here, so that assumption is not promoted
  into proof that inheritance either did or did not occur.

## Concrete guard coverage gap; not the observed failure's cause

`no_second_round_begins_while_lamp_updates_own_switch_waits` at
`tests/round.rs:2969–2974` returns a live records Lock from an inner
no_fork scope. That read guard ends while `held` stays live until the
later `drop(held)`. Thus the intentionally held descriptor is outside the
full-lifetime FORKS coverage and another fixture launch can occur while
it is open. This is a source-level coverage gap for Tests to assess, not
proof of an inherited descriptor at runtime. It is a **different fixture
root/test** from the failure at line 830 and does not explain that failure.
No opportunistic fix, extra probe or production error-policy change was made.

The output rows do not supply fork/lock timelines. All fifteen raw controls
and eighteen foundation controls passed in the failed gate; that observation
neither establishes a cause nor waives the gate failure.

## Prepared baseline, not executed

Isolated worktree `<baseline-tree>`, branch
`second/raw-lock-baseline-20261027`, is exactly unmodified public Main
`36a42f30a756b6fd6f31c9b6102cb66e5008416c`, with no Raw or Receipt patch.
All thirteen round fixture/module plus Records files are byte/mode identical
to the failed source. The baseline source proof and preserved failed driver
are bound in `second-raw-lock-baseline-fixture-proof-20261027.json`.
Raw observer/capture modules are absent; the receipt merge_for API is absent.

Prepared controller `second-raw-lock-baseline-gate-20261027.py` checks live
Main before any Cargo and refuses an advanced base. It guards tracked
lampOS/workflow source/modes, clean head, Cargo configuration and its own
hash; verifies the closed 8ec actual/preserved Cloud-target boundary before
reuse; compiles only the complete round target with compiler JSON; retains
the actual produced round driver; runs exactly one complete round cohort
with four libtest workers and no filter; binds actual Running and post-run
hashes to the preserved driver. No shared Update target is touched.

Planned runtime, only after explicit slot release:
`cargo +1.96.0 test --locked -p lamp-update --test round --no-fail-fast -- --test-threads=4`.
Normal locked metadata and a no-run JSON compile precede that one execution.
There is no serial run, selected-case run, failed-source retry or new test.

Classification is fixed before execution: a passing baseline is
**INCONCLUSIVE**, not a waiver or diagnosis. The same first-attempt failure
proves a baseline failure of that unchanged target, while its actual errno
and mechanism remain unestablished. Other failures or incomplete output are
retained without retry. No Cargo or runtime has started at this checkpoint.
