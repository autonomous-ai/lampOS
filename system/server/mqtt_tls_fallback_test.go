package server

import (
	"context"
	"testing"
	"time"

	"go.autonomous.ai/os/system/server/config"
)

func TestRevertTLSRestoresPlainPortAndPauses(t *testing.T) {
	now := time.Unix(1_000_000, 0)
	c := &config.Config{MQTTTLS: true, MQTTPort: 8883, MQTTPlainPort: 1884}
	revertTLS(c, now)
	if c.MQTTTLS || c.MQTTPort != 1884 || c.MQTTPlainPort != 0 {
		t.Fatalf("got tls=%v port=%d plain=%d", c.MQTTTLS, c.MQTTPort, c.MQTTPlainPort)
	}
	if c.MQTTTLSSwitchAllowed(now.Add(time.Hour)) || !c.MQTTTLSSwitchAllowed(now.Add(25*time.Hour)) {
		t.Fatal("TLS switches should pause for a day")
	}

	c = &config.Config{MQTTTLS: true, MQTTPort: 8883}
	revertTLS(c, now)
	if c.MQTTPort != mqttPlainPortDefault {
		t.Fatalf("port = %d, want %d", c.MQTTPort, mqttPlainPortDefault)
	}
}

func TestWatchTLSConnectLeavesConnectedOrStoppedClients(t *testing.T) {
	s := &Server{config: &config.Config{MQTTTLS: true, MQTTPort: 8883}}
	fired := make(chan time.Time, 1)
	fired <- time.Now()
	s.watchTLSConnect(context.Background(), func() bool { return true }, fired)
	if !s.config.MQTTTLS {
		t.Fatal("a connected TLS client must not fall back")
	}

	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	s.watchTLSConnect(ctx, func() bool { return false }, make(chan time.Time))
	if !s.config.MQTTTLS {
		t.Fatal("a stopped client must not fall back")
	}
}
