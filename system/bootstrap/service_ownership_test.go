package bootstrap

import (
	"context"
	"encoding/json"
	"go.autonomous.ai/os/system/bootstrap/config"
	"go.autonomous.ai/os/system/bootstrap/state"
	"go.autonomous.ai/os/system/domain"
	"os"
	"path/filepath"
	"testing"
)

func serviceBootstrap(t *testing.T) (*Bootstrap, string) {
	t.Helper()
	dir, calls := setupUpdateAudit(t)
	return &Bootstrap{cfg: &config.Config{StateFile: filepath.Join(dir, "state.json")}, state: &state.State{Components: map[string]string{}}, serviceStateDir: dir, pendingUpdatePath: filepath.Join(dir, "pending.json"), rollbackConfigPath: filepath.Join(dir, "config.json")}, calls
}
func serviceRecord(key string, replaces ...string) domain.OTAServiceRecord {
	return domain.OTAServiceRecord{Service: key, Replaces: replaces, Version: "1.0.0", Release: "/opt/autonomous-services/" + key + "/releases/0123456789abcdef0123456789abcdef", Data: "/var/lib/autonomous-services/" + key + "/releases/0123456789abcdef0123456789abcdef"}
}
func putServiceRegistry(t *testing.T, b *Bootstrap, r domain.OTAServiceRegistry) {
	t.Helper()
	data, err := json.Marshal(r)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(b.serviceDirectory(), "ownership.json"), data, 0600); err != nil {
		t.Fatal(err)
	}
}
func sampleServiceRegistry() domain.OTAServiceRegistry {
	return domain.OTAServiceRegistry{Format: 1, Components: map[string]domain.OTAServiceRecord{"lamp-os": serviceRecord("lamp-os", "hal", "os-server"), "voice-worker": serviceRecord("voice-worker")}, Blocked: map[string]string{"hal": "lamp-os", "os-server": "lamp-os"}}
}
func TestServiceOwnershipIndependentComponentsAndBlockedCache(t *testing.T) {
	b, calls := serviceBootstrap(t)
	putServiceRegistry(t, b, sampleServiceRegistry())
	for _, key := range []string{"lamp-os", "voice-worker", "bootstrap", "web", "hermes"} {
		if !b.serviceTargetAllowed(key) {
			t.Errorf("independent %s blocked", key)
		}
	}
	for _, key := range []string{"hal", "os-server", "device"} {
		b.state.Components[key] = "1.0.0"
		if b.serviceTargetAllowed(key) || b.componentInstalled(key) {
			t.Errorf("replaced %s allowed", key)
		}
		if updated, err := b.reconcile(context.Background(), key, domain.OTAComponent{Version: "2.0.0"}); updated || err != nil {
			t.Errorf("blocked reconcile %s: %v %v", key, updated, err)
		}
		if err := b.forceUpdate(context.Background(), key); err == nil {
			t.Errorf("force allowed replaced %s", key)
		}
	}
	if _, err := os.Stat(calls); !os.IsNotExist(err) {
		t.Fatalf("blocked updater invoked: %v", err)
	}
}
func TestServiceFeedCannotActivateAndTopologyImmutable(t *testing.T) {
	b, calls := serviceBootstrap(t)
	target := domain.OTAComponent{Version: "2.0.0", Service: "lamp-os", Replaces: []string{"hal", "os-server"}}
	b.state.Components["lamp-os"] = "1.0.0"
	if updated, err := b.reconcile(context.Background(), "lamp-os", target); updated || err != nil {
		t.Fatalf("feed activation: %v %v", updated, err)
	}
	if err := b.forceUpdate(context.Background(), "lamp-os"); err == nil {
		t.Fatal("force activated unknown service")
	}
	putServiceRegistry(t, b, sampleServiceRegistry())
	target.Replaces = []string{"hal"}
	if _, err := b.reconcile(context.Background(), "lamp-os", target); err == nil {
		t.Fatal("changed topology accepted")
	}
	if _, err := os.Stat(calls); !os.IsNotExist(err) {
		t.Fatal("updater invoked")
	}
}
func TestServiceChainedOwnershipKeepsAllAncestorsBlocked(t *testing.T) {
	b, _ := serviceBootstrap(t)
	r := sampleServiceRegistry()
	r.Components["lamp-next"] = serviceRecord("lamp-next", "lamp-os")
	r.Blocked["lamp-os"] = "lamp-next"
	r.Components["lamp-final"] = serviceRecord("lamp-final", "lamp-next")
	r.Blocked["lamp-next"] = "lamp-final"
	putServiceRegistry(t, b, r)
	for _, key := range []string{"hal", "os-server", "lamp-os", "lamp-next", "device"} {
		if b.serviceTargetAllowed(key) {
			t.Errorf("ancestor %s allowed", key)
		}
	}
	if !b.serviceTargetAllowed("lamp-final") || !b.serviceTargetAllowed("voice-worker") {
		t.Fatal("active independent services blocked")
	}
}
func TestServiceInvalidRegistryBlocksEveryUpdate(t *testing.T) {
	b, calls := serviceBootstrap(t)
	if err := os.WriteFile(filepath.Join(b.serviceDirectory(), "ownership.json"), []byte(`{"format":9}`), 0600); err != nil {
		t.Fatal(err)
	}
	for _, key := range []string{"bootstrap", "hal", "lamp-os"} {
		if b.serviceTargetAllowed(key) {
			t.Errorf("invalid registry allowed %s", key)
		}
		if err := b.forceUpdate(context.Background(), key); err == nil {
			t.Errorf("force allowed %s", key)
		}
	}
	if _, err := os.Stat(calls); !os.IsNotExist(err) {
		t.Fatal("updater invoked")
	}
}
func TestServiceRecoveryPrecedesUpdatesAndSkipsOrphanHAL(t *testing.T) {
	b, calls := serviceBootstrap(t)
	putServiceRegistry(t, b, sampleServiceRegistry())
	if err := os.Mkdir(filepath.Join(b.serviceDirectory(), "hal.previous"), 0755); err != nil {
		t.Fatal(err)
	}
	if err := b.recoverPendingUpdate(context.Background()); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(calls); !os.IsNotExist(err) {
		t.Fatal("orphan HAL resurrected")
	}
	if err := os.WriteFile(filepath.Join(b.serviceDirectory(), "transition.json"), []byte(`{}`), 0600); err != nil {
		t.Fatal(err)
	}
	t.Setenv("UPDATE_TEST_FAIL", "recover")
	if err := b.checkOnce(context.Background()); err == nil {
		t.Fatal("recovery failure must stop before metadata")
	}
	data, err := os.ReadFile(calls)
	if err != nil || string(data) != "recover\n" {
		t.Fatalf("recovery calls %q %v", data, err)
	}
}
func TestServiceMetadataAcceptsConfiguredTrustModeAndRequiresSafeNames(t *testing.T) {
	payload := []byte(`{"lamp-os":{"service":"lamp-os","replaces":["hal"],"version":"1.0.0","url":"https://example.com/a","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}}`)
	if meta, err := decodeOTAMetadataPayload(payload, false); err != nil || len(meta) != 1 {
		t.Fatalf("unsigned service entry rejected: %v %v", meta, err)
	}
	if _, err := decodeOTAMetadataPayload(payload, true); err != nil {
		t.Fatal(err)
	}
	for _, c := range []domain.OTAComponent{{Service: "other"}, {Service: "lamp-os", Replaces: []string{"bootstrap"}}, {Service: "lamp-os", Replaces: []string{"hal", "hal"}}} {
		if err := domain.ValidateOTAServiceComponent("lamp-os", c); err == nil {
			t.Fatalf("invalid component accepted: %+v", c)
		}
	}
	for _, key := range []string{"hal", "../lamp", "lamp.service", "Lamp", ""} {
		if err := domain.ValidateOTAServiceComponent(key, domain.OTAComponent{Service: key}); err == nil {
			t.Errorf("invalid key %q", key)
		}
	}
}
func TestServiceInstalledForceReachesUpdater(t *testing.T) {
	b, calls := serviceBootstrap(t)
	putServiceRegistry(t, b, sampleServiceRegistry())
	t.Setenv("UPDATE_TEST_FAIL", "voice-worker")
	if err := b.forceUpdate(context.Background(), "voice-worker"); err == nil {
		t.Fatal("expected updater test failure")
	}
	data, err := os.ReadFile(calls)
	if err != nil || string(data) != "voice-worker\n" {
		t.Fatalf("calls %q %v", data, err)
	}
}
func TestServiceUpdaterHeaderIsExact(t *testing.T) {
	if !updaterSupportsServiceProtocol([]byte("#!/bin/bash\n# Service OTA protocol: 1\necho ok\n")) {
		t.Fatal("compatible declaration rejected")
	}
	for _, body := range []string{"#!/bin/bash\necho '# Service OTA protocol: 1'\n", "#!/bin/bash\n# Service OTA protocol: 10\n", "#!/bin/bash\n# Service OTA protocol: 1\n# Service OTA protocol: 2\n"} {
		if updaterSupportsServiceProtocol([]byte(body)) {
			t.Fatal("invalid declaration accepted")
		}
	}
}

func TestUnsignedServiceMetadataPreservesLegacyUpdates(t *testing.T) {
	payload := []byte(`{"hal":{"version":"2.0.0","url":"https://example.com/hal"},"lamp-os":{"service":"lamp-os","replaces":["hal"],"version":"1.0.0","url":"https://example.com/lamp","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}}`)
	meta, err := decodeOTAMetadataPayload(payload, false)
	if err != nil || len(meta) != 2 || meta["hal"].Version != "2.0.0" {
		t.Fatalf("legacy feed changed: %+v %v", meta, err)
	}
	if _, exists := meta["lamp-os"]; !exists {
		t.Fatal("unsigned service component missing")
	}
}
