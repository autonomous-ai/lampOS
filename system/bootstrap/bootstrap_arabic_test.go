package bootstrap

import (
	"strings"
	"testing"
)

func TestOTAUpdateStartPhraseArabic(t *testing.T) {
	for _, lang := range []string{"ar", "ar-SA", "ar_EG", " AR-ae "} {
		if got := otaUpdateStartPhrase(lang); !strings.Contains(got, "تحديث") {
			t.Errorf("otaUpdateStartPhrase(%q) = %q", lang, got)
		}
	}
}
