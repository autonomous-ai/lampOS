package config

import (
	"fmt"
	"slices"
	"strings"
	"unicode"
	"unicode/utf8"
)

const (
	// MaxWakePhrases caps the owner list; every phrase is checked on each STT partial.
	MaxWakePhrases = 30
	// MaxWakePhraseRunes keeps a phrase short enough to say in one breath.
	MaxWakePhraseRunes = 40

	// Listening window after a wake; keep aligned with hal/config.py (WAKE_FOLLOWUP_*).
	WakeFollowupDefaultS = 10
	WakeFollowupMinS     = 3
	WakeFollowupMaxS     = 30
)

// ValidateWakeFollowup rejects a listening window outside [WakeFollowupMinS, WakeFollowupMaxS].
func ValidateWakeFollowup(seconds int) error {
	if seconds < WakeFollowupMinS || seconds > WakeFollowupMaxS {
		return fmt.Errorf("wake_followup_s must be between %d and %d seconds", WakeFollowupMinS, WakeFollowupMaxS)
	}
	return nil
}

// NormalizeWakePhrases lowercases, collapses spaces and dedupes owner wake phrases.
// Blank entries are dropped; an empty result means "use the built-in list".
func NormalizeWakePhrases(in []string) ([]string, error) {
	out := make([]string, 0, len(in))
	seen := make(map[string]struct{}, len(in))
	for _, raw := range in {
		p := strings.ToLower(strings.Join(strings.Fields(raw), " "))
		if p == "" {
			continue
		}
		if utf8.RuneCountInString(p) > MaxWakePhraseRunes {
			return nil, fmt.Errorf("wake phrase %q is longer than %d characters", p, MaxWakePhraseRunes)
		}
		if strings.IndexFunc(p, unicode.IsLetter) < 0 {
			return nil, fmt.Errorf("wake phrase %q has no letters", p)
		}
		if _, dup := seen[p]; dup {
			continue
		}
		seen[p] = struct{}{}
		out = append(out, p)
	}
	if len(out) > MaxWakePhrases {
		return nil, fmt.Errorf("at most %d wake phrases", MaxWakePhrases)
	}
	return out, nil
}

// WakeLight is the listening light shown on a wake (config.json `wake_light`).
// Zero fields keep the board's listening look; HAL reads it live.
type WakeLight struct {
	Enabled *bool   `json:"enabled,omitempty" yaml:"enabled"`
	Color   []int   `json:"color,omitempty" yaml:"color"`
	Effect  string  `json:"effect,omitempty" yaml:"effect"`
	Speed   float64 `json:"speed,omitempty" yaml:"speed"`
}

// WakeLightEffects are the owner-facing effects; keep aligned with hal/wake_light.py.
var WakeLightEffects = []string{"solid", "breathing", "spin", "blink"}

// WakeLightDefault is the board's `listening` look in hal/presets.py, shown until the owner picks another.
var WakeLightDefault = WakeLight{Color: []int{4, 8, 16}, Effect: "breathing", Speed: 1.2}

const (
	WakeLightSpeedMin = 0.25
	WakeLightSpeedMax = 3.0
)

// ErrNoWakeCue rejects turning off both the wake chime and the wake light.
var ErrNoWakeCue = fmt.Errorf("keep the wake chime or the wake light on, so a wake is noticeable")

// ErrNoTapCue rejects turning off both the tap-to-talk chime and light.
var ErrNoTapCue = fmt.Errorf("keep the tap-to-talk chime or light on, so you can tell recording started")

// Validate checks the effect, speed and colour; zero values mean "board default".
func (w *WakeLight) Validate() error {
	if w.Effect != "" && !slices.Contains(WakeLightEffects, w.Effect) {
		return fmt.Errorf("wake_light.effect must be one of %v", WakeLightEffects)
	}
	if w.Speed != 0 && (w.Speed < WakeLightSpeedMin || w.Speed > WakeLightSpeedMax) {
		return fmt.Errorf("wake_light.speed must be between %.2f and %.1f", WakeLightSpeedMin, WakeLightSpeedMax)
	}
	if w.Color != nil {
		if len(w.Color) != 3 {
			return fmt.Errorf("wake_light.color must be [r, g, b]")
		}
		lit := false
		for _, c := range w.Color {
			if c < 0 || c > 255 {
				return fmt.Errorf("wake_light.color channels must be 0-255")
			}
			lit = lit || c > 0
		}
		if !lit {
			return fmt.Errorf("wake_light.color must not be black; turn the wake light off instead")
		}
	}
	return nil
}
