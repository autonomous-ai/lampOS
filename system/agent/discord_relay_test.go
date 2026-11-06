package agent

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"go.autonomous.ai/os/system/domain"
	"go.autonomous.ai/os/system/server/config"
)

type restartGateway struct {
	domain.AgentGateway
	restarts int
}

func (g *restartGateway) RestartAgent() error { g.restarts++; return nil }

// legacyRuntimeHomes lays down what earlier firmware left in each runtime's files.
func legacyRuntimeHomes(t *testing.T) (openclawDir, hermesDir, picoDir string) {
	t.Helper()
	const secret = "fake-discord-secret-for-tests-0006"
	root := t.TempDir()
	openclawDir, hermesDir, picoDir = filepath.Join(root, "oc"), filepath.Join(root, "hermes"), filepath.Join(root, "pico")
	for _, d := range []string{openclawDir, hermesDir, picoDir} {
		if err := os.MkdirAll(d, 0755); err != nil {
			t.Fatal(err)
		}
	}
	write := func(path, body string) {
		if err := os.WriteFile(path, []byte(body), 0600); err != nil {
			t.Fatal(err)
		}
	}
	write(filepath.Join(openclawDir, "openclaw.json"), `{"channels":{"discord":{"enabled":true,"token":"`+secret+`"}},"plugins":{"entries":{"discord":{"enabled":true}}}}`)
	write(filepath.Join(hermesDir, ".env"), "DISCORD_BOT_TOKEN="+secret+"\nTELEGRAM_BOT_TOKEN=tg\n")
	write(filepath.Join(picoDir, "config.json"), `{"channel_list":{"discord":{"enabled":true}}}`)
	write(filepath.Join(picoDir, ".security.yml"), "channel_list:\n  discord:\n    settings: {token: "+secret+"}\n")
	return
}

func allFilesUnder(t *testing.T, dirs ...string) string {
	t.Helper()
	var sb strings.Builder
	for _, d := range dirs {
		entries, _ := os.ReadDir(d)
		for _, e := range entries {
			b, _ := os.ReadFile(filepath.Join(d, e.Name()))
			sb.Write(b)
		}
	}
	return sb.String()
}

func TestDiscordRelayReconcileWipesEveryRuntimeAndRestartsTheActiveOne(t *testing.T) {
	oc, hm, pc := legacyRuntimeHomes(t)
	gw := &restartGateway{}
	r := &DiscordRelayReconcile{
		cfg: &config.Config{AgentRuntime: domain.AgentRuntimeHermes},
		gw:  gw, openclawDir: oc, hermesHome: hm, picoHome: pc,
	}

	r.Reconcile()

	if left := allFilesUnder(t, oc, hm, pc); strings.Contains(left, "fake-discord-secret") {
		t.Fatalf("a runtime file still holds the credential:\n%s", left)
	}
	if !strings.Contains(allFilesUnder(t, hm), "TELEGRAM_BOT_TOKEN=tg") {
		t.Error("unrelated runtime settings were lost")
	}
	if gw.restarts != 1 {
		t.Errorf("restarts = %d, want 1 (the active runtime's file changed)", gw.restarts)
	}

	r.Reconcile()
	if gw.restarts != 1 {
		t.Errorf("second pass restarted again (%d); a clean device must be left alone", gw.restarts)
	}
}

func TestDiscordRelayReconcileLeavesActiveRuntimeRunningWhenOnlyAnotherWasDirty(t *testing.T) {
	oc, hm, pc := legacyRuntimeHomes(t)
	if err := os.Remove(filepath.Join(hm, ".env")); err != nil {
		t.Fatal(err)
	}
	gw := &restartGateway{}
	r := &DiscordRelayReconcile{
		cfg: &config.Config{AgentRuntime: domain.AgentRuntimeHermes},
		gw:  gw, openclawDir: oc, hermesHome: hm, picoHome: pc,
	}

	r.Reconcile()

	if gw.restarts != 0 {
		t.Errorf("restarts = %d, want 0: the active runtime's own files were already clean", gw.restarts)
	}
	if strings.Contains(allFilesUnder(t, oc, pc), "fake-discord-secret") {
		t.Error("an inactive runtime kept the credential")
	}
}

func TestDiscordRelayReconcileCleansHermesChannelDirectoryWhateverRuntimeIsActive(t *testing.T) {
	for _, active := range []string{domain.AgentRuntimeHermes, domain.AgentRuntimeOpenClaw, domain.AgentRuntimePicoclaw} {
		t.Run(active, func(t *testing.T) {
			oc, hm, pc := legacyRuntimeHomes(t)
			directory := filepath.Join(hm, "channel_directory.json")
			if err := os.WriteFile(directory, []byte(`{"platforms":{"discord":[{"id":"1","name":"general","guild":"Old Server"}],"telegram":[{"id":"42","name":"Owner"}]}}`), 0600); err != nil {
				t.Fatal(err)
			}
			gw := &restartGateway{}
			r := &DiscordRelayReconcile{
				cfg: &config.Config{AgentRuntime: active},
				gw:  gw, openclawDir: oc, hermesHome: hm, picoHome: pc,
			}

			r.Reconcile()

			got, _ := os.ReadFile(directory)
			if strings.Contains(string(got), "Old Server") || !strings.Contains(string(got), `"telegram"`) {
				t.Fatalf("channel directory = %s", got)
			}
			before := string(got)
			r.Reconcile()
			if after, _ := os.ReadFile(directory); string(after) != before {
				t.Error("second pass rewrote a clean channel directory")
			}
		})
	}
}

func TestChannelReconcileAppliesDiscordWithoutCredential(t *testing.T) {
	t.Chdir(t.TempDir())
	gw := &fakeGateway{supported: []string{domain.ChannelDiscord}}
	cfg := &config.Config{
		AgentRuntime: "hermes", ChannelsAppliedRuntime: "openclaw",
		DiscordManaged: true, DiscordGuildID: "g-1", DiscordUserID: "u-1",
	}
	(&ChannelReconcile{cfg: cfg, gw: gw}).Reconcile()

	if len(gw.addCalls) != 1 {
		t.Fatalf("addCalls = %d, want 1", len(gw.addCalls))
	}
	got := gw.addCalls[0]
	if got.Channel != domain.ChannelDiscord || !got.DiscordManaged || got.DiscordUserID != "u-1" || got.DiscordGuildID != "g-1" {
		t.Errorf("request = %+v", got)
	}
}
