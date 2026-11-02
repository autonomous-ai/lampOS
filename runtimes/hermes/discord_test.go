package hermes

import (
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
