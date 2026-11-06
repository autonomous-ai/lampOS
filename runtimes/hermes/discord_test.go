package hermes

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestScrubDiscordEnvRemovesOnlyDiscordLines(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, ".env")
	body := "TELEGRAM_BOT_TOKEN=tg\nDISCORD_BOT_TOKEN=fake-discord-secret\nDISCORD_GUILD_ID=g\nDISCORD_ALLOWED_USERS=u\nAUTONOMOUS_API_KEY=k\n"
	if err := os.WriteFile(path, []byte(body), 0600); err != nil {
		t.Fatal(err)
	}

	changed, err := ScrubDiscordEnv(dir)
	if err != nil || !changed {
		t.Fatalf("ScrubDiscordEnv = %v, %v; want true, nil", changed, err)
	}
	got, _ := os.ReadFile(path)
	if strings.Contains(string(got), "DISCORD_") || strings.Contains(string(got), "fake-discord-secret") {
		t.Fatalf("DISCORD_ settings remain: %q", got)
	}
	if !strings.Contains(string(got), "TELEGRAM_BOT_TOKEN=tg") || !strings.Contains(string(got), "AUTONOMOUS_API_KEY=k") {
		t.Fatalf("unrelated settings lost: %q", got)
	}

	changed, err = ScrubDiscordEnv(dir)
	if err != nil || changed {
		t.Fatalf("second pass = %v, %v; want false, nil", changed, err)
	}
}

func TestScrubDiscordEnvMissingFile(t *testing.T) {
	changed, err := ScrubDiscordEnv(t.TempDir())
	if err != nil || changed {
		t.Fatalf("missing file: got %v, %v; want false, nil", changed, err)
	}
}

func TestScrubDiscordMigrationArchive(t *testing.T) {
	home := t.TempDir()
	good := filepath.Join(home, "migration", "openclaw", "20261001T000000Z", "archive", "gateway-config.json")
	other := filepath.Join(home, "migration", "openclaw", "20261001T000000Z", "archive", "notes.json")
	outside := filepath.Join(home, "migration", "openclaw", "20261001T000000Z", "elsewhere.json")
	for _, p := range []string{good, other, outside} {
		if err := os.MkdirAll(filepath.Dir(p), 0755); err != nil {
			t.Fatal(err)
		}
	}
	_ = os.WriteFile(good, []byte(`{"channels":{"discord":{"token":"fake-discord-secret"}},"keep":1}`), 0600)
	_ = os.WriteFile(other, []byte("not json at all"), 0600)
	_ = os.WriteFile(outside, []byte(`{"discord":{"token":"fake-discord-secret"}}`), 0600)

	changed, err := ScrubDiscordMigrationArchive(home)
	if !changed {
		t.Fatalf("changed = false, err = %v", err)
	}
	if err == nil {
		t.Error("the non-JSON file should be reported")
	}
	b, _ := os.ReadFile(good)
	if strings.Contains(string(b), "fake-discord-secret") || !strings.Contains(string(b), "keep") {
		t.Errorf("archive = %s", b)
	}
	if b, _ := os.ReadFile(other); string(b) != "not json at all" {
		t.Error("non-JSON file was modified")
	}
	if b, _ := os.ReadFile(outside); !strings.Contains(string(b), "fake-discord-secret") {
		t.Error("only archive/*.json is in scope")
	}
}

func TestRemoveDiscordState(t *testing.T) {
	home := t.TempDir()
	keep := filepath.Join(home, "gateway", "other_state.json")
	for _, rel := range append(append([]string{}, discordStateFiles...), filepath.Join("gateway", "other_state.json")) {
		p := filepath.Join(home, rel)
		if err := os.MkdirAll(filepath.Dir(p), 0755); err != nil {
			t.Fatal(err)
		}
		_ = os.WriteFile(p, []byte(`{"guild_id":"1"}`), 0600)
	}
	removed, err := RemoveDiscordState(home)
	if !removed || err != nil {
		t.Fatalf("RemoveDiscordState = %v, %v", removed, err)
	}
	for _, rel := range discordStateFiles {
		if _, err := os.Stat(filepath.Join(home, rel)); !os.IsNotExist(err) {
			t.Errorf("%s still exists", rel)
		}
	}
	if _, err := os.Stat(keep); err != nil {
		t.Error("an unrelated state file was removed")
	}
	if removed, err := RemoveDiscordState(home); removed || err != nil {
		t.Fatalf("second run = %v, %v; want false, nil", removed, err)
	}
}

func writeDirectory(t *testing.T, home, body string, mode os.FileMode) string {
	t.Helper()
	path := filepath.Join(home, "channel_directory.json")
	if err := os.WriteFile(path, []byte(body), mode); err != nil {
		t.Fatal(err)
	}
	if err := os.Chmod(path, mode); err != nil {
		t.Fatal(err)
	}
	return path
}

func readPlatforms(t *testing.T, path string) map[string]any {
	t.Helper()
	b, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	var doc map[string]any
	if err := json.Unmarshal(b, &doc); err != nil {
		t.Fatalf("result is not JSON: %v\n%s", err, b)
	}
	platforms, _ := doc["platforms"].(map[string]any)
	return platforms
}

func TestScrubDiscordChannelDirectoryDiscordOnly(t *testing.T) {
	home := t.TempDir()
	path := writeDirectory(t, home, `{"updated_at":"2026-11-01T10:00:00","platforms":{"discord":[{"id":"1","name":"general","guild":"Some Server","type":"channel"}]}}`, 0640)

	changed, err := ScrubDiscordChannelDirectory(home)
	if err != nil || !changed {
		t.Fatalf("ScrubDiscordChannelDirectory = %v, %v; want true, nil", changed, err)
	}
	b, _ := os.ReadFile(path)
	if strings.Contains(string(b), "Some Server") || strings.Contains(string(b), "general") {
		t.Fatalf("Discord entries remain: %s", b)
	}
	if p := readPlatforms(t, path); p == nil || len(p) != 0 {
		t.Fatalf("platforms = %v, want an empty object", p)
	}
	if !strings.Contains(string(b), "updated_at") {
		t.Fatalf("other top-level fields lost: %s", b)
	}
}

func TestScrubDiscordChannelDirectoryKeepsOtherPlatforms(t *testing.T) {
	home := t.TempDir()
	path := writeDirectory(t, home, `{"updated_at":"2026-11-01T10:00:00","platforms":{"discord":[{"id":"1","name":"general","guild":"Some Server"}],"telegram":[{"id":"42","name":"Owner","type":"dm"}],"slack":[{"id":"C1","name":"ops é"}]},"extra":{"n":12345678901234567890}}`, 0600)

	changed, err := ScrubDiscordChannelDirectory(home)
	if err != nil || !changed {
		t.Fatalf("ScrubDiscordChannelDirectory = %v, %v; want true, nil", changed, err)
	}
	b, _ := os.ReadFile(path)
	if strings.Contains(string(b), "Some Server") {
		t.Fatalf("Discord entries remain: %s", b)
	}
	p := readPlatforms(t, path)
	if _, ok := p["discord"]; ok || len(p) != 2 {
		t.Fatalf("platforms = %v, want telegram and slack only", p)
	}
	tg, _ := p["telegram"].([]any)
	if len(tg) != 1 || tg[0].(map[string]any)["id"] != "42" {
		t.Fatalf("telegram entries changed: %v", p["telegram"])
	}
	if !strings.Contains(string(b), "ops é") {
		t.Fatalf("non-ASCII text was rewritten: %s", b)
	}
	if !strings.Contains(string(b), "12345678901234567890") {
		t.Fatalf("an unrelated number lost precision: %s", b)
	}
}

func TestScrubDiscordChannelDirectoryKeepsMode(t *testing.T) {
	home := t.TempDir()
	path := writeDirectory(t, home, `{"platforms":{"discord":[{"id":"1"}],"telegram":[]}}`, 0640)
	if changed, err := ScrubDiscordChannelDirectory(home); err != nil || !changed {
		t.Fatalf("= %v, %v", changed, err)
	}
	info, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}
	if info.Mode().Perm() != 0640 {
		t.Fatalf("mode = %v, want 0640", info.Mode().Perm())
	}
	entries, _ := os.ReadDir(home)
	if len(entries) != 1 {
		t.Fatalf("temporary files left behind: %v", entries)
	}
}

func TestScrubDiscordChannelDirectoryMissingAndMalformed(t *testing.T) {
	if changed, err := ScrubDiscordChannelDirectory(t.TempDir()); changed || err != nil {
		t.Fatalf("missing file: %v, %v", changed, err)
	}
	for name, body := range map[string]string{
		"truncated":    `{"platforms":{"discord":[{"id":"1"}],"telegram":[{"id":"42"`,
		"empty":        ``,
		"array":        `[{"discord":1}]`,
		"null":         `null`,
		"text":         `discord was here`,
		"platformsBad": `{"platforms":"discord"}`,
	} {
		t.Run(name, func(t *testing.T) {
			home := t.TempDir()
			path := writeDirectory(t, home, body, 0600)
			changed, err := ScrubDiscordChannelDirectory(home)
			if changed || err != nil {
				t.Fatalf("= %v, %v; want false, nil", changed, err)
			}
			if got, _ := os.ReadFile(path); string(got) != body {
				t.Fatalf("file was modified: %q", got)
			}
		})
	}
}

func TestScrubDiscordChannelDirectoryIdempotent(t *testing.T) {
	home := t.TempDir()
	path := writeDirectory(t, home, `{"platforms":{"discord":[{"id":"1"}],"telegram":[{"id":"42"}]}}`, 0600)
	if changed, err := ScrubDiscordChannelDirectory(home); err != nil || !changed {
		t.Fatalf("first pass = %v, %v", changed, err)
	}
	first, _ := os.ReadFile(path)
	info1, _ := os.Stat(path)
	if changed, err := ScrubDiscordChannelDirectory(home); err != nil || changed {
		t.Fatalf("second pass = %v, %v; want false, nil", changed, err)
	}
	second, _ := os.ReadFile(path)
	info2, _ := os.Stat(path)
	if string(first) != string(second) || !info1.ModTime().Equal(info2.ModTime()) {
		t.Fatal("a clean file was rewritten")
	}
}

func TestRemoveDiscordStateAlsoCleansChannelDirectoryAndSparesHistory(t *testing.T) {
	home := t.TempDir()
	path := writeDirectory(t, home, `{"platforms":{"discord":[{"id":"1"}],"telegram":[{"id":"42"}]}}`, 0600)
	history := filepath.Join(home, "state.db")
	if err := os.WriteFile(history, []byte("history"), 0600); err != nil {
		t.Fatal(err)
	}
	removed, err := RemoveDiscordState(home)
	if err != nil || !removed {
		t.Fatalf("RemoveDiscordState = %v, %v; want true, nil", removed, err)
	}
	if p := readPlatforms(t, path); len(p) != 1 || p["telegram"] == nil {
		t.Fatalf("platforms = %v", p)
	}
	if b, _ := os.ReadFile(history); string(b) != "history" {
		t.Fatal("chat history was touched")
	}
	if removed, err := RemoveDiscordState(home); err != nil || removed {
		t.Fatalf("second pass = %v, %v; want false, nil", removed, err)
	}
}
