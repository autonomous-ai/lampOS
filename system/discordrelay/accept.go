package discordrelay

import (
	"fmt"
	"regexp"
	"strings"
)

// Inbound is the transport-independent view of one relayed message that the pure
// accept filter works on.
type Inbound struct {
	AuthorID    string
	BotUserID   string
	GuildID     string // empty for a direct message
	Content     string
	MentionsBot bool
}

// Accept decides whether a relayed message is for the agent and returns the text
// to hand it. Only the configured owner is heard. A direct message is always
// accepted from the owner; a server message needs an @mention of the bot and, when
// one is configured, the configured server. The bot's own mention is removed from the text.
func Accept(in Inbound, allowedUser, allowedGuild string) (string, bool) {
	if allowedUser == "" || in.AuthorID != allowedUser {
		return "", false
	}
	if in.BotUserID != "" && in.AuthorID == in.BotUserID {
		return "", false
	}
	if in.GuildID != "" {
		if allowedGuild != "" && in.GuildID != allowedGuild {
			return "", false
		}
		if !in.MentionsBot {
			return "", false
		}
	}
	text := stripMention(in.Content, in.BotUserID)
	if text == "" {
		return "", false
	}
	return text, true
}

// RejectReason says why Accept refused a message, for the device log. It mirrors
// Accept's checks in the same order.
func RejectReason(in Inbound, allowedUser, allowedGuild string) string {
	switch {
	case allowedUser == "" || in.AuthorID != allowedUser:
		return "not from the owner"
	case in.BotUserID != "" && in.AuthorID == in.BotUserID:
		return "from the bot itself"
	case in.GuildID != "" && allowedGuild != "" && in.GuildID != allowedGuild:
		return "from another server"
	case in.GuildID != "" && !in.MentionsBot:
		return "server message without an @mention of the bot"
	case stripMention(in.Content, in.BotUserID) == "":
		return "empty message"
	}
	return ""
}

// stripMention removes every mention of the bot user (<@id> and the nickname form
// <@!id>) from content and trims the result.
func stripMention(content, botUserID string) string {
	if botUserID == "" {
		return strings.TrimSpace(content)
	}
	re := regexp.MustCompile(`<@!?` + regexp.QuoteMeta(botUserID) + `>`)
	return strings.TrimSpace(re.ReplaceAllString(content, ""))
}

// TurnText builds the text the agent receives: who is talking and from where, in
// the same shape the other chat channels use.
func TurnText(username, userID, text string) string {
	if username == "" {
		username = "unknown"
	}
	return fmt.Sprintf("[discord] Message from %s [id:%s]:\n%s", username, userID, text)
}
