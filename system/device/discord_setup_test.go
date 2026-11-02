package device

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"go.autonomous.ai/os/system/domain"
	"go.autonomous.ai/os/system/server/config"
)

// Setting up Discord stores the owner's allowlist ids only; whatever else the
// request carried never reaches config.json or the runtime request built from it.
func TestSetupDiscordKeepsNoCredentialOnDevice(t *testing.T) {
	const secret = "fake-discord-secret-for-tests-0004"
	var data domain.SetupRequest
	if err := json.Unmarshal([]byte(`{"channel":"discord","discord_bot_token":"`+secret+`","discord_guild_id":"g-1","discord_user_id":"u-1"}`), &data); err != nil {
		t.Fatal(err)
	}
	s := &Service{config: &config.Config{}}

	s.applySetupChannel(data)

	stored, err := json.Marshal(s.config)
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(stored), secret) {
		t.Fatalf("config holds the Discord credential after setup: %s", stored)
	}
	forwarded, err := json.Marshal(s.wifiProvisionSetupRequest())
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(forwarded), secret) {
		t.Fatalf("runtime setup request carries the Discord credential: %s", forwarded)
	}
}

func TestSetupDiscordSwitchesOnRelayAndStoresOwnerIDs(t *testing.T) {
	// A client that sends discord_managed=1 (or nothing about it) gets the same result.
	for _, body := range []string{
		`{"channel":"discord","discord_managed":true,"discord_guild_id":"g-1","discord_user_id":"u-1"}`,
		`{"channel":"discord","discord_guild_id":"g-1","discord_user_id":"u-1"}`,
	} {
		var data domain.SetupRequest
		if err := json.Unmarshal([]byte(body), &data); err != nil {
			t.Fatal(err)
		}
		s := &Service{config: &config.Config{}}
		s.applySetupChannel(data)
		if !s.config.DiscordManaged || s.config.DiscordGuildID != "g-1" || s.config.DiscordUserID != "u-1" || s.config.Channel != domain.ChannelDiscord {
			t.Errorf("body %s: config = %+v", body, s.config)
		}
		fwd := s.wifiProvisionSetupRequest()
		if !fwd.DiscordManaged || fwd.DiscordUserID != "u-1" {
			t.Errorf("body %s: forwarded setup request = %+v", body, fwd)
		}
	}
}

func TestAddChannelDiscordPersistsRelayModeWithoutCredential(t *testing.T) {
	t.Chdir(t.TempDir()) // WithLockSave writes config/config.json relative to cwd
	const secret = "fake-discord-secret-for-tests-0007"
	var req domain.AddChannelRequest
	if err := json.Unmarshal([]byte(`{"channel":"discord","discord_bot_token":"`+secret+`","discord_guild_id":"g-1","discord_user_id":"u-1"}`), &req); err != nil {
		t.Fatal(err)
	}
	gw := &fakeGateway{supported: []string{domain.ChannelDiscord}}
	s := newService(gw, &config.Config{})

	if _, err := s.AddChannel(context.Background(), req); err != nil {
		t.Fatalf("AddChannel: %v", err)
	}

	if !s.config.DiscordManaged || s.config.DiscordUserID != "u-1" {
		t.Errorf("config = %+v", s.config)
	}
	stored, _ := json.Marshal(s.config)
	onDisk, _ := os.ReadFile(filepath.Join("config", "config.json"))
	handed, _ := json.Marshal(gw.addCalls)
	for name, blob := range map[string][]byte{"memory": stored, "config.json": onDisk, "runtime request": handed} {
		if strings.Contains(string(blob), secret) {
			t.Errorf("%s holds the credential", name)
		}
	}
}
