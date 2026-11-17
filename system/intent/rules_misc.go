package intent

import (
	"time"

	"go.autonomous.ai/os/system/lib/i18n"
)

var miscRules = []rule{
	{
		name:  "what_time",
		match: anyOf("what time", "whats the time", "what's the time"),
		exec: func(string) *Result {
			// Answer in the device language; jev routes non-English questions here too.
			return &Result{TTSText: i18n.TimeOfDay(time.Now()), Actions: []string{"time.Now()"}}
		},
	},
}
