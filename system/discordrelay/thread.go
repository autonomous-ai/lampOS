package discordrelay

import (
	"regexp"
	"strings"
)

// threadHint tells the agent how to answer in a new thread.
const threadHint = "\n(To answer in a new Discord thread, begin your reply with [[thread: short title]]. Only do this when asked or when the answer starts a longer discussion.)"

var threadDirective = regexp.MustCompile(`^\s*\[\[thread:\s*([^\]\n]{1,100}?)\s*\]\]\s*`)

// splitThreadDirective removes a leading thread directive and returns its title.
func splitThreadDirective(text string) (title, rest string) {
	m := threadDirective.FindStringSubmatch(text)
	if m == nil {
		return "", text
	}
	return m[1], strings.TrimSpace(text[len(m[0]):])
}
