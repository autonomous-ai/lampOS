# Requested-audio fixture clock controls

Base: `e2232b8369fb125af6c79e8bf6927ec590a612e9`. These are native x86_64 macOS fixtures, not device or acoustic measurements. Exact commands, source/executable hashes and restoration records are in `observations.json`.

The loaded-ticket control failed **3/3 before**, at the original `assert!(test.prepare())`, and passed **3/3 after**, covering all five original cancellation/expiry reasons in every after run. Admission, real Start authorization, WAV loading and the original readiness assertion succeeded before each injected pause. All 142 original coordinator assertions remain, including the original expiry, short-lease and retirement checks. The runtime cache-contention cancellation case is byte-identical. No production timing bound, cancellation behavior or contention handling changed.

| Sample | Exit | Host wall seconds | Actual whole-child pauses, ms |
| --- | ---: | ---: | --- |
| before-pause-1 | 101 | 0.348512 | 303.869 |
| before-pause-2 | 101 | 0.340831 | 304.036 |
| before-pause-3 | 101 | 0.347300 | 303.979 |
| after-pause-1 | 0 | 3.923751 | 303.972, 304.753, 304.506, 304.241, 304.387 |
| after-pause-2 | 0 | 1.727500 | 304.834, 304.007, 304.534, 304.193, 304.421 |
| after-pause-3 | 0 | 1.739515 | 304.870, 304.969, 303.857, 304.214, 305.044 |
| watchdog-after | 101 | 1.992067 | none; forced watchdog |
| watchdog-before | -6 | 2.160700 | none; forced watchdog |

The first after sample's 3.923751 s host wall time is retained; libtest reported 1.70 s for that test body. These are control durations, not speedup or device-latency claims.

## Replay and failure boundary

The before patch applies to the stated base; the after patch applies to the submitted fixture. In a disposable owned worktree, apply one patch and compile natively with an exclusive target and two build jobs:

```sh
cargo test -p lamp-voice --lib --no-run --offline --message-format=json
```

Use the emitted `lamp_voice` lib-test executable with:

```text
--exact coordinator::tests::requested_audio::cancelled_gapped_or_expired_music_never_acknowledges_its_confirmation_news --nocapture --test-threads=1
```

Each patch adds a marker and stdin gate immediately after the original `test.load(1)`. A bounded external watcher captures combined output. On each `REQUESTED_AUDIO_AFTER_LOAD_CONTROL` marker, it sends SIGSTOP to that directly spawned test child's PID, injects 300 ms of host delay, sends SIGCONT, and writes one `R` byte to the child's stdin. It records actual monotonic pause duration and enforces a separate real 60 s overall watchdog. It does not retry a failed sample. No host sleep or fault hook enters the submitted source. The watcher is kept outside the repository; its digest and exact executed commands are recorded here.

Both original resume paths invalidate the old recording: if the worker reads first, its cached observation is older than the unchanged 100 ms bound; if the monitor observes first, the missed deadline replaces the old continuity fence. This explanation follows unchanged `SpeakerCache::observe` and `Shared::limit`; it is not a claim to have logged their private state. The requested pause is below the normal 500 ms Skills lease and three-second fixture watchdog.

The after fixture drives the same worker loop, WAV loader, Unix authorization and Finish exchange with explicit park acknowledgments and a shared logical clock. Each real Bus publication is validated when observed; subsequent logical monitor steps keep its original wire stamp and freshness bounds. The coordinator, worker and fake Skills peer share that clock. The mute test observes the actual coalesced mute generation before awaiting Finish. The existing logical receipt/trace assertion and an independent real 250 ms host handoff assertion both remain; the host interval includes both retirement and acceptance handlers. Scope ownership is per test, so shadowed Test values cannot nest or prematurely drop the clock. Readiness shares one original three-second host deadline across samples and the park acknowledgment.

## Forced watchdog unwind

The two watchdog patches replace only the fixture park timeout with `Duration::ZERO`, forcing its actual assertion without a host sleep. The pre-repair fixture (`runtime.rs` SHA-256 `d44eb8934479646578276f3eb01f943860c74373f59caadb9af490a994b18af4`) aborts during destructor cleanup with SIGABRT. The final fixture (`5d76c1b00352bdf4395f0d39c8c5293b2d3ad0b299261df902ce664aa0f2bbf2`) returns an ordinary failing-test exit 101, with the intended watchdog diagnostic and no destructor abort. The repair clears park state and drops the capacity guard before asserting; the wake helper can recover the guard during unwind. These samples compare two test-harness versions, not changes to product code.

## Provenance and limitations

The first baseline artifact was reused only after exact scratch-source and executable-hash verification. The initial launch could not bind its Unix socket inside the sandbox. It reached no positive marker and sent no pause signal; that raw output is retained as infrastructure-only, outside the three baseline reproductions. The authorized socket-enabled run produced the six actual signal samples. An earlier external runner syntax error occurred before source mutation or Cargo; its metadata is retained and its script remains in scratch.

The pause path uses `os.kill(child.pid, ...)` on the runner's directly created `Popen(..., start_new_session=True)` child. No SIGINT or SIGTERM occurs in the runner. Exception cleanup could send SIGKILL only to that isolated child's process group; none of the six completed signal samples entered that exception path. Numeric PIDs were not persisted for the completed children and cannot be supplied retrospectively. `signal-audit.json` records this limit and the exact signal sites. Native two-job compilation ran alongside another full-suite check, so shared host load is a scheduling variable; this record makes no causal claim about other tests' failures.

`raw-output.json` preserves every original UTF-8 byte, including trailing spaces and blank lines. Decode each `utf8` string and re-encode it as UTF-8 to verify its recorded length and SHA-256. `diagnostic-excerpts.txt` is explicitly a whitespace-normalized display, not the raw record. All probe sources were restored to their exact base bytes, and all eight frozen candidate hashes remained unchanged. No binary, Python script, full Cargo JSON stream or source export is included.
