package discordrelay

import (
	"encoding/base64"
	"errors"
	"testing"
	"time"
)

const testDeviceKey = "lob_test_device_key"

func TestSealOpenRoundTrip(t *testing.T) {
	now := time.Unix(1_800_000_000, 0)
	s, err := Seal(testDeviceKey, "discord_reply", "dev1", map[string]any{"discord_channel_id": "c1", "text": "hi"}, now)
	if err != nil {
		t.Fatal(err)
	}
	var got struct {
		ChannelID string `json:"discord_channel_id"`
		Text      string `json:"text"`
	}
	if err := Open(testDeviceKey, "discord_reply", "dev1", SealVersion, s, &got, now.Add(time.Minute)); err != nil {
		t.Fatal(err)
	}
	if got.ChannelID != "c1" || got.Text != "hi" {
		t.Fatalf("got %+v", got)
	}
}

func TestOpenRefuses(t *testing.T) {
	now := time.Unix(1_800_000_000, 0)
	s, err := Seal(testDeviceKey, "discord_event", "dev1", map[string]any{"text": "hi"}, now)
	if err != nil {
		t.Fatal(err)
	}
	raw, _ := base64.StdEncoding.DecodeString(s)
	raw[len(raw)-1] ^= 1
	tampered := base64.StdEncoding.EncodeToString(raw)
	var dst map[string]any
	cases := []struct {
		name, key, kind, dev, enc, sealed string
		at                                time.Time
		want                              error
	}{
		{"another device's key", "lob_other", "discord_event", "dev1", "v1", s, now, ErrSealOpen},
		{"sealed for another device", testDeviceKey, "discord_event", "dev2", "v1", s, now, ErrSealOpen},
		{"another kind", testDeviceKey, "discord_reply", "dev1", "v1", s, now, ErrSealOpen},
		{"too old", testDeviceKey, "discord_event", "dev1", "v1", s, now.Add(16 * time.Minute), ErrSealStale},
		{"from the future", testDeviceKey, "discord_event", "dev1", "v1", s, now.Add(-16 * time.Minute), ErrSealStale},
		{"unknown version", testDeviceKey, "discord_event", "dev1", "v9", s, now, ErrSealVersion},
		{"no key", "", "discord_event", "dev1", "v1", s, now, ErrSealNoKey},
		{"tampered", testDeviceKey, "discord_event", "dev1", "v1", tampered, now, ErrSealOpen},
		{"truncated", testDeviceKey, "discord_event", "dev1", "v1", "AAAA", now, ErrSealOpen},
	}
	for _, c := range cases {
		if err := Open(c.key, c.kind, c.dev, c.enc, c.sealed, &dst, c.at); !errors.Is(err, c.want) {
			t.Errorf("%s: got %v, want %v", c.name, err, c.want)
		}
	}
}

// cloudSealedEvent is a discord_event sealed by the cloud side of the relay for
// testDeviceKey, device "dev1", at 1800000000. The device must keep opening it.
const cloudSealedEvent = "o1KyXwQU2A1HMv6TPx+hWxnj2UQNp43JVsvedY8uyUeZA4lYkn45Px1vuF+VVKXWCtCKlPtzPWsJVQfmH+Mbg/ychuFQW2OdStvVjvx5HelhQWB9rdlHbCwMrbbGbxlO6zaAhHNH1QMweXfbVLMKXHyvsuF/Wi+iMUtL79v0dyj9qjEYXvhnK7cra+E7weFwuM80ip4gMMnQbyxu9mR6NpZFythXHLoPU+U="

func TestOpensCloudSealedEvent(t *testing.T) {
	var got struct {
		EventID   string `json:"event_id"`
		UserID    string `json:"discord_user_id"`
		ChannelID string `json:"discord_channel_id"`
		Text      string `json:"text"`
	}
	if err := Open(testDeviceKey, "discord_event", "dev1", SealVersion, cloudSealedEvent, &got, time.Unix(1_800_000_000, 0)); err != nil {
		t.Fatal(err)
	}
	if got.EventID != "m1" || got.UserID != "owner-1" || got.ChannelID != "c1" || got.Text != "hello from the cloud" {
		t.Fatalf("got %+v", got)
	}
}
