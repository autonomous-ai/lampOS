# Updater exec observer controls, 22 October 2026

For [#2810](https://github.com/autonomous-ai/lampOS/issues/2810), five synthetic
observer controls passed under Linux ARM64 Podman in **6.525 seconds**. This is
preparation for an attended trial, not an updater handover or power-cut result.
[controls.json](controls.json) records identities, checks, results and hashes.

The fixture was an owned shell blocked on a FIFO. GNU GDB 13.1 attached to
that existing PID, armed `catch exec`, and only then the runner released the
shell to exec the verified updater with `--help`. The updater was the separately
verified PR artifact, ELF64 AArch64 SHA-256
`a42dbaade44bc19a65addcb18bd09d1dbdf764c55d571f0e05551c6200dce535`.
It was not installed, given a channel/key, or asked to run an update round.
The container used Debian bookworm, Python 3.11.2, SYS_PTRACE and unconfined
seccomp. Those container privileges are not a proposal to weaken a lamp unit.

Before a fixture READY, the observer required its exec catchpoint, the same
PID and start time, canonical executable path, expected executable hash and
exact argument bytes, and a stopped tracee. Auto-loading and debuginfod were
disabled. Its successful hold was bounded to three seconds, with a separate
60-second overall runner deadline. READY was explicitly fixture-only.

| Control | Result | Seconds |
| --- | --- | ---: |
| Hold, detach normally, run updater help | Pass, debugger 0, target 0 | 3.141 |
| TERM observer while target stopped at READY | Pass, handled interrupt/detach, debugger 2, target 0 | 0.111 |
| KILL observer while target stopped at READY | Pass, debugger signal 9, target 0 | 0.089 |
| No exec by three-second deadline | Pass, interrupt/detach, no READY, old shell resumed, target 0 | 3.105 |
| Exec `/bin/true` instead of updater | Pass, identity rejected, no READY, detached, target 0 | 0.066 |

All owned processes were reaped. No cleanup kill or SIGCONT rescue was needed.
TERM was handled by GDB as an interrupt; it was not an unhandled debugger death.
The no-exec shell exited after release; it did not execute the updater. Wrong
exec remained the same PID/start time but failed path, hash and argument checks.
It does not independently exercise each identity guard or a wrong start time.
The successful cases did not exercise the watchdog's emergency cleanup path.
These controls qualify this tool combination in this container only. They do
not establish flash durability, systemd behavior, actual updater self-exec,
confirmation after boot, or safe use against an as-yet-unassigned lamp.

GNU GDB documents that [attach stops the process and detach resumes it](https://sourceware.org/gdb/current/onlinedocs/gdb.html/Attach.html),
and that [`catch exec` stops at an exec event](https://sourceware.org/gdb/current/onlinedocs/gdb.html/Set-Catchpoints.html).
The tested exit behavior above is observed evidence, not an assumption from
those documents. The recipe attaches; it never uses GDB `run` on the target.
The scratch runner is not shipped product code. Only normalized evidence and
its source/log hashes are published; no Python, raw logs or binary is committed.

## Proposed attended trial, not executed

First must provide the lamp, attending person/window, verified old/new/fixed
fallback binaries, persistent service configuration and recovery access.
The earlier pending-old cut plan remains unexecuted. A proposed stronger cut
is the **real service's exec event into the new updater, before its main runs**:
the actual switch has then persisted pending and selected the new current link.
Unlike the FIFO fixture, this must be the installed updater doing its own switch.

Before anyone unplugs, identify the real service PID/start time and approved
arguments, then verify the observed executable and durable pending/current
state against the approved candidate and old/fallback hashes. A debugger exec
event alone is insufficient. Configuration, channel/key access and apply mode
must persist through boot under the actual unit sandbox; a temporary command
or runtime-only override cannot establish that. No clock, confirmation deadline,
quiet wait, health watch or fallback change is proposed. The person performs
the single unplug; Second has no assigned lamp or device access.

After boot, retain boot identity, service PID/start time and **running executable**
path/hash, not just the current link. Record candidate confirmation, pending
cleanup and the next ordinary round at the same installed sequence. If recovery
selects the old updater instead, record that executable, refusal, cleanup and
next round: recovery alone does not prove candidate execution or confirmation.
Restoration must preserve sequence protection and unexplained failures; never
restore an old whole state file to reset a sequence. Restoring the original
updater, if required, needs an approved uncut signed handover, not another unplug.

Actual device observer setup, persistent preflight, physical interruption and
post-boot observations remain **unexecuted**. The real signed-release exercise
[#1812](https://github.com/autonomous-ai/lampOS/issues/1812) follows that phase;
its owner/key/publication and attended Body inputs are still missing.
