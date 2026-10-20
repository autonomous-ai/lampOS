package bootstrap

import (
	"context"
	"fmt"
	"log/slog"
	"os/exec"
	"strings"
	"syscall"
	"time"

	"go.autonomous.ai/os/system/lib/core/system"
)

// installTimeout bounds one `software-update <key>` install; installs that start
// from a much older version can legitimately take close to 30 minutes.
const installTimeout = 30 * time.Minute

// recoverTimeout bounds `software-update recover`, which only restores saved files.
const recoverTimeout = 10 * time.Minute

// updaterStopGrace is how long a timed-out updater gets between SIGTERM and SIGKILL:
// its EXIT trap restores the previous installation, and HAL's health check alone
// may wait two minutes. Variable so tests can shorten it.
var updaterStopGrace = 3 * time.Minute

// updaterErrorOutput bounds how much updater output an error carries; the reason
// a run failed is printed last.
const updaterErrorOutput = 2000

// updaterOutputTail returns the end of the updater's output for an error message.
func updaterOutputTail(out []byte) string {
	s := strings.TrimSpace(string(out))
	if len(s) > updaterErrorOutput {
		s = "…" + strings.ToValidUTF8(s[len(s)-updaterErrorOutput:], "")
	}
	return s
}

// runUpdater runs `software-update args...` until it exits or timeout passes.
//
// On timeout the updater is stopped with SIGTERM, so its EXIT trap restores the
// previous installation, then SIGKILL after updaterStopGrace, so no leftover process
// keeps the update lock. Bootstrap shutting down does not abort an install; only the
// timeout does.
func runUpdater(ctx context.Context, timeout time.Duration, args ...string) ([]byte, error) {
	runCtx, cancel := context.WithTimeout(context.WithoutCancel(ctx), timeout)
	defer cancel()
	stop := system.GroupStop{Grace: updaterStopGrace}
	if len(args) > 0 {
		stop.Signal = signalUpdaterUnit(args[0])
	}
	return system.RunGroup(runCtx, stop, "software-update", args...)
}

// updaterUnit names the transient unit software-update re-executes itself in:
// `software-update-<first arg>-<pid>`, where pid is the process bootstrap started.
func updaterUnit(arg string, pid int) string {
	return fmt.Sprintf("software-update-%s-%d.service", arg, pid)
}

// signalUpdaterUnit signals every process of the updater's transient unit. It reports
// false when there is no such unit (the updater runs inline), so the caller signals
// the updater's process group instead.
func signalUpdaterUnit(arg string) func(pid int, sig syscall.Signal) bool {
	return func(pid int, sig syscall.Signal) bool {
		name := "SIGTERM"
		if sig == syscall.SIGKILL {
			name = "SIGKILL"
		}
		unit := updaterUnit(arg, pid)
		ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
		defer cancel()
		// --kill-whom defaults to all processes in the unit, so long-running children
		// end too and the updater's trap can run.
		out, err := exec.CommandContext(ctx, "systemctl", "kill", "--signal="+name, unit).CombinedOutput()
		if err != nil {
			slog.Debug("updater unit not signalled", "component", "bootstrap", "unit", unit, "signal", name,
				"error", err, "output", strings.TrimSpace(string(out)))
			return false
		}
		slog.Warn("updater timed out; signalled its unit", "component", "bootstrap", "unit", unit, "signal", name)
		return true
	}
}
