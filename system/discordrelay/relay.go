// Package discordrelay connects the agent to Discord through the cloud relay.
//
// The device holds no Discord bot token and opens no Discord connection. The
// cloud receives the owner's messages and sends each one to the device over MQTT
// (cmd "discord_event"); the device answers over the same transport
// ("discord_reply" / "discord_typing" on the device channel) and the cloud posts
// it. This package is the device half: it checks that a message is from the
// owner, hands it to the active agent runtime as a chat turn, remembers which
// Discord channel the turn came from, and returns the finished reply there.
//
// It is runtime-agnostic: it only needs the AgentGateway chat entry points, and
// the reply is collected where every runtime's finished turn is already handled.
package discordrelay

import (
	"context"
	"log/slog"
	"slices"
	"strings"
	"sync"
	"time"

	"github.com/google/wire"

	"go.autonomous.ai/os/system/domain"
	"go.autonomous.ai/os/system/server/config"
)

const (
	// idleWait is how long an accepted message waits for the agent to finish its
	// current turn before it is sent anyway.
	idleWait = 2 * time.Minute

	// idlePoll is how often that wait rechecks the agent.
	idlePoll = 500 * time.Millisecond

	// typingInterval is the refresh period of the typing signal (Discord shows it
	// for about 10 seconds).
	typingInterval = 8 * time.Second

	// runTTL bounds how long a turn may stay unanswered before its origin and
	// typing signal are dropped.
	runTTL = 10 * time.Minute

	// maxInFlight bounds messages waiting to be sent to the runtime.
	maxInFlight = 8

	// dedupTTL is how long a seen event id is remembered; the relay can redeliver.
	dedupTTL = 5 * time.Minute

	// maxJoinedThreads bounds the threads remembered as already joined.
	maxJoinedThreads = 256
)

// Gateway is the slice of domain.AgentGateway the relay uses.
type Gateway interface {
	IsBusy() bool
	NextChatRunID() (reqID string, runID string)
	SendChatMessageWithRun(msg string, reqID string, runID string) (string, error)
	MarkSilentRun(runID string)
	ConsumeSilentRun(runID string) bool
}

// Uplink sends the device's answers to the cloud relay.
type Uplink interface {
	// Reply sends the finished reply text for a Discord channel; sealed when the
	// message it answers arrived sealed.
	Reply(channelID, text string, sealed bool) error
	// ReplyInThread is Reply, in a new thread opened on messageID.
	ReplyInThread(channelID, messageID, threadName, text string, sealed bool) error
	// Typing shows the typing indicator in a Discord channel; sealed as above.
	Typing(channelID string, sealed bool) error
}

// Relay is the device side of the Discord relay. Its methods are safe on a nil
// receiver (they do nothing), so callers that were built without one keep working.
type Relay struct {
	cfg *config.Config
	gw  Gateway

	mu     sync.Mutex
	uplink Uplink
	runs   map[string]run
	seen   map[string]time.Time
	// joined holds the threads the owner has @mentioned the bot in, oldest first.
	// It lives in memory: after a restart one new @mention brings a thread back.
	joined []string
	// guestCalls holds each guest's recent answers, for the hourly limit.
	guestCalls map[string][]time.Time

	inFlight chan struct{}

	// Test seams.
	idleWait   time.Duration
	idlePoll   time.Duration
	typingTick time.Duration
	guestModel func(ctx context.Context, text string) (string, error)
}

type run struct {
	channelID string
	sealed    bool
	at        time.Time
	// threadOn is the message a new thread may be opened on; empty when not allowed.
	threadOn string
}

// ProviderSet wires the relay into the server.
var ProviderSet = wire.NewSet(ProvideRelay)

// ProvideRelay is the Wire provider for Relay.
func ProvideRelay(cfg *config.Config, gw domain.AgentGateway) *Relay {
	return New(cfg, gw)
}

// New builds a Relay. The uplink is installed later with SetUplink, once the MQTT
// side exists.
func New(cfg *config.Config, gw Gateway) *Relay {
	r := &Relay{
		cfg:        cfg,
		gw:         gw,
		runs:       make(map[string]run),
		seen:       make(map[string]time.Time),
		inFlight:   make(chan struct{}, maxInFlight),
		idleWait:   idleWait,
		idlePoll:   idlePoll,
		typingTick: typingInterval,
		guestCalls: make(map[string][]time.Time),
	}
	r.guestModel = r.callGuestModel
	return r
}

// SetUplink installs the transport used for replies and typing signals.
func (r *Relay) SetUplink(u Uplink) {
	if r == nil {
		return
	}
	r.mu.Lock()
	r.uplink = u
	r.mu.Unlock()
}

func (r *Relay) getUplink() Uplink {
	r.mu.Lock()
	defer r.mu.Unlock()
	return r.uplink
}

// Enabled reports whether the device is set up for Discord through the relay.
func (r *Relay) Enabled() bool {
	return r != nil && r.cfg != nil && r.cfg.DiscordManaged && r.cfg.DiscordUserID != ""
}

// Handle processes one discord_event. It never blocks on the agent: an accepted
// message is sent to the runtime from a goroutine. The returned reason says why a
// message was not taken ("" when it was).
func (r *Relay) Handle(ev domain.MQTTDiscordEventCommand) string {
	if r == nil {
		return "relay unavailable"
	}
	if !r.Enabled() {
		return "discord is not set up on this device"
	}
	if strings.TrimSpace(ev.Text) == "" || ev.ChannelID == "" {
		return "empty message"
	}
	if r.seenBefore(ev.EventID) {
		return "duplicate"
	}
	if r.isGuest(ev.DiscordUserID) {
		return r.handleGuest(ev)
	}
	in := Inbound{
		AuthorID:    ev.DiscordUserID,
		BotUserID:   ev.BotUserID,
		GuildID:     ev.GuildID,
		Content:     ev.Text,
		MentionsBot: ev.MentionsBot,

		NoMentionNeeded: r.noMentionNeeded(ev),
	}
	text, ok := Accept(in, r.cfg.DiscordUserID, r.cfg.DiscordGuildID)
	if !ok {
		return RejectReason(in, r.cfg.DiscordUserID, r.cfg.DiscordGuildID)
	}
	if ev.GuildID != "" && ev.ParentChannelID != "" && ev.MentionsBot {
		r.joinThread(ev.ChannelID)
	}

	select {
	case r.inFlight <- struct{}{}:
	default:
		slog.Warn("discord relay: too many messages waiting, dropping one",
			"component", "discord-relay", "event_id", ev.EventID)
		return "busy"
	}

	reqID, runID := r.gw.NextChatRunID()
	threadOn := ""
	if ev.GuildID != "" && ev.ParentChannelID == "" {
		threadOn = ev.EventID
	}
	r.addRun(runID, ev.ChannelID, ev.WasSealed, threadOn)
	// The via marker also tells a runtime that merges concurrent messages into a running
	// turn to keep this one separate, so its reply is never shared with another message.
	body := TurnText(ev.AuthorUsername, ev.DiscordUserID, text)
	if threadOn != "" {
		body += threadHint
	}
	turn := domain.AppendVia(body, domain.ViaDiscord)
	go r.send(reqID, runID, ev.ChannelID, ev.WasSealed, turn)
	return ""
}

// noMentionNeeded reports whether a server message may reach the agent without an
// @mention: it is in a channel the owner chose, in a thread of such a channel, or in
// a thread the bot has already joined. A message addressed to someone else is not for
// the agent, so it still needs its own @mention.
func (r *Relay) noMentionNeeded(ev domain.MQTTDiscordEventCommand) bool {
	if ev.GuildID == "" || ev.MentionsOthers {
		return false
	}
	open, _ := r.cfg.DiscordAccessLists()
	if slices.Contains(open, ev.ChannelID) {
		return true
	}
	if ev.ParentChannelID == "" {
		return false
	}
	if slices.Contains(open, ev.ParentChannelID) {
		return true
	}
	r.mu.Lock()
	defer r.mu.Unlock()
	return slices.Contains(r.joined, ev.ChannelID)
}

// joinThread remembers a thread as joined, dropping the oldest past the limit.
func (r *Relay) joinThread(channelID string) {
	r.mu.Lock()
	defer r.mu.Unlock()
	if slices.Contains(r.joined, channelID) {
		return
	}
	r.joined = append(r.joined, channelID)
	if len(r.joined) > maxJoinedThreads {
		r.joined = r.joined[1:]
	}
}

// send waits for the agent to be idle, hands it the turn and, meanwhile, keeps the
// typing signal alive.
func (r *Relay) send(reqID, runID, channelID string, sealed bool, turn string) {
	defer func() { <-r.inFlight }()
	go r.keepTyping(runID, channelID, sealed)

	deadline := time.Now().Add(r.idleWait)
	for r.gw.IsBusy() && time.Now().Before(deadline) {
		time.Sleep(r.idlePoll)
	}
	// The reply goes to Discord, never to the speaker.
	r.gw.MarkSilentRun(runID)
	if _, err := r.gw.SendChatMessageWithRun(turn, reqID, runID); err != nil {
		r.gw.ConsumeSilentRun(runID)
		r.take(runID)
		slog.Error("discord relay: could not hand the message to the agent",
			"component", "discord-relay", "run_id", runID, "error", err)
		return
	}
	slog.Info("discord relay: message sent to the agent", "component", "discord-relay", "run_id", runID)
}

// keepTyping shows the typing indicator until the turn is finished or runTTL passes.
func (r *Relay) keepTyping(runID, channelID string, sealed bool) {
	deadline := time.Now().Add(runTTL)
	for {
		if !r.IsOriginRun(runID) || time.Now().After(deadline) {
			return
		}
		if up := r.getUplink(); up != nil {
			if err := up.Typing(channelID, sealed); err != nil {
				slog.Debug("discord relay: typing signal failed", "component", "discord-relay", "error", err)
			}
		}
		time.Sleep(r.typingTick)
	}
}

// IsOriginRun reports, without consuming it, whether runID is a turn that came from
// Discord and still awaits its reply.
func (r *Relay) IsOriginRun(runID string) bool {
	if r == nil || runID == "" {
		return false
	}
	r.mu.Lock()
	defer r.mu.Unlock()
	_, ok := r.runs[runID]
	return ok
}

// Finish ends a Discord-origin turn: the reply text (when not empty) goes back to
// the channel the turn came from. It is a no-op for a run that did not come from
// Discord, so it is safe to call for every finished turn. The origin is consumed
// at once; the send happens in the background.
func (r *Relay) Finish(runID, text string) {
	if r == nil {
		return
	}
	origin, ok := r.take(runID)
	if !ok {
		return
	}
	title, text := splitThreadDirective(strings.TrimSpace(text))
	if text == "" {
		return
	}
	up := r.getUplink()
	if up == nil {
		return
	}
	go func() {
		var err error
		if title != "" && origin.threadOn != "" {
			// A thread opened on a message takes that message's id, so later
			// messages there count as a joined thread.
			r.joinThread(origin.threadOn)
			err = up.ReplyInThread(origin.channelID, origin.threadOn, title, text, origin.sealed)
		} else {
			err = up.Reply(origin.channelID, text, origin.sealed)
		}
		if err != nil {
			slog.Error("discord relay: reply not sent", "component", "discord-relay", "run_id", runID, "error", err)
			return
		}
		slog.Info("discord relay: reply sent", "component", "discord-relay", "run_id", runID)
	}()
}

func (r *Relay) addRun(runID, channelID string, sealed bool, threadOn string) {
	r.mu.Lock()
	defer r.mu.Unlock()
	cutoff := time.Now().Add(-runTTL)
	for k, v := range r.runs {
		if v.at.Before(cutoff) {
			delete(r.runs, k)
		}
	}
	r.runs[runID] = run{channelID: channelID, sealed: sealed, at: time.Now(), threadOn: threadOn}
}

func (r *Relay) take(runID string) (run, bool) {
	r.mu.Lock()
	defer r.mu.Unlock()
	v, ok := r.runs[runID]
	if ok {
		delete(r.runs, runID)
	}
	return v, ok
}

// seenBefore records an event id and reports whether it was already seen recently.
func (r *Relay) seenBefore(eventID string) bool {
	if eventID == "" {
		return false
	}
	r.mu.Lock()
	defer r.mu.Unlock()
	now := time.Now()
	if t, ok := r.seen[eventID]; ok && now.Sub(t) < dedupTTL {
		return true
	}
	r.seen[eventID] = now
	if len(r.seen) > 1024 {
		for k, t := range r.seen {
			if now.Sub(t) >= dedupTTL {
				delete(r.seen, k)
			}
		}
	}
	return false
}
