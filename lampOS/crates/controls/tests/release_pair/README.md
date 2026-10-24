# Published Controls calibration files

`release_pair.rs` exercises the actual `calibrate-touch --replay` writer and
`replay --touch` reader in both directions between releases 6408 and 6498.
The ordinary test uses the current native CLI to test the fixture itself;
it does not qualify a released pair. The two ignored tests require native
Linux ARM64 and explicitly supplied, hash-pinned released executables.

The ordinary test first runs one separate hanging-child control: a 60 s `/bin/sleep` helper remains alive when the original 25 ms deadline expires. Dropping the
existing guard must kill and reap it, with its group absent and `waitpid`
independently reporting no waitable child. A separate bounded fallback
cleans up if that assertion fails; it cannot alter the primary receipt or
make the test pass. This helper adds no child to either actual-release case,
whose 18-command receipts and two-case/one-filtered selection stay unchanged.
It tests one direct child, not descendant adoption, kernel hard deadlines
or a live output-file quota. Its streams are discarded.

Each case owns a private directory and executes 18 bounded commands. One
release calibrates three synthetic electrodes into a new pins file. Both
readers replay it; the opposite release recalibrates that file, both readers
reopen it, and the original release writes and reopens the returned file.
The full stored JSON and written bytes must agree. Each replay must contain
47 frames and exactly two expected slide events, ending ready and inactive.

Controls must refuse overwrite, malformed JSON, an invalid timing profile,
and unknown privacy/reset/API-token fields without changing any existing
input. A benign unknown field must remain readable. A synthetic private
value must not appear in stderr. Recovery reads follow the refusals.
These authored unknown fields are refusal controls, not released-writer
output or a claim of arbitrary future-schema compatibility.

Every command has an original five-second deadline within a sixty-second
case budget, a private process group, a reaped child and an absent group.
Failure cleanup kills only that owned group and retains its outcome.
Output streams are files rather than pipes; bounded reads reject large,
nonregular or linked fixture files. Existing data bytes, mode, inode,
device and modification times must survive every command. Executable
copies are read-only and rehashed before each invocation and at the end.

Only calibration replay and Controls replay are invoked. They do not open
GPIO or I2C, serve a bus, start a provider, or run an actuator. `--enable`
enables replay output only. The 20 ms authored replay clock is not measured
touch acquisition, feedback or readiness latency. Hardware calibration,
sleep/wake touch, deployment and update/rollback acceptance remain separate.

The dedicated workflow verifies exactly two successful terminal receipts,
all 36 child reaps, unchanged tracked source, and regular evidence files.
It retains failed commands and cleanup/source outcomes as well as passes.
See [provenance and execution](../../../../docs/verification/controls-release-pair-20261024/README.md).
