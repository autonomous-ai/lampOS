# Named CONTROL EOF negative control, 27 October 2026

Ready for lamp-voice: no

The new IPC camera-authority test
`named_control_eof_after_both_write_halves_close_delivers_live_observers`
runs exactly once on macOS x86_64 against unchanged old production from
`218c1d5f2`. Only its test is added, giving tree `0d8d6c683` at HEAD
`ed9aff39b`. The [receipt](receipt.json) binds six small retained inputs by
SHA-256 without publishing raw output or private paths.

The actual result is **exit 101, zero passes and one failure**. Eighteen
filtered tests are not executions. The first `ReceiveGrant::try_receive`
at `camera_authority.rs:747` rejects the named CONTROL connection with
`EINVAL` (22) after both write halves close and the server disappears.
The regression distinguishes valid queued bytes and SCM_RIGHTS at CONTROL
EOF from the OBSERVER channel's shutdown revocation. Connected OBSERVER
validation and revocation remain strict.

The helper's actual native exit 0 means it observed and preserved this
expected old-production failure; it does not turn the failed test into a
pass. Its exact failed executable is retained as an independent mode 0600
file. Actual dsymutil and an independent Mach-O N_OSO reader both observe
zero debug backing inputs. All 7,886 prior protected files are byte-verified,
the 10,726-file source freeze is stable and owned sessions are closed before
target release.

The repair on separate tree `e927f8518` needs its own fresh qualification.
This negative control claims no fixed-code pass, no repair of every prior
86 failures, and no ARM64, Linux, lamp or physical-latency acceptance.
