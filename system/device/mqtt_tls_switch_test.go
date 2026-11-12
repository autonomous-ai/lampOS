package device

import (
	"testing"
	"time"

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
	if !applyPingMQTT(reply("8883", &on), c, time.Now()) {
		t.Fatal("TLS switch should apply")
	}
	if !c.MQTTTLS || c.MQTTPort != 8883 || c.MQTTPlainPort != 1883 {
		t.Fatalf("got tls=%v port=%d plain=%d", c.MQTTTLS, c.MQTTPort, c.MQTTPlainPort)
	}
	if applyPingMQTT(reply("8883", &on), c, time.Now()) {
		t.Fatal("same reply again must not reconnect")
	}
}

func TestPingTLSSwitchOff(t *testing.T) {
	off := false
	c := plainDevice()
	c.MQTTTLS, c.MQTTPort, c.MQTTPlainPort = true, 8883, 1883
	if !applyPingMQTT(reply("1883", &off), c, time.Now()) {
		t.Fatal("TLS off should apply")
	}
	if c.MQTTTLS || c.MQTTPort != 1883 || c.MQTTPlainPort != 0 {
		t.Fatalf("got tls=%v port=%d plain=%d", c.MQTTTLS, c.MQTTPort, c.MQTTPlainPort)
	}
}

func TestPingWithoutTLSFieldKeepsSetting(t *testing.T) {
	c := plainDevice()
	c.MQTTTLS, c.MQTTPort = true, 8883
	if applyPingMQTT(reply("8883", nil), c, time.Now()) || !c.MQTTTLS {
		t.Fatal("a reply without the TLS field must leave TLS alone")
	}
}

func TestPingTLSSwitchSkippedWhileWaiting(t *testing.T) {
	on := true
	now := time.Now()
	c := plainDevice()
	c.MQTTTLSRetryAfter = now.Add(time.Hour).Unix()
	if applyPingMQTT(reply("8883", &on), c, now) {
		t.Fatal("no change expected while waiting")
	}
	if c.MQTTTLS || c.MQTTPort != 1883 {
		t.Fatalf("must stay plain on 1883, got tls=%v port=%d", c.MQTTTLS, c.MQTTPort)
	}
	// A new broker still applies, on the current plain port.
	r := reply("8883", &on)
	r.Endpoint = "c.example"
	if !applyPingMQTT(r, c, now) || c.MQTTEndpoint != "c.example" || c.MQTTPort != 1883 || c.MQTTTLS {
		t.Fatalf("got endpoint=%s port=%d tls=%v", c.MQTTEndpoint, c.MQTTPort, c.MQTTTLS)
	}
}
