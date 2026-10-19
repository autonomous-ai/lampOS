package bootstrap

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"syscall"
	"time"

	"go.autonomous.ai/os/system/domain"
	"go.autonomous.ai/os/system/lib/core/system"
)

// recoverPendingUpdate runs before version detection: a power loss can leave the
// live directory absent while the stored version would otherwise suppress OTA.
// The updater owns locking, restoration, health checks and journal removal.
// ErrUpdateInProgress blocks every OTA mutation while the installer owns its lock.
var ErrUpdateInProgress = errors.New("OTA installer owns the update lock")

type recoveryInProgress struct{ bootstrapSelfUpdate bool }

func (e *recoveryInProgress) Error() string { return ErrUpdateInProgress.Error() }
func (e *recoveryInProgress) Unwrap() error { return ErrUpdateInProgress }
func bootstrapSelfUpdateInProgress(err error) bool {
	var active *recoveryInProgress
	return errors.As(err, &active) && active.bootstrapSelfUpdate
}

type legacyRecoveryJournal struct {
	Live    string `json:"live"`
	Backup  string `json:"backup"`
	Restore string `json:"restore"`
	HadLive *bool  `json:"had_live"`
	RootFS  string `json:"rootfs"`
}

// The live installer writes this file before replacing a binary. The exemption
// for serving health is limited to this exact, private bootstrap transaction.
func readLegacyRecoveryJournal(path string) (legacyRecoveryJournal, error) {
	var journal legacyRecoveryJournal
	f, err := os.OpenFile(path, os.O_RDONLY|syscall.O_NOFOLLOW|syscall.O_NONBLOCK, 0)
	if err != nil {
		return journal, fmt.Errorf("open recovery journal: %w", err)
	}
	defer f.Close()
	info, err := f.Stat()
	if err != nil {
		return journal, err
	}
	stat, ok := info.Sys().(*syscall.Stat_t)
	if !info.Mode().IsRegular() || info.Size() > 65536 || info.Mode().Perm()&0022 != 0 || !ok || stat.Uid != uint32(os.Geteuid()) {
		return journal, fmt.Errorf("unsafe recovery journal")
	}
	data, err := io.ReadAll(io.LimitReader(f, 65537))
	if err != nil || len(data) > 65536 {
		return journal, fmt.Errorf("cannot read bounded recovery journal")
	}
	if err = json.Unmarshal(data, &journal); err != nil {
		return journal, fmt.Errorf("decode recovery journal: %w", err)
	}
	return journal, nil
}

func legacyRecoveryTarget(j legacyRecoveryJournal, path string) (string, error) {
	directory := filepath.Dir(path)
	var target, live, backup string
	switch j.Restore {
	case "restore_bootstrap_service":
		target, live, backup = "bootstrap", "/usr/local/bin/bootstrap-server", "bootstrap.previous"
	case "restore_web_service_state":
		target, live, backup = "web", "/usr/share/nginx/html/setup", "web.previous"
	case "restore_hal_service_state":
		target, live, backup = "hal", "/opt/hal", "hal.previous"
	case "restore_os_server_service":
		target, live, backup = "os-server", "/usr/local/bin/os-server", "os-server.previous"
	case "restore_device_service_state":
		target, live, backup = "device", j.Live, "device.previous"
		if !filepath.IsAbs(live) || filepath.Clean(live) != live || live == "/" {
			return "", fmt.Errorf("invalid device recovery path")
		}
	default:
		return "", fmt.Errorf("unknown legacy recovery action %q", j.Restore)
	}
	if j.Live != live || j.Backup != filepath.Join(directory, backup) {
		return "", fmt.Errorf("invalid %s recovery paths", target)
	}
	if j.RootFS != "" && (target != "device" || filepath.Dir(j.RootFS) != directory || !strings.HasPrefix(filepath.Base(j.RootFS), ".device.rootfs.new.")) {
		return "", fmt.Errorf("invalid recovery rootfs path")
	}
	return target, nil
}

func updaterLockHeld() (bool, error) {
	// Do not create/remove the stable inode and do not trust software-update.pid.
	f, err := os.OpenFile(updaterLockPath, os.O_RDONLY|syscall.O_NOFOLLOW|syscall.O_NONBLOCK, 0)
	if errors.Is(err, os.ErrNotExist) {
		return false, nil
	}
	if err != nil {
		return false, fmt.Errorf("open updater lock: %w", err)
	}
	defer f.Close()
	info, err := f.Stat()
	if err != nil || !info.Mode().IsRegular() {
		return false, fmt.Errorf("invalid updater lock file")
	}
	err = syscall.Flock(int(f.Fd()), syscall.LOCK_EX|syscall.LOCK_NB)
	if errors.Is(err, syscall.EWOULDBLOCK) || errors.Is(err, syscall.EAGAIN) {
		return true, nil
	}
	if err != nil {
		return false, fmt.Errorf("inspect updater lock: %w", err)
	}
	if err = syscall.Flock(int(f.Fd()), syscall.LOCK_UN); err != nil {
		return false, err
	}
	return false, nil
}

func recoverWithInstallerLock(ctx context.Context, bootstrap bool) error {
	held, err := updaterLockHeld()
	if err != nil {
		return err
	}
	if held {
		return &recoveryInProgress{bootstrapSelfUpdate: bootstrap}
	}
	return runPendingRecovery(ctx)
}

func (b *Bootstrap) recoverPendingUpdate(ctx context.Context) error {
	registry, err := b.serviceRegistry()
	if err != nil {
		return err
	}
	journal := filepath.Join(b.serviceDirectory(), "transition.json")
	if _, err := os.Lstat(journal); err == nil {
		return recoverWithInstallerLock(ctx, false)
	} else if !errors.Is(err, os.ErrNotExist) {
		return fmt.Errorf("inspect service transition: %w", err)
	}
	path := b.pendingUpdatePath
	if path == "" {
		path = "/root/bootstrap/rollback/pending-update.json"
	}
	if _, err := os.Lstat(path); err == nil {
		pending, err := readLegacyRecoveryJournal(path)
		if err != nil {
			return err
		}
		target, err := legacyRecoveryTarget(pending, path)
		if err != nil {
			return err
		}
		if !registry.Allows(target) || (target == "device" && (!registry.Allows(domain.OTAKeyHal) || !registry.Allows(domain.OTAKeyOSServer))) {
			return fmt.Errorf("legacy journal for %s conflicts with service ownership", target)
		}
		return recoverWithInstallerLock(ctx, target == "bootstrap" && (pending.HadLive == nil || *pending.HadLive))
	} else if !errors.Is(err, os.ErrNotExist) {
		return fmt.Errorf("inspect legacy recovery journal: %w", err)
	}
	if !b.halOwned() {
		return nil
	}
	return recoverPendingUpdateAt(ctx, path, "/opt/hal")
}

// Paths are explicit so recovery detection can be tested without a device.
func recoverPendingUpdateAt(ctx context.Context, journalPath, halPath string) error {
	if _, err := os.Stat(journalPath); errors.Is(err, os.ErrNotExist) {
		// Older updaters moved HAL aside without a journal. Do not let the
		// cached installed version hide the surviving rollback directory.
		if _, err := os.Stat(halPath); err == nil {
			return nil
		} else if !errors.Is(err, os.ErrNotExist) {
			return fmt.Errorf("check live HAL for legacy recovery: %w", err)
		}
		backup, err := os.Stat(filepath.Join(filepath.Dir(journalPath), "hal.previous"))
		if errors.Is(err, os.ErrNotExist) {
			return nil
		} else if err != nil {
			return fmt.Errorf("check legacy HAL rollback backup: %w", err)
		} else if !backup.IsDir() {
			return nil
		}
	} else if err != nil {
		return fmt.Errorf("check pending OTA recovery: %w", err)
	}
	return recoverWithInstallerLock(ctx, false)
}
func runPendingRecovery(ctx context.Context) error {
	runCtx, cancel := context.WithTimeout(ctx, 10*time.Minute)
	defer cancel()
	out, err := system.Run(runCtx, "software-update", "recover")
	if err != nil {
		return fmt.Errorf("recover interrupted OTA: %w: %s", err, out)
	}
	return nil
}
