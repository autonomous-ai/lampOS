package system

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"syscall"
	"testing"
	"time"
)

func writeScript(t *testing.T, body string) string {
	t.Helper()
	path := filepath.Join(t.TempDir(), "script.sh")
	if err := os.WriteFile(path, []byte("#!/bin/bash\n"+body), 0o755); err != nil {
		t.Fatal(err)
	}
	return path
}

func readPid(t *testing.T, path string) int {
	t.Helper()
	deadline := time.Now().Add(5 * time.Second)
	for {
		data, err := os.ReadFile(path)
		if err == nil {
			if pid, err := strconv.Atoi(strings.TrimSpace(string(data))); err == nil && pid > 0 {
				return pid
			}
		}
		if time.Now().After(deadline) {
			t.Fatalf("pid file %s not written", path)
		}
		time.Sleep(10 * time.Millisecond)
	}
}

// stopAt approximates when the stop began: the moment the script signalled ready.
func stopAt(t *testing.T, ready string) time.Time {
	t.Helper()
	info, err := os.Stat(ready)
	if err != nil {
		t.Fatal(err)
	}
	return info.ModTime()
}

// waitGone waits for pid to disappear; an orphan may be a zombie briefly until init reaps it.
func waitGone(t *testing.T, pid int) {
	t.Helper()
	deadline := time.Now().Add(5 * time.Second)
	for syscall.Kill(pid, 0) == nil {
		if time.Now().After(deadline) {
			t.Fatalf("process %d still alive", pid)
		}
		time.Sleep(20 * time.Millisecond)
	}
}

// cancelWhenReady returns a context cancelled once the script creates ready, so the
// stop never lands before the script has installed its traps (a new executable can
// start slowly on a test host). Scripts create ready from inside their long-running
// foreground child: bash defers a trap until that child exits, so a SIGTERM sent just
// before the child is forked would otherwise miss it and wait out the grace.
func cancelWhenReady(t *testing.T, ready string) context.Context {
	t.Helper()
	ctx, cancel := context.WithCancel(context.Background())
	t.Cleanup(cancel)
	go func() {
		deadline := time.Now().Add(30 * time.Second)
		for time.Now().Before(deadline) {
			if _, err := os.Stat(ready); err == nil {
				break
			}
			time.Sleep(10 * time.Millisecond)
		}
		cancel()
	}()
	return ctx
}

func TestRunGroupSuccessReturnsCombinedOutput(t *testing.T) {
	script := writeScript(t, "echo out; echo err >&2\n")
	out, err := RunGroup(context.Background(), GroupStop{}, script)
	if err != nil {
		t.Fatalf("RunGroup: %v", err)
	}
	if got := string(out); !strings.Contains(got, "out") || !strings.Contains(got, "err") {
		t.Fatalf("output = %q, want stdout and stderr", got)
	}
}

func TestRunGroupFailureKeepsOutput(t *testing.T) {
	script := writeScript(t, "echo broken; exit 3\n")
	out, err := RunGroup(context.Background(), GroupStop{}, script)
	if err == nil || errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("err = %v, want plain exit failure", err)
	}
	if !strings.Contains(string(out), "broken") {
		t.Fatalf("output = %q", out)
	}
}

func TestRunGroupTimeoutRunsExitTrap(t *testing.T) {
	dir := t.TempDir()
	marker := filepath.Join(dir, "restored")
	ready := filepath.Join(dir, "ready")
	// Same shape as the installer: EXIT trap restores, TERM exits, a long foreground child.
	script := writeScript(t, `trap 'echo restoring; touch "$1"' EXIT
trap 'exit 143' TERM
echo installing
sh -c 'touch "$1"; exec sleep 30' _ "$2"
`)
	ctx := cancelWhenReady(t, ready)
	start := time.Now()
	out, err := RunGroup(ctx, GroupStop{Grace: 20 * time.Second}, script, marker, ready)
	if !errors.Is(err, context.Canceled) {
		t.Fatalf("err = %v, want the context error", err)
	}
	if elapsed := time.Since(start); elapsed > 10*time.Second {
		t.Fatalf("returned after %s; SIGTERM should end it well before the grace", elapsed)
	}
	if _, statErr := os.Stat(marker); statErr != nil {
		t.Fatalf("EXIT trap did not run: %v", statErr)
	}
	if got := string(out); !strings.Contains(got, "installing") || !strings.Contains(got, "restoring") {
		t.Fatalf("output = %q, want output from before and after the stop", got)
	}
}

func TestRunGroupKillsGroupAfterGrace(t *testing.T) {
	dir := t.TempDir()
	childPid := filepath.Join(dir, "child.pid")
	ready := filepath.Join(dir, "ready")
	// Ignores SIGTERM (inherited by the child) and leaves a child holding stdout.
	script := writeScript(t, `trap '' TERM
sleep 60 &
echo $! > "$1"
touch "$2"
sleep 60
`)
	ctx := cancelWhenReady(t, ready)
	start := time.Now()
	_, err := RunGroup(ctx, GroupStop{Grace: 300 * time.Millisecond, PipeDelay: 200 * time.Millisecond}, script, childPid, ready)
	if !errors.Is(err, context.Canceled) {
		t.Fatalf("err = %v, want the context error", err)
	}
	if elapsed := time.Since(start); elapsed > 10*time.Second {
		t.Fatalf("returned after %s; must not wait on pipes held by descendants", elapsed)
	}
	waitGone(t, readPid(t, childPid))
}

func TestRunGroupKillsStragglersAfterLeaderExits(t *testing.T) {
	dir := t.TempDir()
	childPid := filepath.Join(dir, "child.pid")
	ready := filepath.Join(dir, "ready")
	// The leader exits on SIGTERM, but a child that ignores it keeps running.
	script := writeScript(t, `(trap '' TERM; exec sleep 60) &
echo $! > "$1"
trap 'exit 143' TERM
sh -c 'touch "$1"; exec sleep 60' _ "$2"
`)
	ctx := cancelWhenReady(t, ready)
	start := time.Now()
	_, err := RunGroup(ctx, GroupStop{Grace: 30 * time.Second, PipeDelay: 200 * time.Millisecond}, script, childPid, ready)
	if !errors.Is(err, context.Canceled) {
		t.Fatalf("err = %v, want the context error", err)
	}
	if elapsed := time.Since(start); elapsed > 10*time.Second {
		t.Fatalf("returned after %s; should not wait for the grace once the leader exited", elapsed)
	}
	waitGone(t, readPid(t, childPid))
}

func TestRunGroupSignalHookHandlesTerm(t *testing.T) {
	dir := t.TempDir()
	leaderPid := filepath.Join(dir, "leader.pid")
	ready := filepath.Join(dir, "ready")
	script := writeScript(t, `echo $$ > "$1"
trap 'exit 143' TERM
touch "$2"
sleep 60
`)
	var mu sync.Mutex
	var calls []string
	hook := func(pid int, sig syscall.Signal) bool {
		mu.Lock()
		defer mu.Unlock()
		calls = append(calls, strconv.Itoa(pid)+":"+strconv.Itoa(int(sig)))
		return true
	}
	ctx := cancelWhenReady(t, ready)
	_, err := RunGroup(ctx, GroupStop{Grace: 500 * time.Millisecond, Signal: hook}, script, leaderPid, ready)
	if !errors.Is(err, context.Canceled) {
		t.Fatalf("err = %v, want the context error", err)
	}
	// A handled SIGTERM leaves the group alone, so only SIGKILL after the grace ends it.
	if elapsed := time.Since(stopAt(t, ready)); elapsed < 450*time.Millisecond {
		t.Fatalf("returned after %s; group was signalled although the hook handled SIGTERM", elapsed)
	}
	pid := readPid(t, leaderPid)
	want := []string{
		strconv.Itoa(pid) + ":" + strconv.Itoa(int(syscall.SIGTERM)),
		strconv.Itoa(pid) + ":" + strconv.Itoa(int(syscall.SIGKILL)),
	}
	mu.Lock()
	defer mu.Unlock()
	if strings.Join(calls, ",") != strings.Join(want, ",") {
		t.Fatalf("hook calls = %v, want %v", calls, want)
	}
}

func TestRunGroupDeadlineWrapsDeadlineExceeded(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 200*time.Millisecond)
	defer cancel()
	if _, err := RunGroup(ctx, GroupStop{Grace: time.Second}, "sleep", "30"); !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("err = %v, want deadline exceeded", err)
	}
}

func TestRunGroupCancelledBeforeStart(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := RunGroup(ctx, GroupStop{}, "true"); !errors.Is(err, context.Canceled) {
		t.Fatalf("err = %v, want canceled", err)
	}
}
