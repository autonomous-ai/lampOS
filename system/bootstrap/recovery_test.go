package bootstrap

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"syscall"
	"testing"
	"time"

	"go.autonomous.ai/os/system/bootstrap/config"
	"go.autonomous.ai/os/system/bootstrap/state"
	"go.autonomous.ai/os/system/domain"
)

func TestPendingRecoveryFailureStopsEveryUpdateEntry(t *testing.T) {
	entries := map[string]func(*Bootstrap) error{
		"poll":      func(b *Bootstrap) error { return b.checkOnce(context.Background()) },
		"component": func(b *Bootstrap) error { return b.checkComponent(context.Background(), domain.OTAKeyHal) },
		"force":     func(b *Bootstrap) error { return b.forceUpdate(context.Background(), domain.OTAKeyHal) },
	}
	for name, run := range entries {
		t.Run(name, func(t *testing.T) {
			dir, calls := setupUpdateAudit(t)
			t.Setenv("UPDATE_TEST_FAIL", "recover")
			pending := filepath.Join(dir, "pending-update.json")
			if err := os.WriteFile(pending, legacyJournalBytes(t, "restore_hal_service_state", "/opt/hal", filepath.Join(dir, "hal.previous")), 0600); err != nil {
				t.Fatal(err)
			}
			b := &Bootstrap{serviceStateDir: t.TempDir(), cfg: &config.Config{MetadataURL: "http://127.0.0.1:1/should-not-fetch"}, state: &state.State{Components: map[string]string{domain.OTAKeyHal: "9.0.0"}}, pendingUpdatePath: pending}
			if err := run(b); err == nil {
				t.Fatal("failed recovery must stop update entry")
			}
			data, err := os.ReadFile(calls)
			if err != nil || string(data) != "recover\n" {
				t.Fatalf("calls = %q, %v", data, err)
			}
			if b.state.Components[domain.OTAKeyHal] != "9.0.0" {
				t.Fatal("failed recovery changed recorded version")
			}
		})
	}
}

func TestPendingRecoveryRunsBeforeMetadataEvenWithCurrentStoredVersion(t *testing.T) {
	dir, calls := setupUpdateAudit(t)
	t.Setenv("UPDATE_TEST_FAIL", "")
	pending := filepath.Join(dir, "pending-update.json")
	if err := os.WriteFile(pending, legacyJournalBytes(t, "restore_hal_service_state", "/opt/hal", filepath.Join(dir, "hal.previous")), 0600); err != nil {
		t.Fatal(err)
	}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		data, err := os.ReadFile(calls)
		if err != nil || string(data) != "recover\n" {
			t.Errorf("metadata fetched before recovery: %q, %v", data, err)
		}
		_, _ = w.Write([]byte(`{}`))
	}))
	defer server.Close()
	b := &Bootstrap{serviceStateDir: t.TempDir(), cfg: &config.Config{MetadataURL: server.URL}, client: server.Client(), state: &state.State{Components: map[string]string{domain.OTAKeyHal: "9.0.0"}}, pendingUpdatePath: pending}
	if err := b.checkOnce(context.Background()); err != nil {
		t.Fatal(err)
	}
}

func TestNoPendingRecoveryDoesNotInvokeUpdater(t *testing.T) {
	dir, calls := setupUpdateAudit(t)
	b := &Bootstrap{serviceStateDir: t.TempDir(), pendingUpdatePath: filepath.Join(dir, "absent.json")}
	if err := b.recoverPendingUpdate(context.Background()); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(calls); !os.IsNotExist(err) {
		t.Fatalf("unexpected updater execution: %v", err)
	}
}

func TestLegacyHALRecoveryWithoutJournal(t *testing.T) {
	for _, liveExists := range []bool{false, true} {
		name := "missing-live"
		if liveExists {
			name = "installed-live"
		}
		t.Run(name, func(t *testing.T) {
			dir, calls := setupUpdateAudit(t)
			t.Setenv("UPDATE_TEST_FAIL", "")
			halPath := filepath.Join(dir, "hal")
			if liveExists {
				if err := os.Mkdir(halPath, 0755); err != nil {
					t.Fatal(err)
				}
			}
			if err := os.Mkdir(filepath.Join(dir, "hal.previous"), 0755); err != nil {
				t.Fatal(err)
			}
			if err := recoverPendingUpdateAt(context.Background(), filepath.Join(dir, "pending-update.json"), halPath); err != nil {
				t.Fatal(err)
			}
			data, err := os.ReadFile(calls)
			if liveExists {
				if !os.IsNotExist(err) {
					t.Fatalf("healthy installation must not trigger recovery: %q, %v", data, err)
				}
			} else if err != nil || string(data) != "recover\n" {
				t.Fatalf("legacy recovery calls = %q, %v", data, err)
			}
		})
	}
}

func TestLegacyHALRecoveryRequiresDirectoryBackup(t *testing.T) {
	dir, calls := setupUpdateAudit(t)
	if err := os.WriteFile(filepath.Join(dir, "hal.previous"), []byte("invalid backup"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := recoverPendingUpdateAt(context.Background(), filepath.Join(dir, "pending-update.json"), filepath.Join(dir, "hal")); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(calls); !os.IsNotExist(err) {
		t.Fatalf("invalid backup must not trigger recovery: %v", err)
	}
}

func TestServeRecoversBeforeWaitingForProvisioning(t *testing.T) {
	b, calls := serviceBootstrap(t)
	t.Setenv("UPDATE_TEST_FAIL", "recover")
	if err := os.WriteFile(filepath.Join(b.serviceDirectory(), "transition.json"), []byte(`{}`), 0600); err != nil {
		t.Fatal(err)
	}
	// No metadata URL: waiting for provisioning first would hang indefinitely.
	err := b.Serve()
	if err == nil || !strings.Contains(err.Error(), "startup OTA recovery") {
		t.Fatalf("startup error: %v", err)
	}
	data, err := os.ReadFile(calls)
	if err != nil || string(data) != "recover\n" {
		t.Fatalf("calls %q: %v", data, err)
	}
}

func TestServeRecoversBeforeParsingPollInterval(t *testing.T) {
	b, calls := serviceBootstrap(t)
	t.Setenv("UPDATE_TEST_FAIL", "")
	b.cfg.MetadataURL = "http://127.0.0.1:1/unused"
	b.cfg.PollInterval = "invalid"
	if err := os.WriteFile(filepath.Join(b.serviceDirectory(), "transition.json"), []byte(`{}`), 0600); err != nil {
		t.Fatal(err)
	}
	err := b.Serve()
	if err == nil || !strings.Contains(err.Error(), "parse poll interval") {
		t.Fatalf("startup error: %v", err)
	}
	data, err := os.ReadFile(calls)
	if err != nil || string(data) != "recover\n" {
		t.Fatalf("calls %q: %v", data, err)
	}
}

func legacyJournalBytes(t *testing.T, restore, live, backup string) []byte {
	t.Helper()
	data, err := json.Marshal(map[string]any{"restore": restore, "live": live, "backup": backup, "had_live": true, "rootfs": ""})
	if err != nil {
		t.Fatal(err)
	}
	return data
}
func writeBootstrapRecovery(t *testing.T, b *Bootstrap) {
	t.Helper()
	if err := os.WriteFile(b.pendingUpdatePath, legacyJournalBytes(t, "restore_bootstrap_service", "/usr/local/bin/bootstrap-server", filepath.Join(filepath.Dir(b.pendingUpdatePath), "bootstrap.previous")), 0600); err != nil {
		t.Fatal(err)
	}
}
func holdRecoveryLock(t *testing.T) func() {
	t.Helper()
	previous := updaterLockPath
	updaterLockPath = filepath.Join(t.TempDir(), "software-update.lock")
	file, err := os.OpenFile(updaterLockPath, os.O_CREATE|os.O_RDWR, 0600)
	if err != nil {
		t.Fatal(err)
	}
	if err := syscall.Flock(int(file.Fd()), syscall.LOCK_EX|syscall.LOCK_NB); err != nil {
		t.Fatal(err)
	}
	var once sync.Once
	release := func() { once.Do(func() { _ = syscall.Flock(int(file.Fd()), syscall.LOCK_UN); _ = file.Close() }) }
	t.Cleanup(func() { release(); updaterLockPath = previous })
	return release
}
func TestLegacyRecoveryUsesActualTargetAfterV2Migration(t *testing.T) {
	for _, tc := range []struct {
		name, restore, live, backup string
		allowed                     bool
	}{
		{"bootstrap", "restore_bootstrap_service", "/usr/local/bin/bootstrap-server", "bootstrap.previous", true},
		{"web", "restore_web_service_state", "/usr/share/nginx/html/setup", "web.previous", true},
		{"hal", "restore_hal_service_state", "/opt/hal", "hal.previous", false},
		{"os-server", "restore_os_server_service", "/usr/local/bin/os-server", "os-server.previous", false},
		{"device", "restore_device_service_state", "/opt/devices/lamp", "device.previous", false},
		{"invalid bootstrap path", "restore_bootstrap_service", "/usr/local/bin/os-server", "bootstrap.previous", false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			b, calls := serviceBootstrap(t)
			putServiceRegistry(t, b, sampleServiceRegistry())
			if err := os.WriteFile(b.pendingUpdatePath, legacyJournalBytes(t, tc.restore, tc.live, filepath.Join(filepath.Dir(b.pendingUpdatePath), tc.backup)), 0600); err != nil {
				t.Fatal(err)
			}
			err := b.recoverPendingUpdate(context.Background())
			if (err == nil) != tc.allowed {
				t.Fatalf("recovery allowed=%v err=%v", tc.allowed, err)
			}
			data, readErr := os.ReadFile(calls)
			if tc.allowed {
				if readErr != nil || string(data) != "recover\n" {
					t.Fatalf("calls %q %v", data, readErr)
				}
			} else if !os.IsNotExist(readErr) {
				t.Fatalf("blocked target invoked updater: %q", data)
			}
		})
	}
}
func TestHeldInstallerLockOnlyAllowsBootstrapSelfUpdateHealth(t *testing.T) {
	for _, tc := range []struct {
		name, restore, live, backup string
		health                      bool
	}{
		{"bootstrap", "restore_bootstrap_service", "/usr/local/bin/bootstrap-server", "bootstrap.previous", true},
		{"web", "restore_web_service_state", "/usr/share/nginx/html/setup", "web.previous", false},
		{"hal", "restore_hal_service_state", "/opt/hal", "hal.previous", false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			b, calls := serviceBootstrap(t)
			release := holdRecoveryLock(t)
			if err := os.WriteFile(b.pendingUpdatePath, legacyJournalBytes(t, tc.restore, tc.live, filepath.Join(filepath.Dir(b.pendingUpdatePath), tc.backup)), 0600); err != nil {
				t.Fatal(err)
			}
			err := b.recoverPendingUpdate(context.Background())
			if !errors.Is(err, ErrUpdateInProgress) || bootstrapSelfUpdateInProgress(err) != tc.health {
				t.Fatalf("unexpected recovery result: %v", err)
			}
			if _, err := os.Stat(calls); !os.IsNotExist(err) {
				t.Fatal("recover invoked while lock held")
			}
			release()
			if err := b.recoverPendingUpdate(context.Background()); err != nil {
				t.Fatal(err)
			}
			data, err := os.ReadFile(calls)
			if err != nil || string(data) != "recover\n" {
				t.Fatalf("abandoned transaction not recovered: %q %v", data, err)
			}
		})
	}
}
func TestBootstrapSelfUpdateServesHealthButBlocksOTAWhileLocked(t *testing.T) {
	for _, v2 := range []bool{false, true} {
		t.Run(fmt.Sprintf("v2=%v", v2), func(t *testing.T) {
			b, calls := serviceBootstrap(t)
			if v2 {
				putServiceRegistry(t, b, sampleServiceRegistry())
			}
			release := holdRecoveryLock(t)
			writeBootstrapRecovery(t, b)
			listener, err := net.Listen("tcp", "127.0.0.1:0")
			if err != nil {
				t.Fatal(err)
			}
			b.cfg.HttpPort = listener.Addr().(*net.TCPAddr).Port
			listener.Close()
			b.cfg.MetadataURL = "http://127.0.0.1:1/must-not-fetch"
			b.cfg.PollInterval = "1h"
			b.client = &http.Client{Timeout: time.Second}
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			done := make(chan error, 1)
			go func() { done <- b.serve(ctx) }()
			client := &http.Client{Timeout: 200 * time.Millisecond}
			ticker := time.NewTicker(20 * time.Millisecond)
			defer ticker.Stop()
			timeout := time.NewTimer(3 * time.Second)
			defer timeout.Stop()
			healthy := false
			for !healthy {
				select {
				case err := <-done:
					t.Fatalf("bootstrap failed during installer health check: %v", err)
				case <-timeout.C:
					t.Fatal("health did not start while installer held lock")
				case <-ticker.C:
					response, err := client.Get(fmt.Sprintf("http://127.0.0.1:%d/health", b.cfg.HttpPort))
					if err == nil {
						healthy = response.StatusCode == http.StatusOK
						response.Body.Close()
					}
				}
			}
			for name, action := range map[string]func() error{
				"poll":      func() error { return b.checkOnce(ctx) },
				"component": func() error { return b.checkComponent(ctx, "bootstrap") },
				"force":     func() error { return b.forceUpdate(ctx, "bootstrap") },
			} {
				if err := action(); !errors.Is(err, ErrUpdateInProgress) {
					t.Errorf("%s mutation not blocked: %v", name, err)
				}
			}
			if _, err := os.Stat(calls); !os.IsNotExist(err) {
				t.Fatal("new bootstrap competed with live installer")
			}
			cancel()
			select {
			case err := <-done:
				if err != nil {
					t.Fatal(err)
				}
			case <-time.After(3 * time.Second):
				t.Fatal("bootstrap did not shut down")
			}
			release()
			if err := b.recoverPendingUpdate(context.Background()); err != nil {
				t.Fatal(err)
			}
			data, err := os.ReadFile(calls)
			if err != nil || string(data) != "recover\n" {
				t.Fatalf("released lock did not restore normal recovery: %q %v", data, err)
			}
		})
	}
}
func TestMalformedOrSymlinkJournalCannotBypassStartupRecovery(t *testing.T) {
	for _, symlink := range []bool{false, true} {
		b, _ := serviceBootstrap(t)
		_ = holdRecoveryLock(t)
		if symlink {
			other := filepath.Join(filepath.Dir(b.pendingUpdatePath), "other.json")
			if err := os.WriteFile(other, legacyJournalBytes(t, "restore_bootstrap_service", "/usr/local/bin/bootstrap-server", filepath.Join(filepath.Dir(other), "bootstrap.previous")), 0600); err != nil {
				t.Fatal(err)
			}
			if err := os.Symlink(other, b.pendingUpdatePath); err != nil {
				t.Fatal(err)
			}
		} else if err := os.WriteFile(b.pendingUpdatePath, []byte(`{"restore":"restore_bootstrap_service"}`), 0600); err != nil {
			t.Fatal(err)
		}
		err := b.recoverPendingUpdate(context.Background())
		if err == nil || bootstrapSelfUpdateInProgress(err) {
			t.Fatalf("unsafe journal bypassed recovery: %v", err)
		}
	}
}
