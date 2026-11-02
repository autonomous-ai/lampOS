package openclaw

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"

	"go.autonomous.ai/os/system/lib/discordscrub"
)

// Discord messages reach the device through the cloud relay (os-server receives
// them over MQTT and answers the same way), so openclaw never runs a Discord
// channel of its own and its config never holds a Discord credential.

// removeDiscordChannelConfig deletes channels.discord (credential included) and
// disables the discord plugin entry. Reports whether anything changed.
func removeDiscordChannelConfig(channelsMap, entriesMap map[string]any) bool {
	changed := false
	if _, ok := channelsMap["discord"]; ok {
		delete(channelsMap, "discord")
		changed = true
	}
	if entry, ok := entriesMap["discord"].(map[string]any); ok && entry["enabled"] != false {
		entry["enabled"] = false
		changed = true
	}
	return changed
}

// ScrubDiscordConfig removes any native Discord channel config from
// <configDir>/openclaw.json. A missing file is not an error. Reports whether the
// file was rewritten (the caller restarts the gateway when it was).
func ScrubDiscordConfig(configDir string) (bool, error) {
	path := filepath.Join(configDir, "openclaw.json")
	raw, err := os.ReadFile(path)
	if err != nil {
		if os.IsNotExist(err) {
			return false, nil
		}
		return false, fmt.Errorf("read openclaw config: %w", err)
	}
	var configData map[string]any
	if err := json.Unmarshal(raw, &configData); err != nil {
		return false, fmt.Errorf("parse openclaw config: %w", err)
	}
	channelsMap, _ := configData["channels"].(map[string]any)
	pluginsMap, _ := configData["plugins"].(map[string]any)
	entriesMap, _ := pluginsMap["entries"].(map[string]any)
	changed := removeDiscordChannelConfig(channelsMap, entriesMap)
	// Any other place a Discord credential could sit in the same file.
	if discordscrub.Redact(configData) {
		changed = true
	}
	if !changed {
		return false, nil
	}
	written, err := json.MarshalIndent(configData, "", "  ")
	if err != nil {
		return false, fmt.Errorf("marshal openclaw config: %w", err)
	}
	if existingPrimary := extractPrimaryModel(configData); existingPrimary != "" {
		setOSWriteFlag(configDir, existingPrimary)
	}
	if err := os.WriteFile(path, written, 0600); err != nil {
		return false, fmt.Errorf("write openclaw config: %w", err)
	}
	if err := chownRuntimeUserIfRoot(path, openclawRuntimeUser); err != nil {
		return true, fmt.Errorf("set openclaw config ownership: %w", err)
	}
	return true, nil
}

// ScrubDiscordBackups removes Discord settings from the copies openclaw keeps beside
// its live config (openclaw.json.bak, openclaw.json.bak.1, openclaw.json.last-good, …).
// Files that are not JSON are left alone. Reports whether any file was rewritten; a
// bad file does not stop the others, and the errors are joined.
func ScrubDiscordBackups(configDir string) (bool, error) {
	paths, err := filepath.Glob(filepath.Join(configDir, "openclaw.json.*"))
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
