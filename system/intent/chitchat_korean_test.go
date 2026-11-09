package intent

import (
	"net/http"
	"sync/atomic"
	"testing"

	"go.autonomous.ai/os/system/lib/i18n"
)

func TestKoreanChitchatRequiresCompletePhrase(t *testing.T) {
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
	i18n.SetChitchatWakeWords([]string{"루미"})
	t.Cleanup(func() { i18n.SetChitchatWakeWords(previousWakeWords) })

	for _, tc := range []struct {
		text string
		rule string
	}{
		{"거기 있어?", "presence_check"},
		{"루미, 거기 있어?", "presence_check"},
		{"안녕하세요!", "greeting"},
		{"고마워", "thanks"},
		{"미안해", "apology"},
		{"잘했어", "compliment"},
		{"잘 가", "farewell"},
		{"신경 쓰지 마", "nevermind"},
		{"미안한데 지금 몇 시야?", ""},
		{"고마워라는 말 영어로 뭐야?", ""},
		{"안녕이라는 노래 알아?", ""},
		{"불 켜 줘", ""},
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
