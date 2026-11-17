package device

import (
	"context"
	"encoding/json"
	"log/slog"
	"strconv"
	"time"

	"go.autonomous.ai/os/system/beclient"
	"go.autonomous.ai/os/system/domain"
	"go.autonomous.ai/os/system/lib/hal"
	"go.autonomous.ai/os/system/lib/mqtt"
	"go.autonomous.ai/os/system/lib/runtimereg"
	"go.autonomous.ai/os/system/lib/syspath"
	"go.autonomous.ai/os/system/server/config"
)

// buildPingPayload assembles the backend ping body (same fields as the MQTT
// `info` uplink). LocalIP is setup-critical: it rescues the AP→STA redirect.
func (s *Service) buildPingPayload(status string) beclient.PingPayload {
	if phase, _ := s.SetupRuntimeStatus(); phase == "preparing" || phase == "failed" {
		status = "setting_up"
	}
	runtime := CurrentAgentRuntimeFromConfig(s.config)
	p := beclient.PingPayload{
		Status:              status,
		SetupCompleted:      s.SetupCompleted(),
		Mac:                 GetDeviceMac(),
		Version:             config.OSVersion,
		Device:              s.config.DeviceTypeOrDefault(),
		DeviceID:            s.config.DeviceID,
		Timezone:            s.CurrentTimezone(),
		AgentRuntime:        runtime,
		AgentRuntimeVersion: runtimereg.Version(runtime),
		TTSProvider:         s.config.TTSProvider,
		TTSVoice:            s.config.TTSVoice,
		STTLanguage:         s.config.STTLanguage,
		WakeWordEnabled:     s.config.WakeWordEnabled(),
		VoiceInputMode:      s.config.GetVoiceInputMode(),
		UnsupportedChannels: s.config.ChannelsUnsupported,
		MQTTTLS:             s.config.MQTTTLSEnabled(),
		MQTTTLSSwitch:       true,
		MQTTTLSFailing:      mqtt.TLSFailing(time.Now(), mqttTLSFailingAfter),
	}
	if ip, err := s.networkService.GetCurrentIP(); err == nil && ip != apSetupIP {
		p.LocalIP = ip
	}
	if v, err := hal.GetVersion(); err == nil {
		p.HalVersion = v
	}
	if s.beClient != nil {
		p.SlackTeamID = s.beClient.SlackTeamID()
	}
	p.Skills = s.installedSkillsForPing()
	return p
}

// installedSkillsForPing returns installed skills as name+description only;
// best-effort, nil on listing failure.
func (s *Service) installedSkillsForPing() []domain.SkillSummary {
	if s.agentGateway == nil {
		return nil
	}
	list, err := s.agentGateway.ListSkills()
	if err != nil {
		// Debug, not warn: this would otherwise log on every ping.
		slog.Debug("[ping] skills list unavailable", "component", "device", "error", err)
		return nil
	}
	return domain.SummarizeSkills(list)
}

// StartStatusReporter periodically pings the backend (Bearer LLMAPIKey) and saves
// any MQTT config it returns. Exits when ctx is cancelled.
func (s *Service) StartStatusReporter(ctx context.Context) {
	if s.beClient == nil || s.config.LLMAPIKey == "" {
		return
	}
	// Off-device, never ping: the backend record belongs to a real board.
	if !syspath.BackendUplink() {
		slog.Info("backend uplink off — status reporter not started", "component", "status-reporter")
		return
	}
	ticker := time.NewTicker(beclient.StatusReportInterval)
	defer ticker.Stop()
	var lastLocalIP string
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			if !s.agentGateway.IsReady() {
				continue
			}
			// No-op once team_id is cached or when Slack is not configured.
			s.beClient.ResolveSlackTeamIDFromConfig(s.config.OpenclawConfigDir)
			payload := s.buildPingPayload("working")
			// LAN address changed: pooled connections would blackhole until the 15s
			// timeout (no RST), so drop them before pinging.
			if payload.LocalIP != "" && lastLocalIP != "" && payload.LocalIP != lastLocalIP {
				slog.Info("local IP changed, dropping pooled backend connections",
					"component", "status-reporter", "from", lastLocalIP, "to", payload.LocalIP)
				s.beClient.CloseIdleConnections()
			}
			if payload.LocalIP != "" {
				lastLocalIP = payload.LocalIP
			}
			if payload.MQTTTLSFailing {
				slog.Warn("TLS connection to the broker not up, reported in ping", "component", "status-reporter",
					"port", s.config.MQTTPort)
			}
			resp := s.beClient.PingSafe(s.config.BackendKey(), payload)
			dump, _ := json.Marshal(resp)
			slog.Debug("received response from backend", "component", "status-reporter", "response", string(dump))
			if resp == nil {
				continue
			}
			if resp.DeviceID != "" && resp.DeviceID != s.config.DeviceID {
				s.config.DeviceID = resp.DeviceID
			}
			// Apply when the broker, port or channels change: moving to the TLS port on
			// the same broker changes only the port.
			if resp.HasMQTT() && applyPingMQTT(resp.GetMQTT(), s.config) {
				slog.Info("received MQTT config from backend", "component", "status-reporter",
					"endpoint", s.config.MQTTEndpoint, "port", s.config.MQTTPort, "tls", s.config.MQTTTLSEnabled())
				if err := s.config.Save(); err != nil {
					slog.Error("save MQTT config failed", "component", "status-reporter", "error", err)
				}
			}
		}
	}
}

// mqttTLSFailingAfter is how long a TLS client may go without connecting
// before the ping reports it as failing.
const mqttTLSFailingAfter = 3 * time.Minute

// applyPingMQTT copies the backend's MQTT config into c and reports whether
// anything that needs a reconnect changed. TLS changes only when the reply
// says so; the device never turns it off by itself.
func applyPingMQTT(m *beclient.MQTTConfig, c *config.Config) bool {
	tlsChange := m.TLS != nil && *m.TLS != c.MQTTTLS
	if !tlsChange && !mqttConfigChanged(m, c) {
		return false
	}
	if tlsChange {
		c.MQTTTLS = *m.TLS
	}
	c.MQTTEndpoint = m.Endpoint
	if port, _ := strconv.Atoi(m.Port); port != 0 {
		c.MQTTPort = port
	}
	c.MQTTUsername = m.Username
	c.MQTTPassword = m.Password
	if m.FaChannel != "" {
		c.FAChannel = m.FaChannel
	}
	if m.FdChannel != "" {
		c.FDChannel = m.FdChannel
	}
	return true
}

// mqttConfigChanged reports whether the backend's MQTT config differs from the
// running one in broker, port or channels. Login changes alone don't restart.
func mqttConfigChanged(m *beclient.MQTTConfig, c *config.Config) bool {
	port, _ := strconv.Atoi(m.Port)
	return m.Endpoint != c.MQTTEndpoint ||
		(port != 0 && port != c.MQTTPort) ||
		(m.FaChannel != "" && m.FaChannel != c.FAChannel) ||
		(m.FdChannel != "" && m.FdChannel != c.FDChannel)
}
