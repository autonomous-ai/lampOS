package bootstrap

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"
	"time"
)

func TestInstallTimeoutIsThirtyMinutes(t *testing.T) {
	if installTimeout != 30*time.Minute {
		t.Fatalf("installTimeout = %s, want 30m", installTimeout)
	}
	if updaterStopGrace <= 0 || installTimeout+updaterStopGrace >= 40*time.Minute {
		t.Fatalf("install timeout plus stop grace = %s, must stay under the 40m completion watcher", installTimeout+updaterStopGrace)
	}
}

func TestUpdaterUnitName(t *testing.T) {
	if got := updaterUnit("hal", 4242); got != "software-update-hal-4242.service" {
		t.Fatalf("updaterUnit = %q", got)
	}
}

// Inline updater (no transient unit): the unit signal fails, so the process group
// gets SIGTERM and the updater's EXIT trap runs its restore.
func TestRunUpdaterTimeoutStopsInlineUpdater(t *testing.T) {
	dir := t.TempDir()
	bin := filepath.Join(dir, "bin")
	if err := os.Mkdir(bin, 0o755); err != nil {
		t.Fatal(err)
	}
	t.Setenv("PATH", bin+string(os.PathListSeparator)+os.Getenv("PATH"))
	t.Setenv("UPDATER_TEST_DIR", dir)
	stubs := map[string]string{
		"software-update": `#!/bin/bash
echo $$ > "$UPDATER_TEST_DIR/updater.pid"
trap 'touch "$UPDATER_TEST_DIR/restored"' EXIT
trap 'exit 143' TERM
sleep 60
`,
		"systemctl": `#!/bin/sh
printf '%s\n' "$*" >> "$UPDATER_TEST_DIR/systemctl.calls"
exit 1
`,
	}
	for name, body := range stubs {
		if err := os.WriteFile(filepath.Join(bin, name), []byte(body), 0o755); err != nil {
			t.Fatal(err)
		}
	}
	orig := updaterStopGrace
	updaterStopGrace = 20 * time.Second
	t.Cleanup(func() { updaterStopGrace = orig })

	start := time.Now()
	_, err := runUpdater(context.Background(), 2*time.Second, "hal")
	if !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("err = %v, want deadline exceeded", err)
	}
	if elapsed := time.Since(start); elapsed > 10*time.Second {
		t.Fatalf("returned after %s; SIGTERM should end the updater before the grace", elapsed)
	}
	if _, statErr := os.Stat(filepath.Join(dir, "restored")); statErr != nil {
		t.Fatalf("updater EXIT trap did not run: %v", statErr)
	}
	pidData, err := os.ReadFile(filepath.Join(dir, "updater.pid"))
	if err != nil {
		t.Fatal(err)
	}
	pid, _ := strconv.Atoi(strings.TrimSpace(string(pidData)))
	calls, err := os.ReadFile(filepath.Join(dir, "systemctl.calls"))
	if err != nil {
		t.Fatal(err)
	}
	want := "kill --signal=SIGTERM " + updaterUnit("hal", pid)
	if got := strings.TrimSpace(string(calls)); got != want {
		t.Fatalf("systemctl calls = %q, want %q", got, want)
	}
}

// Bootstrap shutting down must not abort an install in progress.
func TestRunUpdaterIgnoresParentCancel(t *testing.T) {
	dir := t.TempDir()
	bin := filepath.Join(dir, "bin")
	if err := os.Mkdir(bin, 0o755); err != nil {
		t.Fatal(err)
	}
	t.Setenv("PATH", bin+string(os.PathListSeparator)+os.Getenv("PATH"))
	if err := os.WriteFile(filepath.Join(bin, "software-update"), []byte("#!/bin/sh\nsleep 0.3\necho installed\n"), 0o755); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	out, err := runUpdater(ctx, 10*time.Second, "hal")
	if err != nil {
		t.Fatalf("runUpdater: %v", err)
	}
	if !strings.Contains(string(out), "installed") {
		t.Fatalf("output = %q", out)
	}
}

func TestUpdaterOutputTailKeepsTheEnd(t *testing.T) {
	if got := updaterOutputTail([]byte("  short\n")); got != "short" {
		t.Fatalf("short output = %q", got)
	}
	long := strings.Repeat("x", 3*updaterErrorOutput) + "HAL staging failed: uv sync exited 2"
	got := updaterOutputTail([]byte(long))
	if !strings.HasSuffix(got, "HAL staging failed: uv sync exited 2") || !strings.HasPrefix(got, "…") {
		t.Fatalf("tail lost the reason: %q", got[len(got)-60:])
	}
	if len(got) > updaterErrorOutput+len("…") {
		t.Fatalf("tail is %d bytes, want <= %d", len(got), updaterErrorOutput+len("…"))
	}
}
