package gatewayd

import (
	"log"
	"time"

	"go.autonomous.ai/os/system/domain"
)

const (
	// holdPoll is how often a held message checks whether the active turn has ended.
	holdPoll = 200 * time.Millisecond

	// maxHeld bounds the messages waiting for a turn of another reply destination.
	maxHeld = 8
)

// isDiscordTurn reports whether the message is a Discord turn: its reply is posted to
// one Discord channel, never spoken and never shared with another message.
func isDiscordTurn(p turnPayload) bool {
	return domain.HasVia(p.Content, domain.ViaDiscord)
}

// mustHold reports whether p may not be steered into the turn that is running now. One
// reply answers every message steered into a turn, so a Discord turn is never merged
// with another message in either direction, whatever the channel. Callers hold s.mu.
func (s *Server) mustHold(p turnPayload) bool {
	if s.activeTurnID == "" && !s.activeStarting {
		return false
	}
	return s.activeDiscord || isDiscordTurn(p)
}

// holdForDestination keeps a message that must not join the active turn waiting until
// that turn ends, then queues it as an ordinary turn. It reports whether it took the
// message. Messages for the same destination as the active turn are not touched, so
// they steer with no added delay.
func (s *Server) holdForDestination(p turnPayload) bool {
	s.mu.Lock()
	hold := s.mustHold(p)
	if hold {
		s.held++
		if s.held > maxHeld {
			s.held--
			s.mu.Unlock()
			log.Printf("%s too many messages waiting for the active turn — rejecting one", logPrefix)
			s.sendJSON(map[string]any{"type": "bridge.rejected", "error": "too many messages waiting", "request_id": p.RequestID, "run_id": p.RunID})
			return true
		}
	}
	s.mu.Unlock()
	if !hold {
		return false
	}
	log.Printf("%s message waits for the active turn (different reply destination)", logPrefix)
	go s.releaseHeld(p)
	return true
}

// releaseHeld polls until the message may run, then queues it. It gives up (and rejects
// the message, which ends its run on the client) once a whole turn could not have
// outlasted the wait, or when the server stops.
func (s *Server) releaseHeld(p turnPayload) {
	defer func() {
		s.mu.Lock()
		s.held--
		s.mu.Unlock()
	}()
	timer := time.NewTimer(s.cfg.TurnTimeout + time.Minute)
	defer timer.Stop()
	tick := time.NewTicker(holdPoll)
	defer tick.Stop()
	for {
		select {
		case <-s.lifetimeContext.Done():
			return
		case <-timer.C:
			s.sendJSON(map[string]any{"type": "bridge.rejected", "error": "timed out waiting for the active turn", "request_id": p.RequestID, "run_id": p.RunID})
			return
		case <-tick.C:
			s.mu.Lock()
			hold := s.mustHold(p)
			s.mu.Unlock()
			if !hold {
				// The worker re-checks before steering, so a new turn that began in the
				// meantime holds it again rather than merging.
				s.enqueue(op{kind: opTurn, payload: p})
				return
			}
		}
	}
}
