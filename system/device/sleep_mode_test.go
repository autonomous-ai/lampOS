package device

import (
	"testing"

	"go.autonomous.ai/os/system/domain"
)

func TestSleepModeDefaultsOffAndToggles(t *testing.T) {
	c := baseConfig()
	if c.SleepModeEnabled() {
		t.Fatal("sleep mode defaults to off")
	}
	on, off := true, false
	ch := applyUpdate(c, domain.UpdateConfigRequest{SleepMode: &on}, "")
	if !c.SleepModeEnabled() || ch.halBoot || ch.sleepOff {
		t.Fatalf("on must persist without a HAL restart or wake: enabled=%v halBoot=%v sleepOff=%v", c.SleepModeEnabled(), ch.halBoot, ch.sleepOff)
	}
	if applyUpdate(c, domain.UpdateConfigRequest{TTSVoice: "x"}, ""); !c.SleepModeEnabled() {
		t.Fatal("a request without sleep_mode must keep it")
	}
	s := &Service{config: c}
	if !s.GetPublicConfig().SleepMode {
		t.Fatal("public config must report the saved toggle")
	}
	ch = applyUpdate(c, domain.UpdateConfigRequest{SleepMode: &off}, "")
	if c.SleepModeEnabled() || !ch.sleepOff || ch.halBoot {
		t.Fatalf("off must persist and wake HAL live: enabled=%v sleepOff=%v halBoot=%v", c.SleepModeEnabled(), ch.sleepOff, ch.halBoot)
	}
}
