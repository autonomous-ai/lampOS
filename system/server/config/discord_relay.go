package config

import (
	"encoding/json"
	"log/slog"
	"strings"
)

// legacyDiscordCredentialKeys are config.json keys earlier firmware used to keep a
// Discord credential on the device. They are never read into Config; a non-empty
// value only triggers the switch to relay mode.
var legacyDiscordCredentialKeys = []string{"discord_bot_token", "discord_app_token"}

// adoptDiscordRelay inspects the raw config.json bytes for a Discord credential left
// by older firmware. When one is present the device switches to relay mode
// (DiscordManaged) and the caller is told to rewrite the file; Config has no field
// for the credential, so the rewrite drops it. Returns true when the file needs
// rewriting.
//
// Earlier firmware wrote an empty "discord_bot_token" key on every device, so an
// empty value means "never set up" and changes nothing.
func adoptDiscordRelay(raw []byte, cfg *Config) bool {
	var keys map[string]json.RawMessage
	if err := json.Unmarshal(raw, &keys); err != nil {
		return false
	}
	found := false
	for _, k := range legacyDiscordCredentialKeys {
		v, ok := keys[k]
		if !ok {
			continue
		}
		var s string
		if json.Unmarshal(v, &s) != nil || strings.TrimSpace(s) == "" {
			continue
		}
		found = true
		slog.Info("dropping stored Discord credential; Discord now runs through the cloud relay",
			"component", "config", "key", k)
	}
	if !found {
		return false
	}
	cfg.DiscordManaged = true
	return true
}
