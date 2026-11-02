package agent

import (
	"log/slog"

	"go.autonomous.ai/os/runtimes/hermes"
	"go.autonomous.ai/os/runtimes/openclaw"
	"go.autonomous.ai/os/runtimes/picoclaw"
	"go.autonomous.ai/os/system/domain"
	"go.autonomous.ai/os/system/server/config"
)

// picoclawHome is where picoclaw keeps its config and secrets on the device.
const picoclawHome = "/root/.picoclaw"

const discordRelayComponent = "discord-relay"

// DiscordRelayReconcile makes sure no runtime config on the device holds Discord
// settings of its own. Discord messages go through the cloud relay, so a device
// that was set up with a bot token before relay mode existed is cleaned on startup:
// config.json drops the token when it loads (see config.adoptDiscordRelay), and this
// removes what earlier firmware wrote into each runtime's own files, whichever
// runtime is active. A runtime whose file changed is restarted if it is the active
// one. It is idempotent and cheap: it rewrites a file only when it finds something.
type DiscordRelayReconcile struct {
	cfg *config.Config
	gw  domain.AgentGateway

	// Test seams.
	openclawDir string
	hermesHome  string
	picoHome    string
}

// ProvideDiscordRelayReconcile is the Wire provider for DiscordRelayReconcile.
func ProvideDiscordRelayReconcile(cfg *config.Config, gw domain.AgentGateway) *DiscordRelayReconcile {
	return &DiscordRelayReconcile{cfg: cfg, gw: gw, openclawDir: cfg.OpenclawConfigDir, hermesHome: hermesHome, picoHome: picoclawHome}
}

// Reconcile removes Discord settings from every runtime's config files.
func (r *DiscordRelayReconcile) Reconcile() {
	active := r.cfg.AgentRuntime
	if active == "" {
		active = domain.AgentRuntimeOpenClaw
	}

	type scrub struct {
		runtime string
		run     func() (bool, error)
	}
	scrubs := []scrub{
		{domain.AgentRuntimeOpenClaw, func() (bool, error) { return openclaw.ScrubDiscordConfig(r.openclawDir) }},
		{domain.AgentRuntimeHermes, func() (bool, error) { return hermes.ScrubDiscordEnv(r.hermesHome) }},
		{domain.AgentRuntimePicoclaw, func() (bool, error) { return picoclaw.ScrubDiscordConfig(r.picoHome) }},
	}

	restart := false
	for _, s := range scrubs {
		changed, err := s.run()
		if err != nil {
			slog.Warn("discord relay: could not clean runtime config", "component", discordRelayComponent,
				"runtime", s.runtime, "error", err)
			continue
		}
		if !changed {
			continue
		}
		slog.Info("discord relay: removed Discord settings from runtime config", "component", discordRelayComponent,
			"runtime", s.runtime)
		if s.runtime == active {
			restart = true
		}
	}

	// Copies and imports of runtime config sitting beside the live files. None of them
	// is read by a running runtime, so changing them never needs a restart.
	if changed, err := openclaw.ScrubDiscordBackups(r.openclawDir); err != nil {
		slog.Warn("discord relay: could not clean openclaw config backups", "component", discordRelayComponent, "error", err)
	} else if changed {
		slog.Info("discord relay: removed Discord settings from openclaw config backups", "component", discordRelayComponent)
	}
	if changed, err := hermes.ScrubDiscordMigrationArchive(r.hermesHome); err != nil {
		slog.Warn("discord relay: could not clean hermes migration archive", "component", discordRelayComponent, "error", err)
	} else if changed {
		slog.Info("discord relay: removed Discord settings from hermes migration archive", "component", discordRelayComponent)
	}

	if changed, err := hermes.RemoveDiscordState(r.hermesHome); err != nil {
		slog.Warn("discord relay: could not remove hermes Discord state", "component", discordRelayComponent, "error", err)
	} else if changed {
		slog.Info("discord relay: removed hermes Discord state files", "component", discordRelayComponent)
	}

	if restart {
		if err := r.gw.RestartAgent(); err != nil {
			slog.Warn("discord relay: restart after cleaning failed", "component", discordRelayComponent, "error", err)
		}
	}
}
