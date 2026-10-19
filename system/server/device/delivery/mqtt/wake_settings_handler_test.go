package mqtthandler

import (
	"encoding/json"
	"net/http"
	"path/filepath"
	"reflect"
	"testing"

	"go.autonomous.ai/os/system/domain"
)

// wakeSettingsTest wires a handler with a real device service to a fake HAL.
func wakeSettingsTest(t *testing.T) (*DeviceMQTTHandler, *voiceHAL, func(kind, data string), func(kind, status string) map[string]json.RawMessage) {
	t.Helper()
	h, messages := infoTestHandler(t, filepath.Join(t.TempDir(), "schedules.json"))
	stub := &voiceHAL{replies: map[string]voiceHALReply{}, calls: make(chan voiceHALCall, 64)}
	original := http.DefaultTransport
	t.Cleanup(func() { http.DefaultTransport = original })
	http.DefaultTransport = stub
	send := func(kind, data string) {
		t.Helper()
		_ = h.dispatchData(domain.MQTTDataCommand{Kind: kind, Data: json.RawMessage(data)})
	}
	read := func(kind, status string) map[string]json.RawMessage {
		t.Helper()
		msg := nextPublish(t, messages)
		if string(msg["kind"]) != `"`+kind+`"` || string(msg["status"]) != `"`+status+`"` {
			t.Fatalf("reply = kind %s status %s error %s; want %s %s", msg["kind"], msg["status"], msg["error"], kind, status)
		}
		return msg
	}
	return h, stub, send, read
}

func decodeWakeSettings(t *testing.T, msg map[string]json.RawMessage) domain.WakeSettings {
	t.Helper()
	var s domain.WakeSettings
	if err := json.Unmarshal(msg["data"], &s); err != nil {
		t.Fatal(err)
	}
	return s
}

func TestWakeSettingsGetReportsDefaultsAndHALFollowup(t *testing.T) {
	_, stub, send, read := wakeSettingsTest(t)
	stub.set("/voice/wake-followup", http.StatusOK, `{"seconds":5,"image_default_s":5}`)

	send(domain.KindWakeSettingsGet, `{}`)
	s := decodeWakeSettings(t, read(domain.KindWakeSettingsGet, "success"))
	if s.PhrasesCustom || len(s.Phrases) == 0 || !reflect.DeepEqual(s.Phrases, s.PhrasesDefault) {
		t.Fatalf("phrases = %v custom=%v default=%v", s.Phrases, s.PhrasesCustom, s.PhrasesDefault)
	}
	if !s.Chime || !s.Light.Enabled || !reflect.DeepEqual(s.Light.Color, []int{4, 8, 16}) || s.Light.Effect != "breathing" || s.Light.Speed != 1.2 {
		t.Fatalf("cues = chime %v light %+v", s.Chime, s.Light)
	}
	if s.FollowupS != 5 || s.FollowupCustom || s.FollowupMinS != 3 || s.FollowupMaxS != 30 {
		t.Fatalf("followup = %d custom=%v [%d,%d]", s.FollowupS, s.FollowupCustom, s.FollowupMinS, s.FollowupMaxS)
	}
	if !s.TapChime || !reflect.DeepEqual(s.TapLight, s.Light) {
		t.Fatalf("tap cues = chime %v light %+v", s.TapChime, s.TapLight)
	}
	if s.MaxPhrases != 30 || s.MaxPhraseChars != 40 || len(s.LightEffects) != 4 || s.VoiceInputMode != "automatic" {
		t.Fatalf("limits = %+v", s)
	}
}

func TestWakeSettingsGetFallsBackWhenHALIsDown(t *testing.T) {
	_, _, send, read := wakeSettingsTest(t)
	send(domain.KindWakeSettingsGet, `{}`)
	if s := decodeWakeSettings(t, read(domain.KindWakeSettingsGet, "success")); s.FollowupS != 10 || s.FollowupCustom {
		t.Fatalf("followup = %d custom=%v", s.FollowupS, s.FollowupCustom)
	}
}

func TestWakeSettingsSetSavesAndRepliesWithSettings(t *testing.T) {
	h, _, send, read := wakeSettingsTest(t)
	send(domain.KindWakeSettingsSet, `{"phrases":["  Hey  SAM ","sam"],"chime":false,"followup_s":15,
		"light":{"enabled":true,"color":[16,9,3],"effect":"spin","speed":2}}`)
	s := decodeWakeSettings(t, read(domain.KindWakeSettingsSet, "success"))
	if !reflect.DeepEqual(s.Phrases, []string{"hey sam", "sam"}) || !s.PhrasesCustom {
		t.Fatalf("phrases = %v custom=%v", s.Phrases, s.PhrasesCustom)
	}
	if s.Chime || s.FollowupS != 15 || !s.FollowupCustom || s.Applying {
		t.Fatalf("settings = %+v", s)
	}
	if want := (domain.WakeLightLook{Enabled: true, Color: []int{16, 9, 3}, Effect: "spin", Speed: 2}); !reflect.DeepEqual(s.Light, want) {
		t.Fatalf("light = %+v", s.Light)
	}
	if h.config.WakeChime == nil || *h.config.WakeChime || *h.config.WakeFollowupS != 15 {
		t.Fatal("settings not saved to config")
	}

	send(domain.KindWakeSettingsSet, `{"phrases":[]}`)
	if s := decodeWakeSettings(t, read(domain.KindWakeSettingsSet, "success")); s.PhrasesCustom || h.config.WakePhrases != nil {
		t.Fatalf("[] must restore the built-in list: %v", s.Phrases)
	}
}

func TestWakeSettingsSetSavesTapCues(t *testing.T) {
	h, _, send, read := wakeSettingsTest(t)
	send(domain.KindWakeSettingsSet, `{"tap_chime":false,"tap_light":{"enabled":true,"color":[3,16,9],"effect":"blink","speed":0.5}}`)
	s := decodeWakeSettings(t, read(domain.KindWakeSettingsSet, "success"))
	if want := (domain.WakeLightLook{Enabled: true, Color: []int{3, 16, 9}, Effect: "blink", Speed: 0.5}); s.TapChime || !reflect.DeepEqual(s.TapLight, want) {
		t.Fatalf("tap cues = chime %v light %+v", s.TapChime, s.TapLight)
	}
	if !s.Chime || !reflect.DeepEqual(s.Light.Color, []int{4, 8, 16}) {
		t.Fatalf("tap cues changed the wake cues: chime %v light %+v", s.Chime, s.Light)
	}
	if h.config.TapChime == nil || *h.config.TapChime || h.config.TapLight == nil || h.config.WakeChime != nil || h.config.WakeLight != nil {
		t.Fatal("tap cues not saved apart from the wake cues")
	}
}

func TestWakeSettingsSetRejectsBadInputWithoutSaving(t *testing.T) {
	h, _, send, read := wakeSettingsTest(t)
	for _, data := range []string{
		`not json`,
		`{}`,
		`{"chime":false,"light":{"enabled":false}}`,
		`{"followup_s":2}`,
		`{"light":{"effect":"rainbow"}}`,
		`{"light":{"color":[0,0,0]}}`,
		`{"phrases":["123"]}`,
		`{"tap_chime":false,"tap_light":{"enabled":false}}`,
		`{"tap_light":{"effect":"rainbow"}}`,
		`{"tap_light":{"color":[0,0,0]}}`,
	} {
		send(domain.KindWakeSettingsSet, data)
		read(domain.KindWakeSettingsSet, "failure")
	}
	if h.config.WakeChime != nil || h.config.WakeFollowupS != nil || h.config.WakeLight != nil || h.config.WakePhrases != nil ||
		h.config.TapChime != nil || h.config.TapLight != nil {
		t.Fatal("rejected payload changed the config")
	}
}

func TestWakeLightPreviewFillsDefaultLook(t *testing.T) {
	_, stub, send, read := wakeSettingsTest(t)
	stub.set("/led/wake-light/preview", http.StatusOK, `{"status":"ok","painted":true}`)

	send(domain.KindWakeLightPreview, `{"color":[10,4,16]}`)
	if msg := read(domain.KindWakeLightPreview, "success"); string(msg["data"]) != `{"status":"ok","painted":true}` {
		t.Fatalf("data = %s", msg["data"])
	}
	c := halCall(t, stub)
	want := map[string]any{"color": []any{10.0, 4.0, 16.0}, "effect": "breathing", "speed": 1.2}
	if c.path != "/led/wake-light/preview" || !reflect.DeepEqual(c.body, want) {
		t.Fatalf("HAL call = %+v", c)
	}
}

func TestWakeLightPreviewRejectsBadLookWithoutCallingHAL(t *testing.T) {
	_, stub, send, read := wakeSettingsTest(t)
	for _, data := range []string{`{}`, `{"color":[0,0,0]}`, `{"color":[1,2]}`, `{"color":[1,2,3],"speed":9}`, `nope`} {
		send(domain.KindWakeLightPreview, data)
		read(domain.KindWakeLightPreview, "failure")
	}
	stub.noCall(t)
}
