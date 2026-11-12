package gatewayd

import (
	"context"
	"encoding/json"
	"strings"
	"testing"
	"time"
)

const (
	discordText = "[discord] Message from owner [id:1]:\nwhat is on my calendar?\n[via:discord]"
	voiceText   = "[user] turn on the light"
)

func newHoldServer(t *testing.T) *Server {
	t.Helper()
	ctx, cancel := context.WithCancel(context.Background())
	t.Cleanup(cancel)
	return &Server{lifetimeContext: ctx, ops: make(chan op, 8), cfg: Config{TurnTimeout: time.Minute}}
}

func TestMustHoldByDestination(t *testing.T) {
	cases := []struct {
		name          string
		active        bool
		activeDiscord bool
		content       string
		want          bool
	}{
		{"no active turn, voice", false, false, voiceText, false},
		{"no active turn, discord", false, false, discordText, false},
		{"stale discord flag, no active turn", false, true, voiceText, false},
		{"voice into voice", true, false, voiceText, false},
		{"voice into discord", true, true, voiceText, true},
		{"discord into voice", true, false, discordText, true},
		{"discord into discord", true, true, discordText, true},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			s := newHoldServer(t)
			if c.active {
				s.activeTurnID = "turn-1"
			}
			s.activeDiscord = c.activeDiscord
			if got := s.mustHold(turnPayload{Content: c.content}); got != c.want {
				t.Fatalf("mustHold = %v, want %v", got, c.want)
			}
		})
	}
}

func TestHeldMessageWaitsThenQueuesAsATurn(t *testing.T) {
	s := newHoldServer(t)
	s.activeTurnID = "turn-1"
	s.activeDiscord = true

	steerWouldRun := s.holdForDestination(turnPayload{Content: voiceText, RunID: "voice"})
	if !steerWouldRun {
		t.Fatal("speech during a Discord turn was not held")
	}
	select {
	case o := <-s.ops:
		t.Fatalf("held message was queued while the Discord turn was active: %+v", o)
	case <-time.After(3 * holdPoll):
	}

	s.mu.Lock()
	s.activeTurnID = ""
	s.mu.Unlock()
	select {
	case o := <-s.ops:
		if o.kind != opTurn || o.payload.RunID != "voice" {
			t.Fatalf("unexpected op %+v", o)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("held message was not released when the turn ended")
	}
	deadline := time.Now().Add(time.Second)
	for time.Now().Before(deadline) {
		s.mu.Lock()
		n := s.held
		s.mu.Unlock()
		if n == 0 {
			return
		}
		time.Sleep(10 * time.Millisecond)
	}
	t.Fatal("held counter not released")
}

func TestSameDestinationIsNotHeld(t *testing.T) {
	s := newHoldServer(t)
	s.activeTurnID = "turn-1"
	if s.holdForDestination(turnPayload{Content: voiceText}) {
		t.Fatal("speech during a speech turn must steer, not wait")
	}
}

func TestHeldMessagesAreBounded(t *testing.T) {
	s := newHoldServer(t)
	s.activeTurnID = "turn-1"
	s.activeDiscord = true
	for i := 0; i < maxHeld+3; i++ {
		if !s.holdForDestination(turnPayload{Content: voiceText}) {
			t.Fatal("message neither held nor rejected")
		}
	}
	s.mu.Lock()
	n := s.held
	s.mu.Unlock()
	if n != maxHeld {
		t.Fatalf("held %d, want %d", n, maxHeld)
	}
}

func TestHeldMessageStopsWhenServerStops(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	s := &Server{lifetimeContext: ctx, ops: make(chan op, 2), cfg: Config{TurnTimeout: time.Minute}}
	s.activeTurnID = "turn-1"
	s.activeDiscord = true
	s.holdForDestination(turnPayload{Content: voiceText})
	cancel()
	deadline := time.Now().Add(2 * time.Second)
	for time.Now().Before(deadline) {
		s.mu.Lock()
		n := s.held
		s.mu.Unlock()
		if n == 0 {
			return
		}
		time.Sleep(10 * time.Millisecond)
	}
	t.Fatal("held message goroutine did not stop")
}

// A Discord turn that starts while the skill preload runs (unlocked) must not receive
// speech that passed the first destination check.
func TestSteerRechecksDestinationAfterPreload(t *testing.T) {
	s := newHoldServer(t)
	capture := &jevCaptureWriter{}
	s.app = &appServer{s: s, in: capture, callbacks: map[int]func(json.RawMessage, json.RawMessage){}}
	s.threadID = "thread"
	s.activeTurnID = "voice-turn"
	s.preloadContext = func(context.Context, string) string {
		s.mu.Lock()
		s.activeTurnID, s.activeDiscord = "discord-turn", true
		s.mu.Unlock()
		return ""
	}

	s.steerAppTurn(turnPayload{Content: voiceText, Source: "user", RunID: "voice"})

	if strings.Contains(capture.String(), "turn/steer") {
		t.Fatalf("speech was steered into the Discord turn: %s", capture.String())
	}
	s.mu.Lock()
	held := s.held
	s.mu.Unlock()
	if held != 1 {
		t.Fatalf("speech should wait for the Discord turn, held=%d", held)
	}
}
