package device

import (
	"reflect"
	"testing"

	"go.autonomous.ai/os/system/domain"
	"go.autonomous.ai/os/system/lib/i18n"
	"go.autonomous.ai/os/system/server/config"
)

func TestApplyUpdateSetsAndClearsOwnerWakePhrases(t *testing.T) {
	c := baseConfig()
	phrases := []string{"ê bé na ơi", "hey sam"}

	ch := applyUpdate(c, domain.UpdateConfigRequest{WakePhrases: &phrases}, "")
	if !ch.wake || !reflect.DeepEqual(c.WakePhrases, phrases) {
		t.Fatalf("set: wake=%v phrases=%v", ch.wake, c.WakePhrases)
	}
	if ch.halBoot {
		t.Fatal("a phrase edit is pushed live, it must not restart HAL")
	}

	if ch := applyUpdate(c, domain.UpdateConfigRequest{WakePhrases: &phrases}, ""); ch.wake {
		t.Fatal("re-saving the same phrases must not report a change")
	}
	if ch := applyUpdate(c, domain.UpdateConfigRequest{TTSVoice: "x"}, ""); ch.wake || c.WakePhrases == nil {
		t.Fatal("a request without wake_phrases must keep them")
	}

	empty := []string{}
	if ch := applyUpdate(c, domain.UpdateConfigRequest{WakePhrases: &empty}, ""); !ch.wake || c.WakePhrases != nil {
		t.Fatalf("[] must restore the built-in list: wake=%v phrases=%v", ch.wake, c.WakePhrases)
	}
}

func TestPublicConfigReportsOwnerWakePhrases(t *testing.T) {
	i18n.SetDeviceName("Luna")
	t.Cleanup(func() { i18n.SetDeviceName("autonomous") })

	s := &Service{config: &config.Config{DeviceType: "intern-v2", WakePhrases: []string{"ê bé na ơi"}}}
	cfg := s.GetPublicConfig()
	if !cfg.WakePhrasesCustom || !reflect.DeepEqual(cfg.WakePhrases, []string{"ê bé na ơi"}) {
		t.Fatalf("custom=%v phrases=%v", cfg.WakePhrasesCustom, cfg.WakePhrases)
	}
	// Intern's built-in list is names, not greeting phrases.
	names := []string{"autonomous", "intern", "luna"}
	if !reflect.DeepEqual(cfg.WakePhrasesDefault, names) {
		t.Fatalf("defaults must stay available for restore: %v", cfg.WakePhrasesDefault)
	}

	s.config.WakePhrases = nil
	if cfg := s.GetPublicConfig(); cfg.WakePhrasesCustom || !reflect.DeepEqual(cfg.WakePhrases, names) {
		t.Fatalf("built-in: custom=%v phrases=%v", cfg.WakePhrasesCustom, cfg.WakePhrases)
	}
}

func TestWakeChimeDefaultsOnAndToggles(t *testing.T) {
	c := baseConfig()
	if !c.WakeChimeEnabled() {
		t.Fatal("the wake chime defaults to on")
	}
	off := false
	ch := applyUpdate(c, domain.UpdateConfigRequest{WakeChime: &off}, "")
	if c.WakeChimeEnabled() || ch.halBoot {
		t.Fatalf("off must persist without a HAL restart: enabled=%v halBoot=%v", c.WakeChimeEnabled(), ch.halBoot)
	}
	if applyUpdate(c, domain.UpdateConfigRequest{TTSVoice: "x"}, ""); c.WakeChimeEnabled() {
		t.Fatal("a request without wake_chime must keep it off")
	}
	s := &Service{config: c}
	if s.GetPublicConfig().WakeChime {
		t.Fatal("public config must report the saved toggle")
	}
}

func TestWakeFollowupDefaultsAndPersists(t *testing.T) {
	c := baseConfig()
	if got := c.WakeFollowupSeconds(); got != config.WakeFollowupDefaultS {
		t.Fatalf("default window = %d, want %d", got, config.WakeFollowupDefaultS)
	}
	fifteen := 15
	ch := applyUpdate(c, domain.UpdateConfigRequest{WakeFollowupS: &fifteen}, "")
	if c.WakeFollowupSeconds() != 15 || ch.halBoot {
		t.Fatalf("window must persist without a HAL restart: got=%d halBoot=%v", c.WakeFollowupSeconds(), ch.halBoot)
	}
	applyUpdate(c, domain.UpdateConfigRequest{TTSVoice: "x"}, "")
	s := &Service{config: c}
	if got := s.GetPublicConfig().WakeFollowupS; got == nil || *got != 15 {
		t.Fatalf("public window = %v, want 15", got)
	}
	if (&Service{config: baseConfig()}).GetPublicConfig().WakeFollowupS != nil {
		t.Fatal("an unset window must stay unset so the UI shows HAL's image default")
	}
}

func TestWakeNeedsTheChimeOrTheLight(t *testing.T) {
	off, on := false, true
	c := baseConfig()
	if !keepsWakeCue(c, domain.UpdateConfigRequest{WakeChime: &off}) {
		t.Fatal("chime off is fine while the light is on")
	}
	if keepsWakeCue(c, domain.UpdateConfigRequest{WakeChime: &off, WakeLight: &config.WakeLight{Enabled: &off}}) {
		t.Fatal("turning both off must be rejected")
	}
	applyUpdate(c, domain.UpdateConfigRequest{WakeLight: &config.WakeLight{Enabled: &off}}, "")
	if c.WakeLightEnabled() {
		t.Fatal("light off must persist")
	}
	if keepsWakeCue(c, domain.UpdateConfigRequest{WakeChime: &off}) {
		t.Fatal("chime off with the saved light off must be rejected")
	}
	if !keepsWakeCue(c, domain.UpdateConfigRequest{WakeChime: &off, WakeLight: &config.WakeLight{Enabled: &on}}) {
		t.Fatal("swapping chime for light in one request is fine")
	}
}

func TestTapToTalkNeedsTheChimeOrTheLight(t *testing.T) {
	off, on := false, true
	c := baseConfig()
	if !c.TapChimeEnabled() || !c.TapLightEnabled() {
		t.Fatal("tap-to-talk cues default on")
	}
	if !keepsTapCue(c, domain.UpdateConfigRequest{TapChime: &off}) {
		t.Fatal("chime off is fine while the light is on")
	}
	if keepsTapCue(c, domain.UpdateConfigRequest{TapChime: &off, TapLight: &config.WakeLight{Enabled: &off}}) {
		t.Fatal("turning both off must be rejected")
	}
	look := &config.WakeLight{Enabled: &off, Color: []int{40, 0, 20}, Effect: "blink", Speed: 2}
	applyUpdate(c, domain.UpdateConfigRequest{TapLight: look}, "")
	look.Color[0] = 1
	if c.TapLightEnabled() || c.TapLight.Color[0] != 40 || c.TapLight.Effect != "blink" {
		t.Fatalf("tap light must persist as its own copy, got %+v", c.TapLight)
	}
	if !c.WakeLightEnabled() || c.WakeLight != nil {
		t.Fatal("the tap light must not change the wake light")
	}
	if keepsTapCue(c, domain.UpdateConfigRequest{TapChime: &off}) {
		t.Fatal("chime off with the saved light off must be rejected")
	}
	if !keepsTapCue(c, domain.UpdateConfigRequest{TapChime: &off, TapLight: &config.WakeLight{Enabled: &on}}) {
		t.Fatal("swapping chime for light in one request is fine")
	}
	pub := (&Service{config: c}).GetPublicConfig()
	if !pub.TapChime || pub.TapLightEnabled || pub.TapLight == nil || pub.TapLight.Effect != "blink" {
		t.Fatalf("public tap cues = %v %v %+v", pub.TapChime, pub.TapLightEnabled, pub.TapLight)
	}
}

func TestWakeLightPersistsAndIsPublic(t *testing.T) {
	c := baseConfig()
	look := &config.WakeLight{Color: []int{0, 20, 40}, Effect: "spin", Speed: 1.5}
	if ch := applyUpdate(c, domain.UpdateConfigRequest{WakeLight: look}, ""); ch.halBoot {
		t.Fatal("the wake light is read live; no HAL restart")
	}
	look.Color[0] = 99 // the saved copy must not alias the request
	pub := (&Service{config: c}).GetPublicConfig()
	if !pub.WakeLightEnabled || pub.WakeLight == nil || pub.WakeLight.Effect != "spin" || pub.WakeLight.Color[0] != 0 {
		t.Fatalf("public wake light = %+v enabled=%v", pub.WakeLight, pub.WakeLightEnabled)
	}
}
