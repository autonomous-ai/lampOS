package picoclaw

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestScrubDiscordConfigDisablesChannelAndDropsSettings(t *testing.T) {
	dir := t.TempDir()
	cfgPath := filepath.Join(dir, "config.json")
	secPath := filepath.Join(dir, ".security.yml")
	if err := os.WriteFile(cfgPath, []byte(`{"channel_list":{"discord":{"enabled":true},"telegram":{"enabled":true}}}`), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(secPath, []byte("channel_list:\n  discord:\n    settings: {token: fake-discord-secret, allow_from: [u]}\n  telegram:\n    settings: {token: tg}\n"), 0600); err != nil {
		t.Fatal(err)
	}

	changed, err := ScrubDiscordConfig(dir)
	if err != nil || !changed {
		t.Fatalf("ScrubDiscordConfig = %v, %v; want true, nil", changed, err)
	}
	sec, _ := os.ReadFile(secPath)
	if strings.Contains(string(sec), "fake-discord-secret") || strings.Contains(string(sec), "discord") {
		t.Fatalf("discord settings remain: %s", sec)
	}
	if !strings.Contains(string(sec), "telegram") {
		t.Fatalf("telegram settings lost: %s", sec)
	}
	cfg, _ := os.ReadFile(cfgPath)
	if !strings.Contains(string(cfg), `"enabled": false`) || !strings.Contains(string(cfg), "telegram") {
		t.Fatalf("config.json not updated as expected: %s", cfg)
	}

	changed, err = ScrubDiscordConfig(dir)
	if err != nil || changed {
		t.Fatalf("second pass = %v, %v; want false, nil", changed, err)
	}
}

func TestScrubDiscordConfigMissingFiles(t *testing.T) {
	changed, err := ScrubDiscordConfig(t.TempDir())
	if err != nil || changed {
		t.Fatalf("missing files: got %v, %v; want false, nil", changed, err)
	}
}
