// Package discordscrub removes Discord settings from JSON config files.
//
// Discord messages go through the cloud relay, so no runtime config on the device
// keeps Discord credentials. Besides each runtime's live config, copies of it can sit
// beside it (backups, the archive kept when one runtime imports another's config);
// this package cleans any such JSON file without needing to know its exact layout.
package discordscrub

import (
	"encoding/json"
	"fmt"
	"os"
	"strings"
)

// credentialKeys are the (lower-cased) keys that hold a Discord credential inside a
// "discord" object.
var credentialKeys = map[string]bool{
	"token": true, "bottoken": true, "bot_token": true,
	"apptoken": true, "app_token": true,
	"clientsecret": true, "client_secret": true,
}

// flatKeys are credential keys that carry the Discord name themselves.
var flatKeys = map[string]bool{
	"discord_bot_token": true, "discord_app_token": true,
	"discordbottoken": true, "discordapptoken": true,
}

// Redact removes Discord settings from a decoded JSON value in place and reports
// whether anything changed:
//   - an object under "channels" named "discord" is deleted;
//   - any other "discord" object loses its credential keys and, if it has an
//     "enabled" key, is switched off;
//   - flat keys such as "discord_bot_token" are deleted.
func Redact(v any) bool {
	return redact(v, "")
}

func redact(v any, parentKey string) bool {
	changed := false
	switch t := v.(type) {
	case map[string]any:
		for k, child := range t {
			lk := strings.ToLower(k)
			if flatKeys[lk] {
				if s, ok := child.(string); !ok || s != "" {
					delete(t, k)
					changed = true
				}
				continue
			}
			if lk == "discord" {
				if m, ok := child.(map[string]any); ok {
					if strings.EqualFold(parentKey, "channels") {
						delete(t, k)
						changed = true
						continue
					}
					for ck := range m {
						if credentialKeys[strings.ToLower(ck)] {
							delete(m, ck)
							changed = true
						}
					}
					if en, ok := m["enabled"]; ok && en != false {
						m["enabled"] = false
						changed = true
					}
				}
			}
			if redact(child, k) {
				changed = true
			}
		}
	case []any:
		for _, child := range t {
			if redact(child, parentKey) {
				changed = true
			}
		}
	}
	return changed
}

// ScrubFile cleans one JSON file. A missing file is not an error. A file that is not
// a JSON object is left alone and reported as an error. The file is rewritten, with
// its mode kept, only when something was removed.
func ScrubFile(path string) (bool, error) {
	raw, err := os.ReadFile(path)
	if err != nil {
		if os.IsNotExist(err) {
			return false, nil
		}
		return false, fmt.Errorf("read %s: %w", path, err)
	}
	var doc any
	if err := json.Unmarshal(raw, &doc); err != nil {
		return false, fmt.Errorf("parse %s: %w", path, err)
	}
	if !Redact(doc) {
		return false, nil
	}
	out, err := json.MarshalIndent(doc, "", "  ")
	if err != nil {
		return false, fmt.Errorf("marshal %s: %w", path, err)
	}
	mode := os.FileMode(0600)
	if info, err := os.Stat(path); err == nil {
		mode = info.Mode().Perm()
	}
	if err := os.WriteFile(path, out, mode); err != nil {
		return false, fmt.Errorf("write %s: %w", path, err)
	}
	return true, nil
}
