package http

import (
	"encoding/json"
	"strings"
	"sync"
	"testing"
	"time"

	"go.autonomous.ai/os/system/discordrelay"
	"go.autonomous.ai/os/system/domain"
	"go.autonomous.ai/os/system/monitor"
	"go.autonomous.ai/os/system/server/config"
)

// discordGateway is the slice of the agent gateway a finished turn touches.
type discordGateway struct {
	domain.AgentGateway
	mu     sync.Mutex
	silent map[string]bool
	sent   []string
}

func (g *discordGateway) GetSessionKey() string                                 { return "test-session" }
func (g *discordGateway) IsWebChatRun(string) bool                              { return false }
func (g *discordGateway) ConsumeWebChatRun(string) bool                         { return false }
func (g *discordGateway) SetBusy(bool)                                          {}
func (g *discordGateway) ConsumeBroadcastRun(string) bool                       { return false }
func (g *discordGateway) ConsumeGuardRun(string) (string, bool)                 { return "", false }
func (g *discordGateway) FetchChatHistory(string, int) (json.RawMessage, error) { return nil, nil }
func (g *discordGateway) IsBusy() bool                                          { return false }
func (g *discordGateway) NextChatRunID() (string, string)                       { return "req-1", "device-chat-discord-1" }
func (g *discordGateway) SendChatMessageWithRun(msg, _, runID string) (string, error) {
	g.mu.Lock()
	g.sent = append(g.sent, msg)
	g.mu.Unlock()
	return runID, nil
}
func (g *discordGateway) MarkSilentRun(runID string) {
	g.mu.Lock()
	if g.silent == nil {
		g.silent = map[string]bool{}
	}
	g.silent[runID] = true
	g.mu.Unlock()
}
func (g *discordGateway) IsSilentRun(runID string) bool {
	g.mu.Lock()
	defer g.mu.Unlock()
	return g.silent[runID]
}
func (g *discordGateway) ConsumeSilentRun(runID string) bool {
	g.mu.Lock()
	defer g.mu.Unlock()
	ok := g.silent[runID]
	delete(g.silent, runID)
	return ok
}

type captureUplink struct {
	mu      sync.Mutex
	replies [][2]string
}

func (u *captureUplink) Reply(ch, text string, _ bool) error {
	u.mu.Lock()
	u.replies = append(u.replies, [2]string{ch, text})
	u.mu.Unlock()
	return nil
}
func (u *captureUplink) Typing(string, bool) error { return nil }
func (u *captureUplink) ReplyInThread(ch, _, _, text string, sealed bool) error {
	return u.Reply(ch, text, sealed)
}
func (u *captureUplink) snapshot() [][2]string {
	u.mu.Lock()
	defer u.mu.Unlock()
	return append([][2]string(nil), u.replies...)
}

func agentEvent(t *testing.T, runID, stream string, data map[string]any) domain.WSEvent {
	t.Helper()
	payload, err := json.Marshal(map[string]any{"runId": runID, "stream": stream, "data": data})
	if err != nil {
		t.Fatal(err)
	}
	return domain.WSEvent{Payload: payload}
}

func newDiscordHandler(t *testing.T) (*AgentHandler, *discordGateway, *captureUplink) {
	t.Helper()
	gw := &discordGateway{}
	up := &captureUplink{}
	relay := discordrelay.New(&config.Config{DiscordManaged: true, DiscordUserID: "owner-1"}, gw)
	relay.SetUplink(up)
	h := &AgentHandler{
		agentGateway:       gw,
		monitorBus:         monitor.ProvideBus(),
		config:             &config.Config{},
		assistantBuf:       make(map[string]*strings.Builder),
		streamedCleanLen:   make(map[string]int),
		firedHWCount:       make(map[string]int),
		streamStats:        make(map[string]*runStreamStats),
		ttsSuppressReasons: make(map[string]string),
		harnessReplies:     make(map[string]harnessReplyState),
		runIDMap:           make(map[string]string),
		channelRuns:        make(map[string]bool),
		cronFireRuns:       make(map[string]bool),
		agentLifecycleAt:   make(map[string]int64),
		runFirstSeenMs:     make(map[string]int64),
		ttsTurnOrder:       make(map[string]uint64),
	}
	h.SetDiscordRelay(relay)
	return h, gw, up
}

func startDiscordTurn(t *testing.T, h *AgentHandler, gw *discordGateway) string {
	t.Helper()
	relay := h.discordRelay
	if reason := relay.Handle(domain.MQTTDiscordEventCommand{
		EventID: "ev-1", DiscordUserID: "owner-1", ChannelID: "chan-7", AuthorUsername: "alex", Text: "what is the weather",
	}); reason != "" {
		t.Fatalf("relay refused the message: %s", reason)
	}
	deadline := time.Now().Add(2 * time.Second)
	for time.Now().Before(deadline) {
		gw.mu.Lock()
		n := len(gw.sent)
		gw.mu.Unlock()
		if n == 1 {
			return "device-chat-discord-1"
		}
		time.Sleep(5 * time.Millisecond)
	}
	t.Fatal("turn never reached the agent")
	return ""
}

func TestFinishedDiscordTurnReturnsReplyToItsChannel(t *testing.T) {
	h, gw, up := newDiscordHandler(t)
	runID := startDiscordTurn(t, h, gw)

	for _, e := range []domain.WSEvent{
		agentEvent(t, runID, "lifecycle", map[string]any{"phase": "start"}),
		agentEvent(t, runID, "assistant", map[string]any{"delta": "It is sunny and 24 degrees."}),
		agentEvent(t, runID, "lifecycle", map[string]any{"phase": "end"}),
	} {
		if err := h.handleAgentStreamEvent(e); err != nil {
			t.Fatal(err)
		}
	}

	deadline := time.Now().Add(2 * time.Second)
	for time.Now().Before(deadline) && len(up.snapshot()) == 0 {
		time.Sleep(5 * time.Millisecond)
	}
	got := up.snapshot()
	if len(got) != 1 || got[0] != [2]string{"chan-7", "It is sunny and 24 degrees."} {
		t.Fatalf("replies = %v, want one reply to chan-7", got)
	}
	if h.discordRelay.IsOriginRun(runID) {
		t.Error("run still tracked after its reply")
	}
}

func TestDiscordTurnWithNothingToSayStillEndsTheRun(t *testing.T) {
	h, gw, up := newDiscordHandler(t)
	runID := startDiscordTurn(t, h, gw)

	for _, e := range []domain.WSEvent{
		agentEvent(t, runID, "lifecycle", map[string]any{"phase": "start"}),
		agentEvent(t, runID, "lifecycle", map[string]any{"phase": "end"}),
	} {
		if err := h.handleAgentStreamEvent(e); err != nil {
			t.Fatal(err)
		}
	}
	if h.discordRelay.IsOriginRun(runID) {
		t.Error("an empty turn left the run (and its typing signal) open")
	}
	if len(up.snapshot()) != 0 {
		t.Error("an empty turn produced a reply")
	}
}

func TestFailedDiscordTurnEndsTheRunWithoutReply(t *testing.T) {
	h, gw, up := newDiscordHandler(t)
	runID := startDiscordTurn(t, h, gw)

	if err := h.handleAgentStreamEvent(agentEvent(t, runID, "lifecycle", map[string]any{"phase": "error", "error": "boom"})); err != nil {
		t.Fatal(err)
	}
	if h.discordRelay.IsOriginRun(runID) {
		t.Error("a failed turn left the run open")
	}
	if len(up.snapshot()) != 0 {
		t.Error("a failed turn produced a reply")
	}
}
