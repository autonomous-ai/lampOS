# Generic release notice package peer read, 23 October 2026 (+07)

Scope: [#3095](https://github.com/autonomous-ai/lampOS/pull/3095), public
`main` PR head `9694ec633`, base `1b5f7a78b`. This read follows #1812's
release preparation, but does not qualify its real signed-release run.
No P1/P2 was found. Two nonblocking P3s are recorded below.

## Independent checks

A scratch export contains 51 files verified against the public commit's
Git blob IDs; all SHA-256 values remain unchanged after verification.
[Source binding](source-binding.json) and [commands/results](affected-gate.json)
are retained. `bash -n scripts/release/upload-lamp-os.sh` passed.

`python3 -m unittest scripts.release.tests.test_lamp_os_package scripts.release.tests.test_upload_lamp_os -q`
passed **41 tests**, no failures, in 10.780 seconds. The
[unaltered affected-test log](affected-tests.txt) is retained. The tests use
synthetic ELF/model files and mocked release tools; they neither build on
ARM64 nor publish. Only the two affected test modules ran, not all release
tests.

The five notice entries from `lamp-voice` `28f796223` parse unchanged:
manifest, provider-unavailable, back-online, voice-service-unreachable and
wifi-lost. Their source URL, byte count and digest pins survive parsing;
`/opt/lampOS/voice/notices` from the generic unit maps to
`runtime/voice/notices`. [Runtime source binding](lamp-source-binding.json).
No asset was downloaded or played for this read.

## P3: cached notice size is not checked

`lamp_os_package.py:124` accepts cached content by digest alone.
[The actual CLI control](cached-size-control.json) keeps a 20-byte cached
manifest and its correct digest but declares 21 bytes. `--fetch-notices`
exits zero without invoking the rejecting curl stand-in. Packaging the
same fixture correctly refuses `notice checksum mismatch: manifest.json`.
Check size as well as digest on cache reuse and cover the mismatch.
The real upload path creates fresh assets and final packaging rechecks
both, so this is a cache/preflight inconsistency, not a bad-package
publication route.

## P3: the package-failure test now fails during fetching

`test_upload_lamp_os.py` checks `PACKAGE_FAIL` before the new mock
`--fetch-notices` branch. [The existing fixture control](upload-control.json)
exits one after Docker build but before Docker run, publishes nothing and
keeps the version. It no longer exercises failure of the final packaging
command after the build. Split the notice-fetch and final-package failure
injections and require the latter to reach Docker run while publishing
nothing. The real script's `set -euo pipefail` still stops either failure.

No source edits, actual release keys, real downloads, Docker/ARM64 builds,
publication, activation, installation or device acceptance occurred. Eighth
owns corrections, and First retains release acceptance and merge.
