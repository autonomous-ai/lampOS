package domain

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"slices"
	"strings"
)

const OTAServiceRegistryPath = "/root/bootstrap/services/ownership.json"

var otaReleaseID = regexp.MustCompile(`^[a-f0-9]{32}$`)

var otaServiceName = regexp.MustCompile(`^[a-z][a-z0-9-]{0,47}$`)

func ValidOTAServiceName(name string) bool { return otaServiceName.MatchString(name) }
func LegacyOTAService(key string) bool {
	switch key {
	case OTAKeyHal, OTAKeyOSServer:
		return true
	}
	return false
}
func ReservedOTAService(key string) bool {
	if LegacyOTAService(key) {
		return true
	}
	switch key {
	case OTAKeyHermes, OTAKeyOpenClaw, OTAKeyCodex, OTAKeyClaudeCode, OTAKeyPicoClaw, OTAKeyOpenCode, OTAKeyBootstrap, OTAKeyWeb, OTAKeyDevice, OTAKeyBuddy, "nginx", "system", "agent", "devices", "signed", "bootstrap-server", "ssh", "sshd", "dbus", "systemd", "networking", "network-manager", "recover", "rollback", "rollback-to-legacy":
		return true
	}
	return false
}

type OTAServiceRecord struct {
	Service  string   `json:"service"`
	Replaces []string `json:"replaces"`
	Version  string   `json:"version"`
	Release  string   `json:"release"`
	Data     string   `json:"data"`
}
type OTAServiceRegistry struct {
	Format     int                         `json:"format"`
	Components map[string]OTAServiceRecord `json:"components"`
	Blocked    map[string]string           `json:"blocked"`
}

func ValidateOTAServiceComponent(key string, c OTAComponent) error {
	if !ValidOTAServiceName(key) || c.Service != key || ReservedOTAService(key) {
		return fmt.Errorf("invalid service component %q", key)
	}
	seen := map[string]bool{}
	for _, old := range c.Replaces {
		if old == key || !ValidOTAServiceName(old) || seen[old] || (!LegacyOTAService(old) && ReservedOTAService(old)) {
			return fmt.Errorf("invalid replacement %q", old)
		}
		seen[old] = true
	}
	return nil
}
func LoadOTAServiceRegistry(path string) (*OTAServiceRegistry, error) {
	r := &OTAServiceRegistry{Format: 1, Components: map[string]OTAServiceRecord{}, Blocked: map[string]string{}}
	info, err := os.Lstat(path)
	if os.IsNotExist(err) {
		return r, nil
	}
	if err != nil {
		return nil, fmt.Errorf("inspect service ownership: %w", err)
	}
	if !info.Mode().IsRegular() {
		return nil, fmt.Errorf("service ownership must be a regular file")
	}
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("read service ownership: %w", err)
	}
	if err = json.Unmarshal(data, r); err != nil {
		return nil, fmt.Errorf("decode service ownership: %w", err)
	}
	if r.Format != 1 || r.Components == nil || r.Blocked == nil {
		return nil, fmt.Errorf("invalid service ownership format")
	}
	for key, c := range r.Components {
		if err := ValidateOTAServiceComponent(key, OTAComponent{Service: c.Service, Replaces: c.Replaces}); err != nil {
			return nil, err
		}
		for _, old := range c.Replaces {
			if !LegacyOTAService(old) {
				if _, ok := r.Components[old]; !ok {
					return nil, fmt.Errorf("unknown replaced component %q", old)
				}
			}
			if r.Blocked[old] != key {
				return nil, fmt.Errorf("missing ownership edge %s -> %s", old, key)
			}
		}
		if strings.TrimSpace(c.Version) == "" || !releasePath(c.Release, "/opt/autonomous-services/", key) || !releasePath(c.Data, "/var/lib/autonomous-services/", key) {
			return nil, fmt.Errorf("invalid service installation %q", key)
		}
	}
	for old, next := range r.Blocked {
		record, ok := r.Components[next]
		if !ok || !slices.Contains(record.Replaces, old) {
			return nil, fmt.Errorf("invalid ownership edge %s -> %s", old, next)
		}
		seen := map[string]bool{}
		for k := old; k != ""; k = r.Blocked[k] {
			if seen[k] {
				return nil, fmt.Errorf("cyclic service ownership")
			}
			seen[k] = true
		}
	}
	return r, nil
}
func releasePath(path, base, key string) bool {
	prefix := base + key + "/releases/"
	return filepath.Clean(path) == path && strings.HasPrefix(path, prefix) && filepath.Dir(path) == strings.TrimSuffix(prefix, "/") && otaReleaseID.MatchString(filepath.Base(path))
}
func (r *OTAServiceRegistry) Allows(key string) bool {
	if r.Blocked[key] != "" {
		return false
	}
	return key != OTAKeyDevice || (r.Blocked[OTAKeyHal] == "" && r.Blocked[OTAKeyOSServer] == "")
}
func (r *OTAServiceRegistry) ValidateTopology(key string, c OTAComponent) error {
	old, ok := r.Components[key]
	if !ok {
		return fmt.Errorf("service component %q requires explicit activation", key)
	}
	if err := ValidateOTAServiceComponent(key, c); err != nil {
		return err
	}
	a, b := slices.Clone(old.Replaces), slices.Clone(c.Replaces)
	slices.Sort(a)
	slices.Sort(b)
	if old.Service != c.Service || !slices.Equal(a, b) {
		return fmt.Errorf("service topology changed for %q; activate a new component", key)
	}
	return nil
}
