package device

import (
	"testing"

	"go.autonomous.ai/os/system/beclient"
	"go.autonomous.ai/os/system/server/config"
)

func plainDevice() *config.Config {
	return &config.Config{MQTTEndpoint: "b.example", MQTTPort: 1883, FAChannel: "P/f_a/1", FDChannel: "P/f_d/1"}
}

func reply(port string, tls *bool) *beclient.MQTTConfig {
	return &beclient.MQTTConfig{Endpoint: "b.example", Port: port, FaChannel: "P/f_a/1", FdChannel: "P/f_d/1", TLS: tls}
}

func TestPingTLSSwitchOn(t *testing.T) {
	on := true
	c := plainDevice()
	if !applyPingMQTT(reply("8883", &on), c) {
		t.Fatal("TLS switch should apply")
	}
	if !c.MQTTTLS || c.MQTTPort != 8883 {
		t.Fatalf("got tls=%v port=%d", c.MQTTTLS, c.MQTTPort)
	}
	if applyPingMQTT(reply("8883", &on), c) {
		t.Fatal("same reply again must not reconnect")
	}
}

func TestPingTLSSwitchOff(t *testing.T) {
	off := false
	c := plainDevice()
	c.MQTTTLS, c.MQTTPort = true, 8883
	if !applyPingMQTT(reply("1883", &off), c) {
		t.Fatal("TLS off should apply")
	}
	if c.MQTTTLS || c.MQTTPort != 1883 {
		t.Fatalf("got tls=%v port=%d", c.MQTTTLS, c.MQTTPort)
	}
}

func TestPingWithoutTLSFieldKeepsSetting(t *testing.T) {
	c := plainDevice()
	c.MQTTTLS, c.MQTTPort = true, 8883
	if applyPingMQTT(reply("8883", nil), c) || !c.MQTTTLS {
		t.Fatal("a reply without the TLS field must leave TLS alone")
	}
}
