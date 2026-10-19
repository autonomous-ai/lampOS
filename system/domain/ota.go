package domain

import (
	"fmt"
	"regexp"
	"strings"
)

// OTAComponent describes version and download URL for a single component.
// Auto-update triggers only below MinVersion (empty = Version); manual software-update always installs Version.
type OTAComponent struct {
	Service    string   `json:"service,omitempty"`
	Replaces   []string `json:"replaces,omitempty"`
	Version    string   `json:"version"`
	MinVersion string   `json:"min_version,omitempty"`
	// MinVersionByDevice overrides MinVersion per device_type; types not listed fall back to MinVersion.
	MinVersionByDevice map[string]string `json:"min_version_by_device,omitempty"`
	// ActivateByDevice authorizes first installation of a service for an exact device type.
	ActivateByDevice map[string]bool `json:"activate_by_device,omitempty"`
	// ActivationReleaseByDevice pins initial migration to the reviewed artifact.
	ActivationReleaseByDevice map[string]OTAActivationRelease `json:"activation_release_by_device,omitempty"`
	URL                       string                          `json:"url"`
	SHA256                    string                          `json:"sha256,omitempty"`
	// Commit pins a git-installed component (hermes) to an upstream commit; empty = unpinned (HEAD).
	Commit string `json:"commit,omitempty"`
}

const (
	OTAKeyOSServer  = "os-server"
	OTAKeyBootstrap = "bootstrap"
	OTAKeyOpenClaw  = "openclaw"
	OTAKeyWeb       = "web"
	OTAKeyHal       = "hal"
	OTAKeyBuddy     = "claude-desktop-buddy"
	// OTAKeyDevice is not a flat metadata key; the profile lives at metadata.devices.<device_type>.
	OTAKeyDevice = "device"

	// Agent-runtime CLI keys must equal the config.json `agent_runtime` name; bootstrap updates only the active one.
	// Hermes auto-updates only when its entry carries a Commit (unpinned min_version is unreachable).
	OTAKeyHermes     = "hermes"
	OTAKeyCodex      = "codex"
	OTAKeyClaudeCode = "claudecode"
	OTAKeyOpenCode   = "opencode"
	// OTAKeyPicoClaw's version is a release tag, read from PicoClawVersionStamp.
	OTAKeyPicoClaw = "picoclaw"
)

// PicoClawVersionStamp records the installed PicoClaw release tag (the binary cannot report it).
const PicoClawVersionStamp = "/usr/local/lib/os-runtimes/picoclaw/installed-version"

// OTAMetadata is the JSON shape returned by the OTA metadata URL, keyed by component.
type OTAMetadata map[string]OTAComponent

// OTAActivationRelease prevents a later upload from inheriting migration consent.
type OTAActivationRelease struct {
	Version string `json:"version"`
	SHA256  string `json:"sha256"`
}

var otaActivationVersion = regexp.MustCompile(`^[0-9]+\.[0-9]+\.[0-9]+$`)
var otaActivationDigest = regexp.MustCompile(`^[a-f0-9]{64}$`)

// ValidateOTAActivationReleases validates shape; stale but well-formed bindings
// are allowed in metadata, but cannot authorize another release's activation.
func ValidateOTAActivationReleases(c OTAComponent) error {
	for device, release := range c.ActivationReleaseByDevice {
		if device == "" || strings.TrimSpace(device) != device || strings.ContainsAny(device, "\r\n\x00") ||
			!otaActivationVersion.MatchString(release.Version) || !otaActivationDigest.MatchString(release.SHA256) {
			return fmt.Errorf("invalid activation release for device %q", device)
		}
	}
	return nil
}
