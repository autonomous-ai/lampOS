package bootstrap

import (
	"strings"
	"testing"
)

func TestOTAUpdateStartPhraseKorean(t *testing.T) {
	for _, lang := range []string{"ko", "ko-KR", "ko_KR", " KO-kr "} {
		if got := otaUpdateStartPhrase(lang); !strings.Contains(got, "업데이트") {
			t.Errorf("otaUpdateStartPhrase(%q) = %q", lang, got)
		}
	}
}
