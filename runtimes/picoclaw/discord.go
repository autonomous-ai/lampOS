package picoclaw

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"

	"github.com/goccy/go-yaml"
)

// Discord messages reach the device through the cloud relay, so picoclaw never
// runs a Discord channel and neither of its config files carries Discord settings.

// ScrubDiscordConfig disables channel_list.discord in <dir>/config.json and drops
// its settings (credential included) from <dir>/.security.yml. Missing files are
// not an error. Reports whether either file was rewritten.
func ScrubDiscordConfig(dir string) (bool, error) {
	changedCfg, err := scrubPicoConfigJSON(filepath.Join(dir, "config.json"))
	if err != nil {
		return false, err
	}
	changedSec, err := scrubPicoSecurityYAML(filepath.Join(dir, ".security.yml"))
	if err != nil {
		return changedCfg, err
	}
	return changedCfg || changedSec, nil
}

func scrubPicoConfigJSON(path string) (bool, error) {
	raw, err := os.ReadFile(path)
	if err != nil {
		if os.IsNotExist(err) {
			return false, nil
		}
		return false, fmt.Errorf("read picoclaw config: %w", err)
	}
	var cfg map[string]any
	if err := json.Unmarshal(raw, &cfg); err != nil {
		return false, fmt.Errorf("parse picoclaw config: %w", err)
	}
	list, _ := cfg["channel_list"].(map[string]any)
	ch, _ := list["discord"].(map[string]any)
	if ch == nil || ch["enabled"] == false {
		return false, nil
	}
	ch["enabled"] = false
	out, err := json.MarshalIndent(cfg, "", "  ")
	if err != nil {
		return false, fmt.Errorf("marshal picoclaw config: %w", err)
	}
	if err := os.WriteFile(path, out, fileMode(path)); err != nil {
		return false, fmt.Errorf("write picoclaw config: %w", err)
	}
	return true, nil
}

func scrubPicoSecurityYAML(path string) (bool, error) {
	raw, err := os.ReadFile(path)
	if err != nil {
		if os.IsNotExist(err) {
			return false, nil
		}
		return false, fmt.Errorf("read picoclaw security: %w", err)
	}
	var sec map[string]any
	if err := yaml.Unmarshal(raw, &sec); err != nil {
		return false, fmt.Errorf("parse picoclaw security: %w", err)
	}
	list, _ := sec["channel_list"].(map[string]any)
	if _, ok := list["discord"]; !ok {
		return false, nil
	}
	delete(list, "discord")
	out, err := yaml.Marshal(sec)
	if err != nil {
		return false, fmt.Errorf("marshal picoclaw security: %w", err)
	}
	if err := os.WriteFile(path, out, fileMode(path)); err != nil {
		return false, fmt.Errorf("write picoclaw security: %w", err)
	}
	return true, nil
}

func fileMode(path string) os.FileMode {
	if info, err := os.Stat(path); err == nil {
		return info.Mode().Perm()
	}
	return 0600
}
