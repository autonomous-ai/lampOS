package mqtt

import (
	"testing"
	"time"
)

func TestTLSFailing(t *testing.T) {
	now := time.Unix(1_000_000, 0)
	markStart(true, now)
	if TLSFailing(now.Add(time.Minute), 3*time.Minute) {
		t.Fatal("too early to report")
	}
	if !TLSFailing(now.Add(4*time.Minute), 3*time.Minute) {
		t.Fatal("TLS client that never connected should report failing")
	}
	markConnected(true, now.Add(5*time.Minute))
	if TLSFailing(now.Add(6*time.Minute), 3*time.Minute) {
		t.Fatal("connected TLS client is not failing")
	}
	markStart(false, now)
	if TLSFailing(now.Add(time.Hour), 3*time.Minute) {
		t.Fatal("plain client is never TLS failing")
	}
}
