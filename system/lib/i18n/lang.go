// Package i18n exposes the active STT language and short localized TTS phrases (see Pick/One).
package i18n

import (
	"strings"
	"sync/atomic"

	"go.autonomous.ai/os/system/server/config"
)

// BCP-47 language codes; LangZh/LangZhHans/LangZhHant normalise to LangZhCN/LangZhTW.
const (
	LangEN     = "en"
	LangJA     = "ja"
	LangKO     = "ko"
	LangVI     = "vi"
	LangZhCN   = "zh-CN"
	LangZhTW   = "zh-TW"
	LangZh     = "zh"
	LangZhHans = "zh-Hans"
	LangZhHant = "zh-Hant"
)

// active is atomic because SetConfig and Lang may run on different goroutines.
var active atomic.Pointer[config.Config]

// SetConfig wires the live config so Lang returns the current setting.
func SetConfig(cfg *config.Config) {
	active.Store(cfg)
}

// Lang returns the active STT language code (e.g. "vi"); "" before SetConfig means English.
func Lang() string {
	cfg := active.Load()
	if cfg == nil {
		return ""
	}
	return NormalizeLang(cfg.STTLanguage)
}

// LangContextTag returns "\n[context: current_language=X]" when configured, else "".
func LangContextTag() string {
	if l := Lang(); l != "" {
		return "\n[context: current_language=" + l + "]"
	}
	return ""
}

// NormalizeLang maps Japanese and Korean regional codes to their shared phrase pools.
func NormalizeLang(lang string) string {
	normalized := strings.ToLower(strings.TrimSpace(lang))
	for _, base := range []string{LangJA, LangKO} {
		if normalized == base || strings.HasPrefix(normalized, base+"-") || strings.HasPrefix(normalized, base+"_") {
			return base
		}
	}
	return lang
}
