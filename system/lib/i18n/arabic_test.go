package i18n

import (
	"reflect"
	"strings"
	"testing"
	"unicode"
)

func TestArabicLanguageNormalization(t *testing.T) {
	for _, code := range []string{"ar", "ar-SA", "ar_EG", " AR-ae "} {
		if got := NormalizeLang(code); got != LangAR {
			t.Errorf("NormalizeLang(%q) = %q", code, got)
		}
	}
}

func TestArabicPhraseCoverage(t *testing.T) {
	hasArabic := func(text string) bool {
		return strings.ContainsFunc(text, func(r rune) bool { return unicode.Is(unicode.Arabic, r) })
	}
	for phrase, langs := range phrases {
		pool := langs[LangAR]
		if len(pool) == 0 {
			t.Errorf("%s has no Arabic variants", phrase)
		}
		for _, text := range pool {
			if !hasArabic(text) {
				t.Errorf("%s has non-Arabic variant %q", phrase, text)
			}
		}
		if !reflect.DeepEqual(poolFor(phrase, "ar-SA"), pool) {
			t.Errorf("%s regional Arabic falls back", phrase)
		}
	}
	for _, phrase := range ChitchatPhrases() {
		if len(InputPhrases(phrase)[LangAR]) == 0 {
			t.Errorf("%s has no Arabic inputs", phrase)
		}
	}
	for key := range toolFillers[LangEN] {
		if len(toolFillers[LangAR][key]) == 0 {
			t.Errorf("%s has no Arabic tool filler", key)
		}
		if !reflect.DeepEqual(FillerForTool("ar-SA", key), FillerForTool(LangAR, key)) {
			t.Errorf("%s regional filler falls back", key)
		}
	}
	for _, pool := range []func(string) []string{FillerOpening, FillerRealtime, FillerContinuation} {
		if !reflect.DeepEqual(pool("ar-SA"), pool(LangAR)) {
			t.Error("regional filler pool falls back")
		}
	}
}
