# Task 33 final source: development-host checks

All results here are from the Intel i7-10700K development Mac, x86_64 macOS
26.6.2, Rust 1.96.0. No lamp or hardware adapter was accessed. The exact task
head is `721d7c0722320281ff02d0b7d97cd91c3c23e730`, based on `c14eab1ee`.
These are supplements to the complete 20-package host gate saved on that task
branch in `docs/verification/touch-host-20261014`: 3,213 passed, zero failed,
fifteen inherited ignored, 203 harnesses. Do not sum overlapping runs.

The task branch contains the one Linux result-type annotation after the
failed initial native build. `final-source-sha256.json` matches that final
implementation through the rebase; no other task runtime, test or fixture
bytes changed. The final task head passed:

- Workspace formatting and strict all-target Clippy for all twenty affected
  packages.
- Complete Body and Records tests after their upstream changes: 364 passed,
  zero failed, one existing timing benchmark ignored, seventeen harnesses.
- The changed Voice local-time tests: four passed, zero failed/ignored. The
  command filters other tests; its twenty harnesses are not twenty passing
  suites of Voice behavior.
- The complete workspace build.
- A fresh source-only workspace build at
  `/private/tmp/lamp-third-touch-standalone-final-_purwo61`, followed by all
  405 Controls/Msgs tests in 29 harnesses, zero failed/ignored. Every one of
  the 33 local package manifests (including the patched vendor dependency)
  resolves inside that copy; its target directory is inside the copy. Cargo's
  ordinary registry/git caches were reused. Its task bytes match the manifest.

`rebase-results.json` and `standalone-results.json` contain exact commands,
exit codes, elapsed host wall times and source identities; the text files are
unaltered command output. All tests run serially. The prior failed native
build and earlier development failures remain in the task verification folder
and the log's `touch-native-20261014` folder; no failure was deleted or weakened.

`default-off-final.json` checks the unchanged pre-task Controls binary's hash,
records the final binary's hash and all replay arguments, then compares stdout
and stderr byte for byte without `--touch`, once in shadow and once with legacy
`--enable`. Both match the original comparison: 901 fixture records, 900
observations, zero rejected, one explicitly authored fault, six gestures,
4,601 ticks and 98 frames. One copy of each identical stream is kept here.
This establishes a host packet comparison, not the required lamp minute.

First confirmed baseline native run 38559471390 (`21c24fc0a`) green on the
Queue issue at 12:36 UTC. The task's own native run is 38564855931; its release
build passed and tests were still running when this evidence was pushed.
Readiness remains no until that exact task-head run is green and the task PR
is open. `later-upstream.json` records the subsequent fetch: only deployment
and documentation changed after the task base, not shared runtime source.

Not shown: all-twelve physical electrode calibration, GPIO polarity or a
continuous TTP223 hand, physical touch latency, I2C contention, service-user
lock access, Voice applying Sleep/Wake/Stop with privacy, or body/light response.
The final owner's contact-start lifecycle, durable deduplication and bus trust
checks remain required. No host result is a result on a lamp.
