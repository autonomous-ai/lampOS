package device

import (
	"testing"

	"go.autonomous.ai/os/system/beclient"
	"go.autonomous.ai/os/system/server/config"
)

func TestMQTTConfigChanged(t *testing.T) {
	cur := &config.Config{MQTTEndpoint: "b.example", MQTTPort: 1883, FAChannel: "P/f_a/1", FDChannel: "P/f_d/1"}
	same := beclient.MQTTConfig{Endpoint: "b.example", Port: "1883", FaChannel: "P/f_a/1", FdChannel: "P/f_d/1"}
	cases := map[string]struct {
		mod  func(m *beclient.MQTTConfig)
		want bool
	}{
		"unchanged":      {func(m *beclient.MQTTConfig) {}, false},
		"tls port only":  {func(m *beclient.MQTTConfig) { m.Port = "8883" }, true},
		"new broker":     {func(m *beclient.MQTTConfig) { m.Endpoint = "c.example" }, true},
		"new channels":   {func(m *beclient.MQTTConfig) { m.FaChannel = "Q/f_a/1"; m.FdChannel = "Q/f_d/1" }, true},
		"empty port":     {func(m *beclient.MQTTConfig) { m.Port = "" }, false},
		"empty channels": {func(m *beclient.MQTTConfig) { m.FaChannel, m.FdChannel = "", "" }, false},
	}
	for name, tc := range cases {
		m := same
		tc.mod(&m)
		if got := mqttConfigChanged(&m, cur); got != tc.want {
			t.Errorf("%s: got %v, want %v", name, got, tc.want)
		}
	}
}
