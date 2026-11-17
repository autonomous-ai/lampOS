package mqtt

import (
	"sync/atomic"
	"time"
)

// TLS connection health, read by the ping so the backend can see a device
// whose TLS connection to the broker never comes up. Unix seconds; 0 = unset.
var (
	tlsStartedAt   atomic.Int64
	tlsConnectedAt atomic.Int64
)

func markStart(tls bool, now time.Time) {
	tlsConnectedAt.Store(0)
	if tls {
		tlsStartedAt.Store(now.Unix())
	} else {
		tlsStartedAt.Store(0)
	}
}

func markConnected(tls bool, now time.Time) {
	if tls {
		tlsConnectedAt.Store(now.Unix())
	}
}

// TLSFailing reports whether a TLS client started at least `after` ago and has
// not connected since. The client keeps retrying over TLS either way.
func TLSFailing(now time.Time, after time.Duration) bool {
	start := tlsStartedAt.Load()
	return start != 0 && tlsConnectedAt.Load() == 0 && now.Unix()-start >= int64(after/time.Second)
}
