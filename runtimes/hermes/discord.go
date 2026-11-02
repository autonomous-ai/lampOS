package hermes

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"go.autonomous.ai/os/system/lib/discordscrub"
)

// Discord messages reach the device through the cloud relay, so hermes-gateway
// never runs a Discord adapter and its .env carries no DISCORD_* settings.

// ScrubDiscordEnv removes every DISCORD_* line from <home>/.env. A missing file
// is not an error. Reports whether the file was rewritten (the caller restarts
// the gateway when it was).
func ScrubDiscordEnv(home string) (bool, error) {
	path := filepath.Join(home, ".env")
	raw, err := os.ReadFile(path)
	if err != nil {
		if os.IsNotExist(err) {
			return false, nil
		}
		return false, fmt.Errorf("read hermes env: %w", err)
	}
	lines := strings.Split(string(raw), "\n")
	kept := lines[:0:0]
	changed := false
	for _, l := range lines {
		if strings.HasPrefix(strings.TrimSpace(l), "DISCORD_") {
			changed = true
			continue
		}
		kept = append(kept, l)
	}
	if !changed {
		return false, nil
	}
	info, statErr := os.Stat(path)
	mode := os.FileMode(0600)
	if statErr == nil {
		mode = info.Mode().Perm()
	}
	if err := os.WriteFile(path, []byte(strings.Join(kept, "\n")), mode); err != nil {
		return false, fmt.Errorf("write hermes env: %w", err)
	}
	return true, nil
}

// ScrubDiscordMigrationArchive removes Discord settings from the JSON files hermes
// archives when it imports from openclaw (<home>/migration/openclaw/<stamp>/archive/*.json).
// Files that are not JSON are left alone. Reports whether any file was rewritten.
func ScrubDiscordMigrationArchive(home string) (bool, error) {
	paths, err := filepath.Glob(filepath.Join(home, "migration", "openclaw", "*", "archive", "*.json"))
	if err != nil {
		return false, err
	}
	changedAny := false
	var errs []error
	for _, p := range paths {
		changed, err := discordscrub.ScrubFile(p)
		if err != nil {
			errs = append(errs, err)
			continue
		}
		changedAny = changedAny || changed
	}
	return changedAny, errors.Join(errs...)
}

// discordStateFiles are the files hermes-gateway's Discord adapter writes about the
// servers it talked to (registered slash commands, the capability cache). Nothing
// reads them once Discord runs through the relay.
var discordStateFiles = []string{
	filepath.Join("gateway", "discord_command_sync_state.json"),
	filepath.Join("cache", "discord_capabilities.json"),
}

// RemoveDiscordState deletes the Discord adapter's state files under home. Missing
// files are not an error. Reports whether any file was removed.
func RemoveDiscordState(home string) (bool, error) {
	removed := false
	var errs []error
	for _, rel := range discordStateFiles {
		err := os.Remove(filepath.Join(home, rel))
		switch {
		case err == nil:
			removed = true
		case !os.IsNotExist(err):
			errs = append(errs, err)
		}
	}
	return removed, errors.Join(errs...)
}
