package mqtt

import (
	"crypto/tls"
	"testing"
)

func TestServerURLScheme(t *testing.T) {
	tests := []struct {
		name string
		opts Options
		want string
	}{
		{"plain default port", Options{Endpoint: "broker.example"}, "mqtt://broker.example:1883"},
		{"plain custom port", Options{Endpoint: "broker.example", Port: 1884}, "mqtt://broker.example:1884"},
		{"tls default port", Options{Endpoint: "broker.example", TLS: true}, "ssl://broker.example:1883"},
		{"tls custom port", Options{Endpoint: " broker.example ", Port: 8883, TLS: true}, "ssl://broker.example:8883"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			u, err := tt.opts.ServerURL()
			if err != nil {
				t.Fatalf("ServerURL: %v", err)
			}
			if got := u.String(); got != tt.want {
				t.Fatalf("ServerURL = %q, want %q", got, tt.want)
			}
		})
	}
}

func TestServerURLRequiresEndpoint(t *testing.T) {
	for _, tlsOn := range []bool{false, true} {
		o := Options{TLS: tlsOn}
		if _, err := o.ServerURL(); err == nil {
			t.Fatalf("TLS=%t: empty endpoint must be rejected", tlsOn)
		}
	}
}

func TestTLSConfigOff(t *testing.T) {
	o := Options{Endpoint: "broker.example", Port: 8883}
	if cfg := o.TLSConfig(); cfg != nil {
		t.Fatalf("TLSConfig = %+v, want nil when TLS is off", cfg)
	}
}

func TestTLSConfigOn(t *testing.T) {
	o := Options{Endpoint: " broker.example ", Port: 8883, TLS: true}
	cfg := o.TLSConfig()
	if cfg == nil {
		t.Fatal("TLSConfig is nil when TLS is on")
	}
	if cfg.InsecureSkipVerify {
		t.Fatal("certificate verification must stay on")
	}
	if cfg.MinVersion != tls.VersionTLS12 {
		t.Fatalf("MinVersion = %#x, want TLS 1.2", cfg.MinVersion)
	}
	if cfg.ServerName != "broker.example" {
		t.Fatalf("ServerName = %q, want broker.example", cfg.ServerName)
	}
	if cfg.RootCAs != nil {
		t.Fatal("RootCAs must be nil so the system roots are used")
	}
	if cfg.VerifyConnection != nil || cfg.VerifyPeerCertificate != nil {
		t.Fatal("no custom verification hooks allowed")
	}
}

func TestFactoryPassesTLS(t *testing.T) {
	for _, tlsOn := range []bool{false, true} {
		f, err := ProvideFactory(Config{Endpoint: "broker.example", Port: 8883, TLS: tlsOn})
		if err != nil {
			t.Fatal(err)
		}
		c := f.GetClient("id")
		if c.opts.TLS != tlsOn {
			t.Fatalf("client TLS = %t, want %t", c.opts.TLS, tlsOn)
		}
		u, err := c.opts.ServerURL()
		if err != nil {
			t.Fatal(err)
		}
		wantScheme := "mqtt"
		if tlsOn {
			wantScheme = "ssl"
		}
		if u.Scheme != wantScheme {
			t.Fatalf("scheme = %q, want %q", u.Scheme, wantScheme)
		}
	}
}
