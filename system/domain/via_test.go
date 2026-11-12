package domain

import "testing"

func TestAppendVia(t *testing.T) {
	tests := []struct{ msg, source, want string }{
		{"[user] hello", "mobile", "[user] hello\n[via:mobile]"},
		{"Summarize my calendar.", ViaSchedule, "Summarize my calendar.\n[via:schedule]"},
		{"/status", "web", "/status"},                              // command router needs the literal text
		{"[user] hi\n[via:web]", "mobile", "[user] hi\n[via:web]"}, // never stamped twice
		{"", "web", ""},
		{"[user] hi", "", "[user] hi"},
	}
	for _, tt := range tests {
		if got := AppendVia(tt.msg, tt.source); got != tt.want {
			t.Errorf("AppendVia(%q, %q) = %q, want %q", tt.msg, tt.source, got, tt.want)
		}
	}
}

func TestHasVia(t *testing.T) {
	tests := []struct {
		msg, source string
		want        bool
	}{
		{AppendVia("[discord] Message from a [id:1]:\nhi", ViaDiscord), ViaDiscord, true},
		{AppendVia("[discord] Message from a [id:1]:\nhi", ViaDiscord) + "\n", ViaDiscord, true},
		{"[via:discord]", ViaDiscord, true},
		{AppendVia("[user] hello", "mobile"), ViaDiscord, false},
		{"[user] hello", ViaDiscord, false},
		{"[user] the tag [via:discord] is in the middle\nof speech", ViaDiscord, false},
		{"", ViaDiscord, false},
		{"[via:discord]", "", false},
	}
	for _, tt := range tests {
		if got := HasVia(tt.msg, tt.source); got != tt.want {
			t.Errorf("HasVia(%q, %q) = %v, want %v", tt.msg, tt.source, got, tt.want)
		}
	}
}
