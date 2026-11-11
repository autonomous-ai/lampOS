package config

import (
	"encoding/json"
	"strings"
	"testing"
)

func TestMQTTTLSEnabled(t *testing.T) {
	tests := []struct {
		name string
		env  string
		cfg  bool
		want bool
	}{
		{"default off", "", false, false},
		{"config on", "", true, true},
		{"env 1", "1", false, true},
		{"env true", "true", false, true},
		{"env on mixed case", " On ", false, true},
		{"env 0 does not override config", "0", true, true},
		{"env junk is off", "maybe", false, false},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Setenv("MQTT_TLS", tt.env)
			c := &Config{MQTTTLS: tt.cfg}
			if got := c.MQTTTLSEnabled(); got != tt.want {
				t.Fatalf("MQTTTLSEnabled = %t, want %t", got, tt.want)
			}
		})
	}
}

func TestProvideMQTTConfigTLS(t *testing.T) {
	t.Setenv("MQTT_TLS", "")
	if ProvideMQTTConfig(&Config{MQTTEndpoint: "b", MQTTPort: 8883}).TLS {
		t.Fatal("TLS must be off by default")
	}
	if !ProvideMQTTConfig(&Config{MQTTEndpoint: "b", MQTTPort: 8883, MQTTTLS: true}).TLS {
		t.Fatal("mqtt_tls must enable TLS")
	}
	t.Setenv("MQTT_TLS", "true")
	if !ProvideMQTTConfig(&Config{MQTTEndpoint: "b"}).TLS {
		t.Fatal("MQTT_TLS=true must enable TLS")
	}
}

func TestMQTTTLSJSONKeyOmittedWhenOff(t *testing.T) {
	// Saved config.json stays unchanged for devices that never turn the switch on.
	data, err := json.Marshal(&Config{})
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(data), "mqtt_tls") {
		t.Fatalf("mqtt_tls must be omitted when off: %s", data)
	}
}
