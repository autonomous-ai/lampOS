package discordrelay

import (
	"fmt"
	"strings"
	"sync"
	"testing"
	"time"

	"go.autonomous.ai/os/system/domain"
	"go.autonomous.ai/os/system/server/config"
)

type fakeGateway struct {
	mu      sync.Mutex
	busy    bool
	seq     int
	sent    []string
	sentRun []string
	silent  map[string]bool
	sendErr error
}

func (g *fakeGateway) IsBusy() bool {
	g.mu.Lock()
	defer g.mu.Unlock()
	return g.busy
}
func (g *fakeGateway) setBusy(b bool) { g.mu.Lock(); g.busy = b; g.mu.Unlock() }
func (g *fakeGateway) NextChatRunID() (string, string) {
	g.mu.Lock()
	defer g.mu.Unlock()
	g.seq++
	return "req-" + string(rune('0'+g.seq)), "run-" + string(rune('0'+g.seq))
}
func (g *fakeGateway) SendChatMessageWithRun(msg, _, runID string) (string, error) {
	g.mu.Lock()
	defer g.mu.Unlock()
	if g.sendErr != nil {
		return "", g.sendErr
	}
	g.sent = append(g.sent, msg)
	g.sentRun = append(g.sentRun, runID)
	return runID, nil
}
func (g *fakeGateway) MarkSilentRun(runID string) {
	g.mu.Lock()
	defer g.mu.Unlock()
	if g.silent == nil {
		g.silent = map[string]bool{}
	}
	g.silent[runID] = true
}
func (g *fakeGateway) ConsumeSilentRun(runID string) bool {
	g.mu.Lock()
	defer g.mu.Unlock()
	ok := g.silent[runID]
	delete(g.silent, runID)
	return ok
}
func (g *fakeGateway) sentCount() int { g.mu.Lock(); defer g.mu.Unlock(); return len(g.sent) }

type fakeUplink struct {
	mu      sync.Mutex
	replies [][2]string
	typing  []string
}

func (u *fakeUplink) Reply(ch, text string, _ bool) error {
	u.mu.Lock()
	u.replies = append(u.replies, [2]string{ch, text})
	u.mu.Unlock()
	return nil
}
func (u *fakeUplink) Typing(ch string, _ bool) error {
	u.mu.Lock()
	u.typing = append(u.typing, ch)
	u.mu.Unlock()
	return nil
}
func (u *fakeUplink) replyCount() int  { u.mu.Lock(); defer u.mu.Unlock(); return len(u.replies) }
func (u *fakeUplink) typingCount() int { u.mu.Lock(); defer u.mu.Unlock(); return len(u.typing) }

func newTestRelay(gw *fakeGateway, up *fakeUplink) *Relay {
	cfg := &config.Config{DiscordManaged: true, DiscordUserID: "owner-1", DiscordGuildID: "guild-1"}
	r := New(cfg, gw)
	r.idlePoll = 5 * time.Millisecond
	r.typingTick = 10 * time.Millisecond
	r.SetUplink(up)
	return r
}

func waitFor(t *testing.T, what string, cond func() bool) {
	t.Helper()
	deadline := time.Now().Add(2 * time.Second)
	for time.Now().Before(deadline) {
		if cond() {
			return
		}
		time.Sleep(5 * time.Millisecond)
	}
	t.Fatalf("timed out waiting for %s", what)
}

func dmEvent(id, text string) domain.MQTTDiscordEventCommand {
	return domain.MQTTDiscordEventCommand{
		EventID: id, DiscordUserID: "owner-1", ChannelID: "chan-9",
		AuthorUsername: "alex", BotUserID: "bot-1", Text: text,
	}
}

func TestOwnerMessageBecomesTurnAndReplyGoesBackToSameChannel(t *testing.T) {
	gw, up := &fakeGateway{}, &fakeUplink{}
	r := newTestRelay(gw, up)

	if reason := r.Handle(dmEvent("e1", "turn the lamp on")); reason != "" {
		t.Fatalf("Handle refused the owner's message: %s", reason)
	}
	waitFor(t, "turn handed to the agent", func() bool { return gw.sentCount() == 1 })

	runID := gw.sentRun[0]
	if !strings.Contains(gw.sent[0], "Message from alex [id:owner-1]:\nturn the lamp on") || !strings.Contains(gw.sent[0], "[via:discord]") {
		t.Errorf("turn text = %q", gw.sent[0])
	}
	if !gw.silent[runID] {
		t.Error("the run must be silent: the reply goes to Discord, not the speaker")
	}
	if !r.IsOriginRun(runID) {
		t.Fatal("run not tracked as Discord-origin")
	}
	waitFor(t, "typing signal", func() bool { return up.typingCount() >= 1 })

	r.Finish(runID, "  Done, the lamp is on.  ")
	waitFor(t, "reply uplink", func() bool { return up.replyCount() == 1 })
	if got := up.replies[0]; got != [2]string{"chan-9", "Done, the lamp is on."} {
		t.Errorf("reply = %v", got)
	}
	if r.IsOriginRun(runID) {
		t.Error("origin must be consumed once the reply is sent")
	}
	// Typing stops once the turn is finished.
	n := up.typingCount()
	time.Sleep(60 * time.Millisecond)
	if up.typingCount() != n {
		t.Error("typing signal kept firing after the turn finished")
	}
}

func TestOnlyTheOwnerIsHeard(t *testing.T) {
	gw, up := &fakeGateway{}, &fakeUplink{}
	r := newTestRelay(gw, up)

	stranger := dmEvent("e2", "hello")
	stranger.DiscordUserID = "someone-else"
	if reason := r.Handle(stranger); reason == "" {
		t.Error("a message from another user must be refused")
	}

	otherGuild := dmEvent("e3", "<@bot-1> hello")
	otherGuild.GuildID, otherGuild.MentionsBot = "guild-2", true
	if reason := r.Handle(otherGuild); reason == "" {
		t.Error("a message from another server must be refused")
	}

	noMention := dmEvent("e4", "hello")
	noMention.GuildID = "guild-1"
	if reason := r.Handle(noMention); reason == "" {
		t.Error("a server message without a mention must be refused")
	}

	time.Sleep(30 * time.Millisecond)
	if gw.sentCount() != 0 {
		t.Errorf("agent received %d turns, want 0", gw.sentCount())
	}
}

func TestServerMessageWithMentionIsAcceptedAndMentionStripped(t *testing.T) {
	gw, up := &fakeGateway{}, &fakeUplink{}
	r := newTestRelay(gw, up)

	ev := dmEvent("e5", "<@bot-1> what time is it")
	ev.GuildID, ev.MentionsBot = "guild-1", true
	if reason := r.Handle(ev); reason != "" {
		t.Fatalf("refused: %s", reason)
	}
	waitFor(t, "turn", func() bool { return gw.sentCount() == 1 })
	if strings.Contains(gw.sent[0], "<@bot-1>") || !strings.Contains(gw.sent[0], "what time is it") {
		t.Errorf("turn text = %q", gw.sent[0])
	}
}

func TestDuplicateEventIsIgnored(t *testing.T) {
	gw, up := &fakeGateway{}, &fakeUplink{}
	r := newTestRelay(gw, up)

	if reason := r.Handle(dmEvent("dup", "hi")); reason != "" {
		t.Fatal(reason)
	}
	if reason := r.Handle(dmEvent("dup", "hi")); reason != "duplicate" {
		t.Errorf("second delivery reason = %q, want duplicate", reason)
	}
	waitFor(t, "turn", func() bool { return gw.sentCount() == 1 })
	time.Sleep(30 * time.Millisecond)
	if gw.sentCount() != 1 {
		t.Errorf("agent received %d turns, want 1", gw.sentCount())
	}
}

func TestNotSetUpMeansNothingIsAccepted(t *testing.T) {
	gw, up := &fakeGateway{}, &fakeUplink{}
	r := newTestRelay(gw, up)
	r.cfg.DiscordManaged = false
	if reason := r.Handle(dmEvent("e6", "hi")); reason == "" {
		t.Error("must refuse when Discord is not set up")
	}
	r.cfg.DiscordManaged, r.cfg.DiscordUserID = true, ""
	if reason := r.Handle(dmEvent("e7", "hi")); reason == "" {
		t.Error("must refuse without an owner id")
	}
}

func TestMessageWaitsForIdleAgent(t *testing.T) {
	gw, up := &fakeGateway{busy: true}, &fakeUplink{}
	r := newTestRelay(gw, up)

	if reason := r.Handle(dmEvent("e8", "hi")); reason != "" {
		t.Fatal(reason)
	}
	time.Sleep(40 * time.Millisecond)
	if gw.sentCount() != 0 {
		t.Fatal("turn was sent while the agent was busy")
	}
	gw.setBusy(false)
	waitFor(t, "turn after idle", func() bool { return gw.sentCount() == 1 })
}

func TestSendFailureDropsTheRun(t *testing.T) {
	gw, up := &fakeGateway{sendErr: errTest}, &fakeUplink{}
	r := newTestRelay(gw, up)

	if reason := r.Handle(dmEvent("e9", "hi")); reason != "" {
		t.Fatal(reason)
	}
	waitFor(t, "run dropped", func() bool {
		r.mu.Lock()
		defer r.mu.Unlock()
		return len(r.runs) == 0
	})
	if len(gw.silent) != 0 {
		t.Error("silent mark must be undone when the send fails")
	}
}

type testErr string

func (e testErr) Error() string { return string(e) }

const errTest = testErr("agent unavailable")

func TestFinishIgnoresRunsThatDidNotComeFromDiscord(t *testing.T) {
	gw, up := &fakeGateway{}, &fakeUplink{}
	r := newTestRelay(gw, up)
	r.Finish("voice-run-1", "spoken reply")
	time.Sleep(20 * time.Millisecond)
	if up.replyCount() != 0 {
		t.Error("a non-Discord run produced a Discord reply")
	}
}

func TestFinishWithEmptyTextSendsNothingButEndsTheRun(t *testing.T) {
	gw, up := &fakeGateway{}, &fakeUplink{}
	r := newTestRelay(gw, up)
	if reason := r.Handle(dmEvent("e10", "hi")); reason != "" {
		t.Fatal(reason)
	}
	waitFor(t, "turn", func() bool { return gw.sentCount() == 1 })
	runID := gw.sentRun[0]
	r.Finish(runID, "   ")
	if r.IsOriginRun(runID) {
		t.Error("run must end even with nothing to say")
	}
	time.Sleep(20 * time.Millisecond)
	if up.replyCount() != 0 {
		t.Error("empty reply was sent")
	}
}

func TestNilRelayIsSafe(t *testing.T) {
	var r *Relay
	r.SetUplink(nil)
	r.Finish("x", "y")
	if r.IsOriginRun("x") || r.Enabled() {
		t.Error("nil relay must report nothing")
	}
	if r.Handle(domain.MQTTDiscordEventCommand{}) == "" {
		t.Error("nil relay must refuse")
	}
}

func TestAcceptRules(t *testing.T) {
	cases := []struct {
		name string
		in   Inbound
		want bool
	}{
		{"owner dm", Inbound{AuthorID: "o", Content: "hi"}, true},
		{"other dm", Inbound{AuthorID: "x", Content: "hi"}, false},
		{"bot itself", Inbound{AuthorID: "o", BotUserID: "o", Content: "hi"}, false},
		{"guild ok", Inbound{AuthorID: "o", GuildID: "g", MentionsBot: true, Content: "hi"}, true},
		{"guild wrong", Inbound{AuthorID: "o", GuildID: "g2", MentionsBot: true, Content: "hi"}, false},
		{"guild no mention", Inbound{AuthorID: "o", GuildID: "g", Content: "hi"}, false},
		{"only a mention", Inbound{AuthorID: "o", BotUserID: "b", GuildID: "g", MentionsBot: true, Content: "<@b>"}, false},
	}
	for _, c := range cases {
		if _, ok := Accept(c.in, "o", "g"); ok != c.want {
			t.Errorf("%s: accepted = %v, want %v", c.name, ok, c.want)
		}
	}
	if _, ok := Accept(Inbound{AuthorID: "o", GuildID: "g", MentionsBot: true, Content: "hi"}, "o", ""); !ok {
		t.Error("with no server configured, the owner's mention must be accepted from any server")
	}
	if _, ok := Accept(Inbound{AuthorID: "o", GuildID: "g", Content: "hi"}, "o", ""); ok {
		t.Error("a server message without a mention must be refused")
	}
}

func TestRejectReasonMatchesAccept(t *testing.T) {
	cases := []struct {
		in   Inbound
		want string
	}{
		{Inbound{AuthorID: "x", Content: "hi"}, "not from the owner"},
		{Inbound{AuthorID: "o", BotUserID: "o", Content: "hi"}, "from the bot itself"},
		{Inbound{AuthorID: "o", GuildID: "other", MentionsBot: true, Content: "hi"}, "from another server"},
		{Inbound{AuthorID: "o", GuildID: "g", Content: "hi"}, "server message without an @mention of the bot"},
		{Inbound{AuthorID: "o", BotUserID: "b", GuildID: "g", MentionsBot: true, Content: "<@b>"}, "empty message"},
		{Inbound{AuthorID: "o", GuildID: "g", MentionsBot: true, Content: "hi"}, ""},
	}
	for _, c := range cases {
		_, ok := Accept(c.in, "o", "g")
		got := RejectReason(c.in, "o", "g")
		if got != c.want {
			t.Errorf("RejectReason(%+v) = %q, want %q", c.in, got, c.want)
		}
		if ok != (got == "") {
			t.Errorf("Accept and RejectReason disagree for %+v: ok=%v reason=%q", c.in, ok, got)
		}
	}
}

func TestServerMessageNeedsMentionOutsideOpenPlaces(t *testing.T) {
	base := domain.MQTTDiscordEventCommand{
		DiscordUserID: "owner-1", GuildID: "guild-1", Text: "hello", BotUserID: "bot-1",
	}
	event := func(id, channel, parent string, mentions bool, author string) domain.MQTTDiscordEventCommand {
		ev := base
		ev.EventID, ev.ChannelID, ev.ParentChannelID, ev.MentionsBot = id, channel, parent, mentions
		ev.DiscordUserID = author
		return ev
	}
	const needsMention = "server message without an @mention of the bot"

	gw, up := &fakeGateway{}, &fakeUplink{}
	r := newTestRelay(gw, up)
	r.cfg.DiscordOpenChannels = []string{"open-1"}

	steps := []struct {
		name string
		ev   domain.MQTTDiscordEventCommand
		want string
	}{
		{"ordinary channel", event("1", "chan-1", "", false, "owner-1"), needsMention},
		{"chosen channel", event("2", "open-1", "", false, "owner-1"), ""},
		{"thread of chosen channel", event("3", "thread-0", "open-1", false, "owner-1"), ""},
		{"thread not joined", event("4", "thread-1", "chan-1", false, "owner-1"), needsMention},
		{"mention joins thread", event("5", "thread-1", "chan-1", true, "owner-1"), ""},
		{"joined thread", event("6", "thread-1", "chan-1", false, "owner-1"), ""},
		{"other person in joined thread", event("7", "thread-1", "chan-1", false, "someone"), "not from the owner"},
		{"addressed to someone else in chosen channel", func() domain.MQTTDiscordEventCommand {
			ev := event("10", "open-1", "", false, "owner-1")
			ev.MentionsOthers = true
			return ev
		}(), needsMention},
		{"addressed to someone else and the bot", func() domain.MQTTDiscordEventCommand {
			ev := event("11", "open-1", "", true, "owner-1")
			ev.MentionsOthers = true
			return ev
		}(), ""},
		{"other person in chosen channel", event("8", "open-1", "", false, "someone"), "not from the owner"},
	}
	for _, s := range steps {
		if got := r.Handle(s.ev); got != s.want {
			t.Errorf("%s: got %q, want %q", s.name, got, s.want)
		}
	}

	other := event("9", "open-1", "", false, "owner-1")
	other.GuildID = "guild-2"
	if got := r.Handle(other); got != "from another server" {
		t.Errorf("another server: got %q", got)
	}
}

func TestJoinedThreadsAreBounded(t *testing.T) {
	r := newTestRelay(&fakeGateway{}, &fakeUplink{})
	for i := 0; i <= maxJoinedThreads; i++ {
		r.joinThread(fmt.Sprintf("thread-%d", i))
	}
	if len(r.joined) != maxJoinedThreads || r.joined[0] != "thread-1" {
		t.Fatalf("len=%d first=%s", len(r.joined), r.joined[0])
	}
}
