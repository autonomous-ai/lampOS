package domain

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
)

func TestServiceRegistryRejectsUnsafeOwnership(t *testing.T) {
	base := OTAServiceRecord{Service: "lamp-os", Version: "1.0.0", Release: "/opt/autonomous-services/lamp-os/releases/0123456789abcdef0123456789abcdef", Data: "/var/lib/autonomous-services/lamp-os/releases/0123456789abcdef0123456789abcdef"}
	for _, kind := range []string{"unknown-replaced", "missing-block", "cycle", "path-traversal"} {
		t.Run(kind, func(t *testing.T) {
			r := OTAServiceRegistry{Format: 1, Components: map[string]OTAServiceRecord{}, Blocked: map[string]string{}}
			c := base
			switch kind {
			case "unknown-replaced":
				c.Replaces = []string{"unregistered"}
				r.Blocked["unregistered"] = "lamp-os"
			case "missing-block":
				c.Replaces = []string{"hal"}
			case "cycle":
				c.Replaces = []string{"lamp-next"}
				r.Blocked["lamp-next"] = "lamp-os"
				r.Blocked["lamp-os"] = "lamp-next"
				r.Components["lamp-next"] = OTAServiceRecord{Service: "lamp-next", Replaces: []string{"lamp-os"}, Version: "1.0.0", Release: "/opt/autonomous-services/lamp-next/releases/0123456789abcdef0123456789abcdef", Data: "/var/lib/autonomous-services/lamp-next/releases/0123456789abcdef0123456789abcdef"}
			case "path-traversal":
				c.Release = "/opt/autonomous-services/lamp-os/releases/../a"
			}
			r.Components["lamp-os"] = c
			data, err := json.Marshal(r)
			if err != nil {
				t.Fatal(err)
			}
			path := filepath.Join(t.TempDir(), "ownership.json")
			if err := os.WriteFile(path, data, 0600); err != nil {
				t.Fatal(err)
			}
			if _, err := LoadOTAServiceRegistry(path); err == nil {
				t.Fatal("invalid registry accepted")
			}
		})
	}
}

func TestLegacyAgentTargetsCannotBeServiceReplacements(t *testing.T) {
	for _, key := range []string{OTAKeyHermes, OTAKeyOpenClaw, OTAKeyCodex, OTAKeyClaudeCode, OTAKeyPicoClaw, OTAKeyOpenCode} {
		if LegacyOTAService(key) || !ReservedOTAService(key) {
			t.Errorf("agent %s must be reserved and nonreplaceable", key)
		}
		if err := ValidateOTAServiceComponent("lamp-os", OTAComponent{Service: "lamp-os", Replaces: []string{key}}); err == nil {
			t.Errorf("legacy agent replacement %s accepted", key)
		}
	}
}

func TestUpdaterCommandsCannotBeServiceComponents(t *testing.T) {
	for _, key := range []string{"recover", "rollback", "rollback-to-legacy"} {
		if !ReservedOTAService(key) {
			t.Fatalf("updater command %s not reserved", key)
		}
		if err := ValidateOTAServiceComponent(key, OTAComponent{Service: key}); err == nil {
			t.Fatalf("updater command %s accepted as component", key)
		}
	}
}
