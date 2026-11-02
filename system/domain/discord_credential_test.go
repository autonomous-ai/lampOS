package domain

import (
	"encoding/json"
	"strings"
	"testing"
)

// Setup, add_channel and Wi-Fi provisioning requests may still arrive from older
// clients with a Discord credential; the device must not carry it any further.
func TestChannelRequestsCarryNoDiscordCredential(t *testing.T) {
	const secret = "fake-discord-secret-for-tests-0003"

	var setup SetupRequest
	if err := json.Unmarshal([]byte(`{"channel":"discord","discord_bot_token":"`+secret+`","discord_guild_id":"g","discord_user_id":"u"}`), &setup); err != nil {
		t.Fatal(err)
	}
	var wifi WifiProvisionRequest
	if err := json.Unmarshal([]byte(`{"ssid":"s","channel":"discord","discord_bot_token":"`+secret+`"}`), &wifi); err != nil {
		t.Fatal(err)
	}
	mqtt := MQTTAddChannelCommand{
		Channel: "discord",
		Config:  map[string]interface{}{"bot_token": secret, "guild_id": "g", "user_id": "u"},
	}
	add := mqtt.ToRequest()

	for name, v := range map[string]any{"setup": setup, "wifi-provision": wifi, "add_channel": add} {
		b, err := json.Marshal(v)
		if err != nil {
			t.Fatal(err)
		}
		if strings.Contains(string(b), secret) {
			t.Errorf("%s request kept the Discord credential: %s", name, b)
		}
	}
}

func TestDiscordChannelNeedsOnlyTheOwnerID(t *testing.T) {
	ownerOnly := SetupRequest{Channel: ChannelDiscord, DiscordUserID: "u-1"}
	if err := ownerOnly.ValidateChannel(); err != nil {
		t.Errorf("setup with only an owner id: %v", err)
	}
	if err := (&SetupRequest{Channel: ChannelDiscord, DiscordGuildID: "g-1"}).ValidateChannel(); err == nil {
		t.Error("setup without an owner id must be refused")
	}
	if err := (&AddChannelRequest{Channel: ChannelDiscord, DiscordUserID: "u-1"}).ValidateChannel(); err != nil {
		t.Errorf("add_channel with only an owner id: %v", err)
	}
	if err := (&AddChannelRequest{Channel: ChannelDiscord}).ValidateChannel(); err == nil {
		t.Error("add_channel without an owner id must be refused")
	}
}

func TestAddChannelCommandReadsRelayFlag(t *testing.T) {
	cmd := MQTTAddChannelCommand{Channel: "discord", Config: map[string]interface{}{"managed": true, "guild_id": "g", "user_id": "u"}}
	req := cmd.ToRequest()
	if !req.DiscordManaged || req.DiscordGuildID != "g" || req.DiscordUserID != "u" {
		t.Errorf("request = %+v", req)
	}
}
