package bootstrap

import (
	"context"
	"crypto/ed25519"
	"crypto/rand"
	"encoding/base64"
	"encoding/json"
	"io"
	"net/http"
	"reflect"
	"strings"
	"testing"

	"go.autonomous.ai/os/system/domain"
)

type serviceMetadataTransport func(*http.Request) (*http.Response, error)

func (f serviceMetadataTransport) RoundTrip(r *http.Request) (*http.Response, error) { return f(r) }

func TestUnsignedServiceMetadataRequiresArtifactAndTopology(t *testing.T) {
	valid := domain.OTAComponent{Service: "lamp-os", Replaces: []string{"hal", "os-server"}, Version: "2.0.0", URL: "https://example.test/lamp.zip", SHA256: strings.Repeat("a", 64)}
	cases := []struct {
		name   string
		change func(*domain.OTAComponent)
	}{
		{"missing url", func(c *domain.OTAComponent) { c.URL = "" }},
		{"blank url", func(c *domain.OTAComponent) { c.URL = "  " }},
		{"missing checksum", func(c *domain.OTAComponent) { c.SHA256 = "" }},
		{"invalid checksum", func(c *domain.OTAComponent) { c.SHA256 = strings.Repeat("g", 64) }},
		{"wrong service name", func(c *domain.OTAComponent) { c.Service = "other" }},
		{"reserved replacement", func(c *domain.OTAComponent) { c.Replaces = []string{"bootstrap"} }},
		{"duplicate replacement", func(c *domain.OTAComponent) { c.Replaces = []string{"hal", "hal"} }},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			c := valid
			tc.change(&c)
			payload, err := json.Marshal(domain.OTAMetadata{"lamp-os": c})
			if err != nil {
				t.Fatal(err)
			}
			for _, signed := range []bool{false, true} {
				if _, err := decodeOTAMetadataPayload(payload, signed); err == nil {
					t.Fatalf("invalid service accepted, signed=%v", signed)
				}
			}
		})
	}
}

func TestServiceFetchUsesConfiguredTrustModeWithoutDowngrade(t *testing.T) {
	publicKey, privateKey, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	component := promotedService("lamp-os", "hal", "os-server")
	component.URL = "https://example.test/lamp.zip"
	component.SHA256 = strings.Repeat("a", 64)
	payload, err := json.Marshal(domain.OTAMetadata{"lamp-os": component})
	if err != nil {
		t.Fatal(err)
	}
	var envelope signedOTAMetadata
	envelope.Format = otaMetadataFormat
	envelope.Payload = base64.StdEncoding.EncodeToString(payload)
	envelope.Signature.Algorithm = "ed25519"
	envelope.Signature.Value = base64.StdEncoding.EncodeToString(ed25519.Sign(privateKey, payload))
	signed, err := json.Marshal(envelope)
	if err != nil {
		t.Fatal(err)
	}
	envelope.Signature.Value = base64.StdEncoding.EncodeToString(make([]byte, ed25519.SignatureSize))
	tampered, err := json.Marshal(envelope)
	if err != nil {
		t.Fatal(err)
	}
	for _, tc := range []struct {
		name, key string
		data      []byte
		wantErr   bool
	}{
		{"legacy unsigned", "", payload, false},
		{"pinned unsigned", base64.StdEncoding.EncodeToString(publicKey), payload, true},
		{"pinned valid", base64.StdEncoding.EncodeToString(publicKey), signed, false},
		{"pinned invalid", base64.StdEncoding.EncodeToString(publicKey), tampered, true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			b, _ := serviceBootstrap(t)
			b.cfg.MetadataURL = "https://example.test/metadata.json"
			b.cfg.SigningPublicKey = tc.key
			b.client = &http.Client{Transport: serviceMetadataTransport(func(*http.Request) (*http.Response, error) {
				return &http.Response{StatusCode: http.StatusOK, Body: io.NopCloser(strings.NewReader(string(tc.data)))}, nil
			})}
			meta, err := b.fetchMetadata(context.Background())
			if (err != nil) != tc.wantErr {
				t.Fatalf("metadata=%v error=%v", meta, err)
			}
			if tc.wantErr {
				return
			}
			plan, err := b.activationPlan(meta)
			if err != nil || !reflect.DeepEqual(plan, []string{"lamp-os"}) {
				t.Fatalf("plan=%v error=%v", plan, err)
			}
			// A registered service continues to receive metadata in legacy mode too.
			putServiceRegistry(t, b, sampleServiceRegistry())
			meta, err = b.fetchMetadata(context.Background())
			if err != nil || meta["lamp-os"].Version != "2.0.0" {
				t.Fatalf("managed metadata=%v error=%v", meta, err)
			}
		})
	}
}
