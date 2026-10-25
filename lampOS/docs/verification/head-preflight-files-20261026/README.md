# Nonregular head-pair inputs refuse before mutation

The shared head-pair reader previously called blocking read_to_string on
local units, drop-ins, known Body calibration and pins. An installed FIFO
could block the install or update before its preflight could refuse.
The reader now opens with NONBLOCK and NOCTTY, checks that same descriptor,
and reads only regular files. Symlinks to regular files remain accepted.
Actual null-device masks remain empty after character-device identity
validation, without reading a device. Other nonregular inputs refuse;
missing files and dangling links retain existing absence semantics.

On unchanged production at 3d9722f38, the original install regression
failed **0/1**, exit101, after the ready witness and a successful FIFO-writer
open proved the installer reached its reader. The five-second watchdog
killed and reaped the child before scratch removal. The original executable,
raw log and source hashes were retained; negative.json records their
identities and negative-tests.json holds the exact overlay losslessly.
This is a reached product failure, not a fixture setup timeout.

The final source on the same base passes formatting, strict Update
all-target Clippy and workspace build. Serial native x86_64 Mac tests pass:
complete install **101/0**, update-round head pairing **34/0**, apply library
**23/0**, and null-device identity **1/0**. All nine new install cases pass
once; all 7,132 source hashes stay fixed. fixed.json records exact commands,
times, input identities and raw-log hashes. The original 179,429-byte
install test prefix is unchanged; coverage is appended.

The bounded child cases cover the original FIFO reader, a FIFO with no
writer at all, Body calibration FIFO, directory refusal, regular files,
regular symlinks, missing files, dangling links and null masks. Refusal
preserves every captured file/link/mode and leaves all host commands empty.
The metadata-only /dev/zero control prevents broad character-device
acceptance without risking an unbounded read. The final fixture consumes
a ready witness even when a fast child exits between polling operations.

The shared reader's actual consumers are install preflight, settings apply
projection/verification and update planning/pre-restart validation. Existing
round tests execute real apply::run and cover settings-off, custom pins,
installed/carried units and rollback. Other crates use unrelated Update
APIs; workspace build checks those consumers without claiming their full
runtime suites ran. No runtime switch, schema or microphone behavior changes.

The watchdog is a test diagnostic, not a product timeout. The fix refuses
without waiting for a FIFO writer; regular-file storage can still be slow.
No global filesystem deadline, pathname-race protection, hardware touch
latency, device access or local ARM64 link is claimed. The PR's Linux ARM64
and Clippy gates and current-head reads remain separate requirements.
