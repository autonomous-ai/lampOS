package mqtthandler

import (
	"encoding/json"
	"sync"
	"testing"
	"time"

	"go.autonomous.ai/os/system/discordrelay"
	"go.autonomous.ai/os/system/domain"
	"go.autonomous.ai/os/system/server/config"
)

type relayGateway struct {
	domain.AgentGateway
	mu   sync.Mutex
	sent []string
}

func (g *relayGateway) IsBusy() bool                    { return false }
func (g *relayGateway) NextChatRunID() (string, string) { return "req-1", "run-1" }
func (g *relayGateway) MarkSilentRun(string)            {}
func (g *relayGateway) ConsumeSilentRun(string) bool    { return true }
func (g *relayGateway) SendChatMessageWithRun(msg, _, runID string) (string, error) {
	g.mu.Lock()
	g.sent = append(g.sent, msg)
	g.mu.Unlock()
	return runID, nil
}
func (g *relayGateway) count() int { g.mu.Lock(); defer g.mu.Unlock(); return len(g.sent) }

func TestDiscordEventFromTheRelayReachesTheAgent(t *testing.T) {
	cfg := &config.Config{DiscordManaged: true, DiscordUserID: "owner-1"}
	gw := &relayGateway{}
	h := &DeviceMQTTHandler{config: cfg, discordRelay: discordrelay.New(cfg, gw)}

	payload := []byte(`{"cmd":"discord_event","event_id":"m1","discord_user_id":"owner-1","guild_id":"",` +
		`"discord_channel_id":"c1","author_username":"alex","bot_user_id":"b1","mentions_bot":false,"text":"hello lamp"}`)
	if err := h.HandleMessage("fa", payload); err != nil {
		t.Fatalf("HandleMessage: %v", err)
	}
	deadline := time.Now().Add(2 * time.Second)
	for time.Now().Before(deadline) && gw.count() == 0 {
		time.Sleep(5 * time.Millisecond)
	}
	if gw.count() != 1 {
		t.Fatal("the agent did not receive the Discord message")
	}
}

func TestDiscordEventFromSomeoneElseNeverReachesTheAgent(t *testing.T) {
	cfg := &config.Config{DiscordManaged: true, DiscordUserID: "owner-1"}
	gw := &relayGateway{}
	h := &DeviceMQTTHandler{config: cfg, discordRelay: discordrelay.New(cfg, gw)}

	payload := []byte(`{"cmd":"discord_event","event_id":"m2","discord_user_id":"stranger","discord_channel_id":"c1","text":"hello"}`)
	if err := h.HandleMessage("fa", payload); err != nil {
		t.Fatalf("HandleMessage: %v", err)
	}
	time.Sleep(50 * time.Millisecond)
	if gw.count() != 0 {
		t.Fatal("a message from another user reached the agent")
	}
}

func TestDiscordEventWithoutRelayIsIgnored(t *testing.T) {
	h := &DeviceMQTTHandler{config: &config.Config{}}
	if err := h.HandleMessage("fa", []byte(`{"cmd":"discord_event","text":"x"}`)); err != nil {
		t.Fatalf("HandleMessage: %v", err)
	}
}

func TestDiscordUplinkEnvelope(t *testing.T) {
	cfg := &config.Config{DeviceType: "intern-v2", DeviceID: "dev-1"}

	reply, err := discordEnvelope(cfg, domain.MsgTypeDiscordReply, "aa:bb", "chan-1", map[string]any{"text": "hi"})
	if err != nil {
		t.Fatal(err)
	}
	b, _ := json.Marshal(reply)
	var got map[string]any
	_ = json.Unmarshal(b, &got)
	for k, want := range map[string]any{
		"type": "discord_reply", "device": "intern-v2", "id": "dev-1",
		"discord_channel_id": "chan-1", "text": "hi",
	} {
		if got[k] != want {
			t.Errorf("reply[%q] = %v, want %v", k, got[k], want)
		}
	}

	typing, err := discordEnvelope(cfg, domain.MsgTypeDiscordTyping, "aa:bb", "chan-1", nil)
	if err != nil {
		t.Fatal(err)
	}
	if typing["type"] != "discord_typing" || typing["discord_channel_id"] != "chan-1" {
		t.Errorf("typing envelope = %v", typing)
	}
	if _, has := typing["text"]; has {
		t.Error("a typing signal carries no text")
	}
}

func sealedEventPayload(t *testing.T, key, deviceID string, body map[string]any) []byte {
	t.Helper()
	s, err := discordrelay.Seal(key, domain.CommandDiscordEvent, deviceID, body, time.Now())
	if err != nil {
		t.Fatal(err)
	}
	b, _ := json.Marshal(map[string]any{"cmd": domain.CommandDiscordEvent, "enc": discordrelay.SealVersion, "sealed": s})
	return b
}

func TestSealedDiscordEventReachesTheAgent(t *testing.T) {
	cfg := &config.Config{DiscordManaged: true, DiscordUserID: "owner-1", DeviceID: "dev1", LLMAPIKey: "lob_test_device_key"}
	gw := &relayGateway{}
	h := &DeviceMQTTHandler{config: cfg, discordRelay: discordrelay.New(cfg, gw)}

	body := map[string]any{"event_id": "s1", "discord_user_id": "owner-1", "discord_channel_id": "c1",
		"author_username": "alex", "bot_user_id": "b1", "text": "sealed hello"}
	if err := h.HandleMessage("fa", sealedEventPayload(t, "lob_test_device_key", "dev1", body)); err != nil {
		t.Fatalf("HandleMessage: %v", err)
	}
	deadline := time.Now().Add(2 * time.Second)
	for gw.count() == 0 && time.Now().Before(deadline) {
		time.Sleep(5 * time.Millisecond)
	}
	if gw.count() != 1 {
		t.Fatalf("the sealed message did not reach the agent")
	}
}

func TestSealedDiscordEventForAnotherDeviceIsDropped(t *testing.T) {
	cfg := &config.Config{DiscordManaged: true, DiscordUserID: "owner-1", DeviceID: "dev1", LLMAPIKey: "lob_test_device_key"}
	gw := &relayGateway{}
	h := &DeviceMQTTHandler{config: cfg, discordRelay: discordrelay.New(cfg, gw)}

	body := map[string]any{"event_id": "s2", "discord_user_id": "owner-1", "discord_channel_id": "c1", "text": "not yours"}
	for _, p := range [][]byte{
		sealedEventPayload(t, "lob_other_key", "dev1", body),
		sealedEventPayload(t, "lob_test_device_key", "dev2", body),
	} {
		if err := h.HandleMessage("fa", p); err != nil {
			t.Fatalf("HandleMessage: %v", err)
		}
	}
	time.Sleep(50 * time.Millisecond)
	if gw.count() != 0 {
		t.Fatalf("a message sealed for someone else reached the agent")
	}
}

func TestSealedReplyEnvelopeHidesChannelAndText(t *testing.T) {
	cfg := &config.Config{DeviceID: "dev1", LLMAPIKey: "lob_test_device_key"}
	m, err := sealedDiscordEnvelope(cfg, domain.MsgTypeDiscordReply, "intern-v2-0001", "c1", map[string]any{"text": "secret reply"}, time.Now())
	if err != nil {
		t.Fatal(err)
	}
	if _, ok := m["discord_channel_id"]; ok {
		t.Fatal("channel id in clear")
	}
	if _, ok := m["text"]; ok {
		t.Fatal("text in clear")
	}
	if m["type"] != domain.MsgTypeDiscordReply || m["enc"] != discordrelay.SealVersion || m["id"] != "dev1" {
		t.Fatalf("routing fields: %v", m)
	}
	var got struct {
		ChannelID string `json:"discord_channel_id"`
		Text      string `json:"text"`
	}
	if err := discordrelay.Open("lob_test_device_key", domain.MsgTypeDiscordReply, "dev1", discordrelay.SealVersion, m["sealed"].(string), &got, time.Now()); err != nil {
		t.Fatal(err)
	}
	if got.ChannelID != "c1" || got.Text != "secret reply" {
		t.Fatalf("got %+v", got)
	}
}
