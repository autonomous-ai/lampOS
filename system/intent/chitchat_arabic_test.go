package intent

import (
	"net/http"
	"sync/atomic"
	"testing"

	"go.autonomous.ai/os/system/lib/i18n"
)

func TestArabicChitchatRequiresCompletePhrase(t *testing.T) {
	var emotionCalls atomic.Int32
	routeIntentHAL(t, func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodPost || r.URL.Path != "/emotion" {
			t.Errorf("unexpected HAL call: %s %s", r.Method, r.URL.Path)
		}
		emotionCalls.Add(1)
	})
	previousEnabled := chitchatEnabled()
	SetChitchatEnabled(true)
	t.Cleanup(func() { SetChitchatEnabled(previousEnabled) })
	previousWakeWords := i18n.ChitchatWakeWords()
	i18n.SetChitchatWakeWords([]string{"لومي"})
	t.Cleanup(func() { i18n.SetChitchatWakeWords(previousWakeWords) })

	for _, tc := range []struct {
		text string
		rule string
	}{
		{"هل انت هنا؟", "presence_check"},
		{"لومي، هل أنت هنا؟", "presence_check"},
		{"مرحباً!", "greeting"},
		{"السلام عليكم", "greeting"},
		{"شكراً", "thanks"},
		{"شكراً جزيلاً!", "thanks"},
		{"أنا آسف", "apology"},
		{"أحسنت", "compliment"},
		{"مع السلامة", "farewell"},
		{"إلى اللقاء", "farewell"},
		{"لا عليك", "nevermind"},
		{"شكراً، كم الساعة الآن؟", ""},
		{"مرحبا، شغل الموسيقى", ""},
		{"ما معنى شكرا بالإنجليزية؟", ""},
		{"أطفئ النور", ""},
	} {
		t.Run(tc.text, func(t *testing.T) {
			before := emotionCalls.Load()
			result := Match(tc.text)
			if tc.rule == "" {
				if result != nil {
					t.Errorf("Match(%q) = %+v, want agent fallback", tc.text, result)
				}
				if emotionCalls.Load() != before {
					t.Error("agent fallback must not send an emotion to HAL")
				}
				return
			}
			if result == nil || result.Rule != "chitchat_"+tc.rule || result.TTSText == "" || result.ExecutionFailed {
				t.Fatalf("Match(%q) = %+v, want successful chitchat_%s", tc.text, result, tc.rule)
			}
			if emotionCalls.Load() != before+1 {
				t.Error("matched chitchat must send exactly one emotion to HAL")
			}
		})
	}
}
