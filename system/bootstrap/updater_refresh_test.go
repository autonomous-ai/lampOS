package bootstrap

import (
	"context"
	"errors"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"go.autonomous.ai/os/system/bootstrap/config"
	"go.autonomous.ai/os/system/bootstrap/state"
	"go.autonomous.ai/os/system/domain"
)

func TestUpdaterURLFrom(t *testing.T) {
	cases := []struct {
		name    string
		in      string
		want    string
		wantErr bool
	}{
		{
			name: "production metadata url",
			in:   "https://cdn.autonomous.ai/os/ota/metadata.json",
			want: "https://cdn.autonomous.ai/os/software-update",
		},
		{
			name: "bucket-style url keeps its namespace",
			in:   "https://storage.googleapis.com/s3-autonomous-upgrade-3/os/ota/metadata.json",
			want: "https://storage.googleapis.com/s3-autonomous-upgrade-3/os/software-update",
		},
		{
			name: "query is dropped",
			in:   "https://cdn.autonomous.ai/os/ota/metadata.json?v=3",
			want: "https://cdn.autonomous.ai/os/software-update",
		},
		{
			name:    "relative url is refused",
			in:      "/os/ota/metadata.json",
			wantErr: true,
		},
		{
			name:    "no namespace to derive from",
			in:      "https://cdn.autonomous.ai/metadata.json",
			wantErr: true,
		},
		{
			name:    "empty url is refused",
			in:      "",
			wantErr: true,
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got, err := updaterURLFrom(tc.in)
			if tc.wantErr {
				if err == nil {
					t.Fatalf("expected an error, got %q", got)
				}
				return
			}
			if err != nil {
				t.Fatalf("unexpected error: %v", err)
			}
			if got != tc.want {
				t.Errorf("got %q, want %q", got, tc.want)
			}
		})
	}
}

// withTempUpdaterPath swaps updaterPath for a temp destination.
func withTempUpdaterPath(t *testing.T, contents string) string {
	t.Helper()
	dir := t.TempDir()
	dest := filepath.Join(dir, "software-update")
	if err := os.WriteFile(dest, []byte(contents), 0o755); err != nil {
		t.Fatalf("seed updater: %v", err)
	}
	orig, originalLock := updaterPath, updaterLockPath
	updaterPath = dest
	updaterLockPath = filepath.Join(t.TempDir(), "software-update.lock")
	t.Cleanup(func() { updaterPath, updaterLockPath = orig, originalLock })
	return dest
}

func TestInstallUpdaterRejectsBrokenSyntax(t *testing.T) {
	good := "#!/usr/bin/env bash\necho ok\n"
	dest := withTempUpdaterPath(t, good)

	// A truncated download (`if` without `fi`) must never reach disk.
	truncated := []byte("#!/usr/bin/env bash\nif [ -f /tmp/x ]; then\n  echo half\n")
	if err := installUpdater(context.Background(), truncated); err == nil {
		t.Fatal("expected bash -n to reject a truncated script")
	}

	after, err := os.ReadFile(dest)
	if err != nil {
		t.Fatalf("read updater: %v", err)
	}
	if string(after) != good {
		t.Errorf("existing updater was replaced by a broken download:\n%s", after)
	}

	entries, err := os.ReadDir(filepath.Dir(dest))
	if err != nil {
		t.Fatalf("read dir: %v", err)
	}
	if len(entries) != 1 {
		names := make([]string, 0, len(entries))
		for _, e := range entries {
			names = append(names, e.Name())
		}
		t.Errorf("expected only the updater to remain, got %v", names)
	}
}

func TestInstallUpdaterReplacesAndKeepsItExecutable(t *testing.T) {
	dest := withTempUpdaterPath(t, "#!/usr/bin/env bash\necho old\n")

	want := "#!/usr/bin/env bash\necho new\n"
	if err := installUpdater(context.Background(), []byte(want)); err != nil {
		t.Fatalf("install: %v", err)
	}

	got, err := os.ReadFile(dest)
	if err != nil {
		t.Fatalf("read updater: %v", err)
	}
	if string(got) != want {
		t.Errorf("got %q, want %q", got, want)
	}

	info, err := os.Stat(dest)
	if err != nil {
		t.Fatalf("stat updater: %v", err)
	}
	if perm := info.Mode().Perm(); perm != 0o755 {
		t.Errorf("mode = %o, want 755", perm)
	}
}

func TestUpdateInFlightBlocksRefresh(t *testing.T) {
	if updateInFlight() {
		t.Fatal("nothing should be in flight at the start")
	}
	inFlight.Store("hal", struct{}{})
	defer inFlight.Delete("hal")
	if !updateInFlight() {
		t.Error("a running force update must report as in flight")
	}
}

func TestRefreshUpdaterSharesCLIInstallationLock(t *testing.T) {
	b, _ := serviceBootstrap(t)
	original := "#!/bin/sh\necho old\n"
	dest := withTempUpdaterPath(t, original)
	published := "#!/bin/sh\necho new\n"
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_, _ = w.Write([]byte(published))
	}))
	defer server.Close()
	b.cfg.MetadataURL = server.URL + "/os/ota/metadata.json"
	b.client = server.Client()
	unlock, err := lockUpdaterRefresh()
	if err != nil {
		t.Fatal(err)
	}
	b.refreshUpdater(context.Background())
	data, readErr := os.ReadFile(dest)
	unlock()
	if readErr != nil || string(data) != original {
		t.Fatalf("updater changed while CLI held lock: %q %v", data, readErr)
	}
	b.refreshUpdater(context.Background())
	data, err = os.ReadFile(dest)
	if err != nil || string(data) != published {
		t.Fatalf("updater did not refresh after CLI released lock: %q %v", data, err)
	}
}

func TestRefreshUpdaterRechecksActivationAfterDownload(t *testing.T) {
	b, _ := serviceBootstrap(t)
	original := "#!/bin/sh\n# Service OTA protocol: 1\necho current\n"
	dest := withTempUpdaterPath(t, original)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		// Simulate activation committing while the network request is pending.
		// Acquiring the lock here also proves fetch does not hold it.
		unlock, err := lockUpdaterRefresh()
		if err != nil {
			t.Errorf("network fetch held installation lock: %v", err)
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		if err := os.WriteFile(filepath.Join(b.serviceDirectory(), "ownership.json"), []byte(`{}`), 0o600); err != nil {
			t.Error(err)
		}
		unlock()
		_, _ = w.Write([]byte("#!/bin/sh\necho incompatible\n"))
	}))
	defer server.Close()
	b.cfg.MetadataURL = server.URL + "/os/ota/metadata.json"
	b.client = server.Client()
	b.refreshUpdater(context.Background())
	data, err := os.ReadFile(dest)
	if err != nil || string(data) != original {
		t.Fatalf("activation compatibility state was stale: %q %v", data, err)
	}
}

func TestRefreshUpdaterPreservesMigrationCapabilitiesBeforeActivation(t *testing.T) {
	original := "#!/bin/sh\n# Service OTA protocol: 1\n# Service OTA activation protocol: 1\n# Service OTA preflight protocol: 1\n# Service OTA cleanup protocol: 1\necho current\n"
	for _, protocol := range []string{"preflight", "cleanup"} {
		t.Run(protocol, func(t *testing.T) {
			b, _ := serviceBootstrap(t)
			dest := withTempUpdaterPath(t, original)
			published := strings.ReplaceAll(original, "# Service OTA "+protocol+" protocol: 1\n", "")
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				_, _ = w.Write([]byte(published))
			}))
			defer server.Close()
			b.cfg.MetadataURL = server.URL + "/os/ota/metadata.json"
			b.client = server.Client()
			b.refreshUpdater(context.Background())
			data, err := os.ReadFile(dest)
			if err != nil || string(data) != original {
				t.Fatalf("stale feed removed %s before migration: %v", protocol, err)
			}
		})
	}
}

// recoveryRefreshFixture fails `software-update recover` with the on-device
// updater and serves `published` as the feed copy; empty serves the device's own.
func recoveryRefreshFixture(t *testing.T, published string) (*Bootstrap, string, *int) {
	t.Helper()
	dir, calls := setupUpdateAudit(t)
	t.Setenv("UPDATE_TEST_FAIL", "recover")
	orig, originalLock := updaterPath, updaterLockPath
	updaterPath = filepath.Join(dir, "bin", "software-update")
	updaterLockPath = filepath.Join(t.TempDir(), "software-update.lock")
	t.Cleanup(func() { updaterPath, updaterLockPath = orig, originalLock })
	pending := filepath.Join(dir, "pending-update.json")
	if err := os.WriteFile(pending, legacyJournalBytes(t, "restore_hal_service_state", "/opt/hal", filepath.Join(dir, "hal.previous")), 0600); err != nil {
		t.Fatal(err)
	}
	if published == "" {
		current, err := os.ReadFile(updaterPath)
		if err != nil {
			t.Fatal(err)
		}
		published = string(current)
	}
	fetches := new(int)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if strings.HasSuffix(r.URL.Path, "/software-update") {
			*fetches++
			_, _ = w.Write([]byte(published))
			return
		}
		_, _ = w.Write([]byte(`{}`))
	}))
	t.Cleanup(server.Close)
	b := &Bootstrap{serviceStateDir: t.TempDir(), cfg: &config.Config{MetadataURL: server.URL + "/os/ota/metadata.json"}, client: server.Client(), state: &state.State{Components: map[string]string{domain.OTAKeyHal: "9.0.0"}}, pendingUpdatePath: pending}
	return b, calls, fetches
}

func TestFailedRecoveryRetriesWithRefreshedUpdater(t *testing.T) {
	fixed := "#!/bin/sh\nprintf 'fixed-%s\\n' \"$1\" >> \"$UPDATE_TEST_CALLS\"\n"
	b, calls, _ := recoveryRefreshFixture(t, fixed)
	if err := b.checkOnce(context.Background()); err != nil {
		t.Fatalf("recovery with the refreshed updater should let the cycle continue: %v", err)
	}
	data, err := os.ReadFile(calls)
	if err != nil || string(data) != "recover\nfixed-recover\n" {
		t.Fatalf("calls = %q, %v", data, err)
	}
	if installed, err := os.ReadFile(updaterPath); err != nil || string(installed) != fixed {
		t.Fatalf("feed updater not installed: %q %v", installed, err)
	}
}

func TestFailedRecoveryIsNotRetriedWithUnchangedUpdater(t *testing.T) {
	b, calls, fetches := recoveryRefreshFixture(t, "")
	if err := b.checkOnce(context.Background()); err == nil {
		t.Fatal("failed recovery must still stop the cycle")
	}
	if *fetches != 1 {
		t.Fatalf("updater fetched %d times, want 1", *fetches)
	}
	data, err := os.ReadFile(calls)
	if err != nil || string(data) != "recover\n" {
		t.Fatalf("calls = %q, %v", data, err)
	}
}

func TestRecoveryHeldByInstallerDoesNotRefreshUpdater(t *testing.T) {
	b, calls, fetches := recoveryRefreshFixture(t, "#!/bin/sh\necho new\n")
	release := holdRecoveryLock(t)
	defer release()
	if err := b.recoverWithFreshUpdater(context.Background()); !errors.Is(err, ErrUpdateInProgress) {
		t.Fatalf("expected the installer lock to win: %v", err)
	}
	if *fetches != 0 {
		t.Fatalf("updater fetched %d times while the installer held the lock", *fetches)
	}
	if _, err := os.Stat(calls); !os.IsNotExist(err) {
		t.Fatal("recover invoked while lock held")
	}
}
