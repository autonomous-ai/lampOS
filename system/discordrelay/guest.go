package discordrelay

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"net/http"
	"slices"
	"strings"
	"time"

	"go.autonomous.ai/os/system/domain"
)

const (
	guestTimeout    = 60 * time.Second
	guestMaxTokens  = 4000
	guestRateLimit  = 10
	guestRateWindow = time.Hour
	guestMaxInput   = 2000

	guestSystemPrompt = "You are a helpful assistant answering one message in a Discord server. " +
		"You are talking to a guest of the device owner. You have no tools, no memory of earlier " +
		"messages and no access to the owner's data, accounts or device. If asked for any of " +
		"those, say you cannot help with that here. Keep answers short."
)

// isGuest reports whether the author is on the owner's guest list. The owner is never a guest.
func (r *Relay) isGuest(authorID string) bool {
	_, guests := r.cfg.DiscordAccessLists()
	return authorID != "" && authorID != r.cfg.DiscordUserID && slices.Contains(guests, authorID)
}

// handleGuest answers a guest with a direct model call. The turn never reaches the
// agent, so a guest has no way to use its tools, memory, connected accounts or the
// device. A guest must @mention the bot in the connected server, every time.
func (r *Relay) handleGuest(ev domain.MQTTDiscordEventCommand) string {
	switch {
	case ev.GuildID == "" || r.cfg.DiscordGuildID == "" || ev.GuildID != r.cfg.DiscordGuildID:
		return "guest message outside the connected server"
	case !ev.MentionsBot:
		return "guest message without an @mention of the bot"
	}
	text := stripMention(ev.Text, ev.BotUserID)
	if text == "" {
		return "empty message"
	}
	if runes := []rune(text); len(runes) > guestMaxInput {
		text = string(runes[:guestMaxInput])
	}
	if !r.allowGuest(ev.DiscordUserID) {
		return "guest over the hourly limit"
	}
	select {
	case r.inFlight <- struct{}{}:
	default:
		return "busy"
	}
	go r.answerGuest(ev, text)
	return ""
}

func (r *Relay) answerGuest(ev domain.MQTTDiscordEventCommand, text string) {
	defer func() { <-r.inFlight }()
	up := r.getUplink()
	if up == nil {
		return
	}
	if err := up.Typing(ev.ChannelID, ev.WasSealed); err != nil {
		slog.Debug("discord relay: typing signal failed", "component", "discord-relay", "error", err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), guestTimeout)
	defer cancel()
	answer, err := r.guestModel(ctx, text)
	if err != nil {
		slog.Error("discord relay: guest answer failed", "component", "discord-relay", "error", err)
		return
	}
	if answer = strings.TrimSpace(answer); answer == "" {
		return
	}
	if err := up.Reply(ev.ChannelID, answer, ev.WasSealed); err != nil {
		slog.Error("discord relay: guest reply not sent", "component", "discord-relay", "error", err)
	}
}

// allowGuest applies the per-guest hourly limit.
func (r *Relay) allowGuest(id string) bool {
	r.mu.Lock()
	defer r.mu.Unlock()
	now := time.Now()
	kept := slices.DeleteFunc(r.guestCalls[id], func(t time.Time) bool { return now.Sub(t) >= guestRateWindow })
	if len(kept) >= guestRateLimit {
		r.guestCalls[id] = kept
		return false
	}
	r.guestCalls[id] = append(kept, now)
	return true
}

// callGuestModel sends one message, with no tools, to the configured model.
func (r *Relay) callGuestModel(ctx context.Context, text string) (string, error) {
	base := strings.TrimSuffix(strings.TrimRight(strings.TrimSpace(r.cfg.LLMBaseURL), "/"), "/v1")
	if base == "" || r.cfg.LLMAPIKey == "" {
		return "", errors.New("model not configured")
	}
	body, err := json.Marshal(map[string]any{
		"model":      r.cfg.LLMModelKey(),
		"max_tokens": guestMaxTokens,
		"system":     guestSystemPrompt,
		"messages":   []any{map[string]any{"role": "user", "content": text}},
	})
	if err != nil {
		return "", err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, base+"/v1/messages", bytes.NewReader(body))
	if err != nil {
		return "", err
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("x-api-key", r.cfg.LLMAPIKey)
	req.Header.Set("anthropic-version", "2023-06-01")
	req.Header.Set("X-Auto-Source", "discord-guest")
	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		return "", err
	}
	defer resp.Body.Close()
	data, err := io.ReadAll(io.LimitReader(resp.Body, 1<<20))
	if err != nil {
		return "", err
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return "", fmt.Errorf("model status %d", resp.StatusCode)
	}
	var out struct {
		Content []struct {
			Type string `json:"type"`
			Text string `json:"text"`
		} `json:"content"`
	}
	if err := json.Unmarshal(data, &out); err != nil {
		return "", err
	}
	var b strings.Builder
	for _, c := range out.Content {
		if c.Type == "text" {
			b.WriteString(c.Text)
		}
	}
	return b.String(), nil
}
