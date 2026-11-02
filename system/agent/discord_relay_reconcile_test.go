package agent

import (
	"encoding/json"
	"strings"
	"testing"

	"go.autonomous.ai/os/system/domain"
	"go.autonomous.ai/os/system/server/config"
)

// A runtime switch re-applies channels to the new runtime; none of what it is
// handed may carry a Discord credential.
func TestChannelReconcileHandsRuntimeNoDiscordCredential(t *testing.T) {
	t.Chdir(t.TempDir())

	const secret = "fake-discord-secret-for-tests-0002"
	var cfg config.Config
	raw := `{"agent_runtime":"hermes","channels_applied_runtime":"openclaw","channel":"discord",` +
		`"discord_bot_token":"` + secret + `","discord_guild_id":"g-1","discord_user_id":"u-1"}`
	if err := json.Unmarshal([]byte(raw), &cfg); err != nil {
		t.Fatal(err)
	}
	gw := &fakeGateway{supported: []string{domain.ChannelTelegram, domain.ChannelSlack, domain.ChannelDiscord}}

	(&ChannelReconcile{cfg: &cfg, gw: gw}).Reconcile()

	got, err := json.Marshal(gw.addCalls)
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(got), secret) {
		t.Fatalf("runtime was handed the stored Discord credential: %s", got)
	}
}
