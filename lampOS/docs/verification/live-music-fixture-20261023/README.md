# Live music fixture: shared worker-state contention

On public `lamp-voice` `43bc29f9b3163e03165148813c27af9e073e615c`, a
controlled wait on the actual process-wide worker-state lock reproduces the
reported Live music fixture failure. Taking that lock before starting the
fake Skills peer preserves the existing contract under the same wait.

These are focused native Mac controls, not a full crate gate, ARM64 CI or a
lamp result. The machine reported `Darwin arm64`; Rust was 1.96.0 with host
`aarch64-apple-darwin`. No provider, real credential or device was contacted.

## Source and historical failure

The [original report](https://github.com/autonomous-ai/lampOS/issues/1569#issuecomment-6349047038)
records one failure in a parallel Linux laptop run and five isolated passes
on base `398921996` plus #3107. At that PR's exact head `b1b8ab445`, line
11513 is `music_accept(&listener).unwrap()`, before `read_line` at 11516.
The report described a socket read timeout; its line instead points inside
the bounded accept helper. Its historical raw output was not available to
this read, and the exact cause of that historical run remains unproven.

The source ordering is the same on `43bc29f9b`: the fake peer starts its
three-second accept deadline before `LiveRig::start_with` obtains
`ONE_THREAD`. Another fixture in the same libtest process can hold that
mutex while the peer times out and closes the listening socket. Later
dispatch then immediately returns unavailable. This specific contention
requires same-process parallel tests; it does not explain unrelated tests
isolated into separate nextest processes.

## Controlled comparison

Both invocations ran this command from `lampOS/`, with two Cargo build jobs,
incremental compilation disabled and dev/test debug information disabled:

```sh
cargo +1.96.0 test -q -p lamp-voice --lib --offline \
  provider_worker::tests::requested_audio_crosses_the_live_worker_only_after_its_tool_reply \
  -- --exact --nocapture
```

The temporary patch starts another thread that owns the real `one_thread()`
guard, acknowledges ownership, then holds it for a bounded five-second
channel receive. This deliberately exceeds the existing three-second accept
budget. It adds no product delay, network request or new production hook.
The same temporary contention is present in both variants.

| Variant | Command result | Test output | Whole command |
| --- | --- | --- | --- |
| Original ordering | exit 101 | 0 passed, 1 failed, 0 ignored, 3,498 filtered; 5.05 s | 55.564 s |
| Lock before fake peer | exit 0 | 1 passed, 0 failed, 0 ignored, 3,498 filtered; 5.10 s | 47.511 s |

The first run produced the accept timeout, then `the executive did not answer
in 0 ms`, then the unavailable reply where the existing test expects
`next: Confirm.`. The second run adds only the early guard to that controlled
fixture and passes both the direct and wrapped call variants, keeping every
existing identity, offer, reply, audio and completion assertion.

The complete logs, temporary patches and command records are adjacent:

- `original_order.txt`, `.patch` and `.json` retain the deliberate failure.
- `lock_before_peer.txt`, `.patch` and `.json` retain the passing comparison.
- `restoration.json` proves the disposable source was restored afterward.

Each invocation hashed all 6,509 tracked regular files under `lampOS/` and
`.github/workflows` before and after the command; every input remained
unchanged during its invocation. The JSON records retain the input-manifest
digest and fixture digest. Executable paths are displayed as `cargo` to omit
the local account path; log bytes are unchanged.

## Scope of the correction

The candidate adds the existing reentrant worker-state guard before creating
the fake peer. It retains all original timeouts and changes no production
bytes or API. The separate decline/music fixture has the same source
ordering, but its module-local `one_at_a_time` atomic is not `ONE_THREAD`;
its assigned owner received that finding. This receipt does not qualify a
correction to that separate fixture or waive the original Linux failure.

## Mac gate and current-base carry

The uninstrumented two-file candidate passed the changed Voice crate gate
on base `43bc29f9b`: **3,766 passed, 0 failed, 14 ignored**, across six result
lines, with no filtering. Formatting passed in 5.136 s; strict all-target
Voice Clippy passed in 34.435 s; the complete Voice command took 326.060 s.
`mac-before-rebase/` retains the exact candidate patch, complete command
outputs and source binding. All 6,509 bound files stayed unchanged.

The patch was committed as `e488aa9ff`, then rebased onto the new integration
head `7b3e6aef8` as `226c4e89b`. `rebase.json` retains the equal range-diff.
Both changed files are byte-identical across that carry. The final clean
`226c4e89b` Mac gate also passes: **3,789 passed, 0 failed, 14 ignored**,
six result lines and no filtering. Formatting took 5.257 s, strict all-target
Voice Clippy 28.998 s, and the Voice command 332.739 s. All 6,512
bound files stayed unchanged. `mac-current/` retains the exact commands,
complete outputs and source binding. Neither Mac pass substitutes for the
official PR qualification below.

## Official PR qualification

Public [PR #3117](https://github.com/autonomous-ai/lampOS/pull/3117), head
`226c4e89b`, passed both official checks on their first attempts:

- [Linux ARM64 run 39846332230](https://github.com/autonomous-ai/lampOS/actions/runs/39846332230/job/120060466263):
  **7,008 passed, 0 failed, 31 skipped**, including one passing test
  classified as leaky. Nextest ran its selected scope across 60 binaries
  in 139.986 s; another 106 binaries were outside that selection. This is
  the PR's selected gate, not a claim of a full unfiltered workspace run.
- The changed Live music fixture passed in **0.053 s**. The leaky result
  was `same_voice::tests::each_line_the_check_says_is_said_to_its_journal`
  and remains in the receipt.
- [Strict Linux Clippy run 39846332487](https://github.com/autonomous-ai/lampOS/actions/runs/39846332487/job/120060466552):
  successful, every crate and target.

Both logs identify the same tested merge,
`a5f0ce4a9470778b8320c6c263358d5f04401b41`, joining head `226c4e89b`
to base `7b3e6aef8`. Both native watchers exited 0. `native.json` records
the exact run, attempt, job, merge parents, selection and output digests.
Complete downloaded job logs remain retained locally: ARM64 1,237,428 bytes,
Clippy 36,048 bytes, each bound by SHA-256 and its primary job URL. Embedded
Cargo result lines are retained separately and are not added again to the
nextest total. Publication was skipped, as expected for a pull request.

Fourteen's independent source and combined-fixture read found no P1, P2 or
P3. First owns later current-base integration and merge. These checks do
not establish a played-audio, physical-lamp or historical-failure result.
