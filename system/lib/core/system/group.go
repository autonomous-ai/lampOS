package system

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"os/exec"
	"syscall"
	"time"
)

// DefaultStopGrace is the SIGTERM-to-SIGKILL delay RunGroup uses when GroupStop.Grace is zero.
const DefaultStopGrace = 2 * time.Minute

// defaultPipeDelay bounds the wait for output pipes that surviving descendants keep open.
const defaultPipeDelay = 5 * time.Second

// GroupStop controls how RunGroup stops a command whose context ends.
type GroupStop struct {
	// Signal, when set, runs before the process group is signalled, with the leader pid
	// and SIGTERM (then SIGKILL once Grace expires). It reaches work the command handed
	// off outside its own process group, such as a transient systemd unit. Returning true
	// for SIGTERM means the hook delivered it, so the group itself is left alone until
	// SIGKILL and the leader can keep waiting for the handed-off work to finish.
	Signal func(pid int, sig syscall.Signal) bool
	// Grace is the time between SIGTERM and SIGKILL. Zero means DefaultStopGrace.
	Grace time.Duration
	// PipeDelay bounds the wait for output pipes after the leader exits. Zero means 5s.
	PipeDelay time.Duration
}

// RunGroup runs name with args in its own process group and returns combined output.
//
// Unlike Run, ending ctx does not SIGKILL only the leader: the whole group gets SIGTERM,
// so shell EXIT traps can restore state and children exit too; after Grace the group
// gets SIGKILL, so no descendant survives holding inherited locks or pipes. The group is
// also killed once the leader exits after a stop, to remove stragglers that ignored
// SIGTERM. The returned error wraps ctx.Err() when the command was stopped.
func RunGroup(ctx context.Context, stop GroupStop, name string, args ...string) ([]byte, error) {
	grace := stop.Grace
	if grace <= 0 {
		grace = DefaultStopGrace
	}
	pipeDelay := stop.PipeDelay
	if pipeDelay <= 0 {
		pipeDelay = defaultPipeDelay
	}

	var out bytes.Buffer
	cmd := exec.Command(name, args...)
	cmd.SysProcAttr = &syscall.SysProcAttr{Setpgid: true}
	// Same writer for both streams: exec shares one pipe and one copy goroutine.
	cmd.Stdout = &out
	cmd.Stderr = &out
	cmd.WaitDelay = pipeDelay
	if err := ctx.Err(); err != nil {
		return nil, fmt.Errorf("%s %v: %w", name, args, err)
	}
	if err := cmd.Start(); err != nil {
		return nil, fmt.Errorf("%s %v: %w", name, args, err)
	}
	pid := cmd.Process.Pid
	done := make(chan error, 1)
	go func() { done <- cmd.Wait() }()

	select {
	case err := <-done:
		// The command itself succeeded; only a descendant kept the output pipe open.
		if errors.Is(err, exec.ErrWaitDelay) {
			err = nil
		}
		if err != nil {
			return out.Bytes(), fmt.Errorf("%s %v: %w", name, args, err)
		}
		return out.Bytes(), nil
	case <-ctx.Done():
	}

	if stop.Signal == nil || !stop.Signal(pid, syscall.SIGTERM) {
		signalGroup(pid, syscall.SIGTERM)
	}
	timer := time.NewTimer(grace)
	defer timer.Stop()
	var waitErr error
	select {
	case waitErr = <-done:
	case <-timer.C:
		if stop.Signal != nil {
			stop.Signal(pid, syscall.SIGKILL)
		}
		signalGroup(pid, syscall.SIGKILL)
		waitErr = <-done
	}
	// Remaining group members ignored SIGTERM; they would keep inherited locks.
	signalGroup(pid, syscall.SIGKILL)
	if waitErr == nil {
		waitErr = errors.New("exited 0 after stop")
	}
	return out.Bytes(), fmt.Errorf("%s %v: stopped: %w (%v)", name, args, ctx.Err(), waitErr)
}

// signalGroup signals every process in the group led by pid; a gone group is not an error.
func signalGroup(pid int, sig syscall.Signal) {
	_ = syscall.Kill(-pid, sig)
}
