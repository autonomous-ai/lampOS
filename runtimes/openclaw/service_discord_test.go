package openclaw

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestRemoveDiscordChannelConfigDropsChannelAndDisablesPlugin(t *testing.T) {
	channels := map[string]any{
		"telegram": map[string]any{"enabled": true},
		"discord":  map[string]any{"enabled": true, "token": "fake-discord-secret", "dmPolicy": "allowlist"},
	}
	entries := map[string]any{"discord": map[string]any{"enabled": true}}

	if !removeDiscordChannelConfig(channels, entries) {
		t.Fatal("expected a change")
	}
	if _, ok := channels["discord"]; ok {
		t.Error("channels.discord must be removed")
	}
	if _, ok := channels["telegram"]; !ok {
		t.Error("other channels must be left alone")
	}
	if entries["discord"].(map[string]any)["enabled"] != false {
		t.Errorf("plugin entry enabled = %v, want false", entries["discord"])
	}
	if removeDiscordChannelConfig(channels, entries) {
		t.Error("second pass must be a no-op")
	}
}

func TestRemoveDiscordChannelConfigDoesNotCreateEntries(t *testing.T) {
	channels := map[string]any{}
	entries := map[string]any{}
	if removeDiscordChannelConfig(channels, entries) {
		t.Fatal("nothing to remove, expected no change")
	}
	if len(entries) != 0 || len(channels) != 0 {
		t.Fatalf("maps must stay empty: %v %v", channels, entries)
	}
}

func TestScrubDiscordConfigRewritesOnlyWhenTokenPresent(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "openclaw.json")
	const secret = "fake-discord-secret-for-tests-0005"
	body := `{"channels":{"telegram":{"enabled":true,"botToken":"tg"},"discord":{"enabled":true,"token":"` + secret + `"}},` +
		`"plugins":{"entries":{"discord":{"enabled":true}}}}`
	if err := os.WriteFile(path, []byte(body), 0600); err != nil {
		t.Fatal(err)
	}

	changed, err := ScrubDiscordConfig(dir)
	if err != nil || !changed {
		t.Fatalf("ScrubDiscordConfig = %v, %v; want true, nil", changed, err)
	}
	got, _ := os.ReadFile(path)
	if strings.Contains(string(got), secret) {
		t.Fatalf("credential still on disk: %s", got)
	}
	var cfg map[string]any
	if err := json.Unmarshal(got, &cfg); err != nil {
		t.Fatal(err)
	}
	if cfg["channels"].(map[string]any)["telegram"] == nil {
		t.Error("telegram channel lost")
	}

	changed, err = ScrubDiscordConfig(dir)
	if err != nil || changed {
		t.Fatalf("second ScrubDiscordConfig = %v, %v; want false, nil", changed, err)
	}
}

func TestScrubDiscordConfigMissingFile(t *testing.T) {
	changed, err := ScrubDiscordConfig(t.TempDir())
	if err != nil || changed {
		t.Fatalf("missing file: got %v, %v; want false, nil", changed, err)
	}
}

func TestScrubDiscordBackups(t *testing.T) {
	dir := t.TempDir()
	body := `{"channels":{"discord":{"token":"fake-discord-secret"},"telegram":{"enabled":true}}}`
	for _, n := range []string{"openclaw.json.bak", "openclaw.json.bak.1", "openclaw.json.last-good", "openclaw.json.corrupt"} {
		content := body
		if n == "openclaw.json.corrupt" {
			content = "{"
		}
		if err := os.WriteFile(filepath.Join(dir, n), []byte(content), 0600); err != nil {
			t.Fatal(err)
		}
	}
	live := filepath.Join(dir, "openclaw.json")
	if err := os.WriteFile(live, []byte(body), 0600); err != nil {
		t.Fatal(err)
	}

	changed, err := ScrubDiscordBackups(dir)
	if !changed {
		t.Fatalf("changed = false (err %v)", err)
	}
	if err == nil {
		t.Error("the unreadable backup should be reported")
	}
	for _, n := range []string{"openclaw.json.bak", "openclaw.json.bak.1", "openclaw.json.last-good"} {
		b, _ := os.ReadFile(filepath.Join(dir, n))
		if strings.Contains(string(b), "fake-discord-secret") || !strings.Contains(string(b), "telegram") {
			t.Errorf("%s = %s", n, b)
		}
	}
	if b, _ := os.ReadFile(live); !strings.Contains(string(b), "fake-discord-secret") {
		t.Error("ScrubDiscordBackups must not touch the live file")
	}
}
