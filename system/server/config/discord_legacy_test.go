package config

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

const fakeLegacyDiscordSecret = "fake-discord-secret-for-tests-0001"

// writeLegacyConfig lays down a config.json written by firmware that still kept a
// Discord credential on the device.
func writeLegacyConfig(t *testing.T) string {
	t.Helper()
	dir := t.TempDir()
	t.Chdir(dir)
	raw := `{"httpPort":8080,"channel":"discord","discord_bot_token":"` + fakeLegacyDiscordSecret + `","discord_guild_id":"g-1","discord_user_id":"u-1"}`
	if err := os.MkdirAll(filepath.Join(dir, "config"), 0755); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(dir, "config", "config.json")
	if err := os.WriteFile(path, []byte(raw), 0600); err != nil {
		t.Fatal(err)
	}
	return path
}

// A stored Discord credential must not survive a restart: after the config is loaded
// it is gone from memory and from config.json, and Discord runs through the relay.
func TestProvideConfigDropsStoredDiscordCredential(t *testing.T) {
	path := writeLegacyConfig(t)

	cfg := ProvideConfig()

	mem, err := json.Marshal(cfg)
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(mem), fakeLegacyDiscordSecret) {
		t.Fatal("loaded config still holds the stored Discord credential")
	}
	disk, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(disk), fakeLegacyDiscordSecret) {
		t.Fatal("config.json still holds the stored Discord credential after load")
	}
	var onDisk map[string]any
	if err := json.Unmarshal(disk, &onDisk); err != nil {
		t.Fatal(err)
	}
	if onDisk["discord_managed"] != true {
		t.Fatalf("discord_managed = %v, want true (relay mode)", onDisk["discord_managed"])
	}
	if onDisk["discord_user_id"] != "u-1" || onDisk["discord_guild_id"] != "g-1" {
		t.Fatalf("allowlist ids lost: %v", onDisk)
	}
}

// Earlier firmware wrote an empty credential key on every device; that is not a
// Discord setup and must not turn relay mode on.
func TestProvideConfigIgnoresEmptyLegacyDiscordKey(t *testing.T) {
	dir := t.TempDir()
	t.Chdir(dir)
	if err := os.MkdirAll(filepath.Join(dir, "config"), 0755); err != nil {
		t.Fatal(err)
	}
	raw := `{"httpPort":8080,"channel":"telegram","discord_bot_token":"","discord_guild_id":"","discord_user_id":""}`
	if err := os.WriteFile(filepath.Join(dir, "config", "config.json"), []byte(raw), 0600); err != nil {
		t.Fatal(err)
	}

	cfg := ProvideConfig()

	if cfg.DiscordManaged {
		t.Error("an empty legacy key switched Discord on")
	}
}

func TestLoadAdoptsRelayModeForStoredCredential(t *testing.T) {
	writeLegacyConfig(t)
	cfg, err := Load()
	if err != nil {
		t.Fatal(err)
	}
	if !cfg.DiscordManaged || cfg.DiscordUserID != "u-1" {
		t.Errorf("config = %+v", cfg)
	}
	mem, _ := json.Marshal(cfg)
	if strings.Contains(string(mem), fakeLegacyDiscordSecret) {
		t.Error("credential held in memory")
	}
}
