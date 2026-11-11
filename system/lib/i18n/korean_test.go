package i18n

import (
	"reflect"
	"strings"
	"testing"
	"unicode"
)

func TestKoreanLanguageNormalization(t *testing.T) {
	for _, code := range []string{"ko", "ko-KR", "ko_KR", " KO-kr "} {
		if got := NormalizeLang(code); got != LangKO {
			t.Errorf("NormalizeLang(%q) = %q", code, got)
		}
	}
}

func TestKoreanPhraseCoverage(t *testing.T) {
	hasHangul := func(text string) bool {
		return strings.ContainsFunc(text, func(r rune) bool { return unicode.Is(unicode.Hangul, r) })
	}
	for phrase, langs := range phrases {
		pool := langs[LangKO]
		if len(pool) == 0 {
			t.Errorf("%s has no Korean variants", phrase)
		}
		for _, text := range pool {
			if !hasHangul(text) {
				t.Errorf("%s has non-Korean variant %q", phrase, text)
			}
		}
		if !reflect.DeepEqual(poolFor(phrase, "ko-KR"), pool) {
			t.Errorf("%s regional Korean falls back", phrase)
		}
	}
	for _, phrase := range ChitchatPhrases() {
		if len(InputPhrases(phrase)[LangKO]) == 0 {
			t.Errorf("%s has no Korean inputs", phrase)
		}
	}
	for key := range toolFillers[LangEN] {
		if len(toolFillers[LangKO][key]) == 0 {
			t.Errorf("%s has no Korean tool filler", key)
		}
		if !reflect.DeepEqual(FillerForTool("ko-KR", key), FillerForTool(LangKO, key)) {
			t.Errorf("%s regional filler falls back", key)
		}
	}
	for _, pool := range []func(string) []string{FillerOpening, FillerRealtime, FillerContinuation} {
		if !reflect.DeepEqual(pool("ko-KR"), pool(LangKO)) {
			t.Error("regional filler pool falls back")
		}
	}
}
