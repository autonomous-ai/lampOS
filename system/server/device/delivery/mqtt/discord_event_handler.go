package mqtthandler

import (
	"encoding/json"
	"log/slog"
	"sync"
	"time"

	"go.autonomous.ai/os/system/device"
	"go.autonomous.ai/os/system/discordrelay"
	"go.autonomous.ai/os/system/domain"
	"go.autonomous.ai/os/system/server/config"
)

// handleDiscordEvent receives one Discord message from the cloud relay (cmd
// "discord_event"). The device holds no Discord credential: the relay delivers each
// message here and posts the device's reply, so there is nothing to answer on the
// transport itself. The relay checks the sender and hands the message to the agent.
func (h *DeviceMQTTHandler) handleDiscordEvent(cmd domain.MQTTMessage) error {
	var ev domain.MQTTDiscordEventCommand
	if err := json.Unmarshal(cmd.Raw(), &ev); err != nil {
		slog.Error("discord_event: invalid payload", "component", "mqtt", "error", err)
		return nil
	}
	if ev.Enc != "" || ev.Sealed != "" {
		var inner domain.MQTTDiscordEventCommand
		if err := discordrelay.Open(h.config.BackendKey(), domain.CommandDiscordEvent, h.config.DeviceID,
			ev.Enc, ev.Sealed, &inner, time.Now()); err != nil {
			slog.Warn("discord_event not delivered to the agent", "component", "mqtt", "reason", err.Error())
			return nil
		}
		inner.Cmd, inner.WasSealed = ev.Cmd, true
		ev = inner
	}
	if reason := h.discordRelay.Handle(ev); reason != "" {
		slog.Info("discord_event not delivered to the agent", "component", "mqtt",
			"event_id", ev.EventID, "reason", reason)
	}
	return nil
}

// discordUplink sends the agent's Discord answers up the device channel. The cloud
// relay routes them by the envelope's type/device/id, like every other device
// message, and posts them to Discord.
//
// Typing signals and replies come from different goroutines, so they are sent one at
// a time over their own connection id; two overlapping connections with the same id
// would make the broker drop the first.
type discordUplink struct {
	h  *DeviceMQTTHandler
	mu sync.Mutex
}

// Reply publishes {"type":"discord_reply","discord_channel_id":...,"text":...}, or,
// sealed, {"type":"discord_reply","enc":"v1","sealed":...} with both fields inside.
func (u *discordUplink) Reply(channelID, text string, sealed bool) error {
	return u.publish(domain.MsgTypeDiscordReply, channelID, map[string]any{"text": text}, sealed)
}

// ReplyInThread is Reply plus the message to open a thread on and the thread's name.
func (u *discordUplink) ReplyInThread(channelID, messageID, threadName, text string, sealed bool) error {
	return u.publish(domain.MsgTypeDiscordReply, channelID, map[string]any{
		"text": text, "thread_message_id": messageID, "thread_name": threadName,
	}, sealed)
}

// Typing publishes {"type":"discord_typing","discord_channel_id":...}, sealed as above.
func (u *discordUplink) Typing(channelID string, sealed bool) error {
	return u.publish(domain.MsgTypeDiscordTyping, channelID, nil, sealed)
}

func (u *discordUplink) publish(msgType, channelID string, extra map[string]any, sealed bool) error {
	var m map[string]any
	var err error
	if sealed {
		m, err = sealedDiscordEnvelope(u.h.config, msgType, device.GetDeviceMac(), channelID, extra, time.Now())
	} else {
		m, err = discordEnvelope(u.h.config, msgType, device.GetDeviceMac(), channelID, extra)
	}
	if err != nil {
		return err
	}
	u.mu.Lock()
	defer u.mu.Unlock()
	return u.h.publishAs("device-"+u.h.config.DeviceID+"-discord", m)
}

// discordEnvelope builds an uplink message: the standard device envelope plus the
// Discord channel id and any extra fields (the reply text).
func discordEnvelope(cfg *config.Config, msgType, mac, channelID string, extra map[string]any) (map[string]any, error) {
	b, err := json.Marshal(domain.NewMQTTInfoResponse(cfg, msgType, mac))
	if err != nil {
		return nil, err
	}
	m := map[string]any{}
	if err := json.Unmarshal(b, &m); err != nil {
		return nil, err
	}
	m["discord_channel_id"] = channelID
	for k, v := range extra {
		m[k] = v
	}
	return m, nil
}

// sealedDiscordEnvelope is discordEnvelope with the channel id and extra fields
// sealed for this device; the standard envelope fields stay in clear for routing.
func sealedDiscordEnvelope(cfg *config.Config, msgType, mac, channelID string, extra map[string]any, now time.Time) (map[string]any, error) {
	m, err := discordEnvelope(cfg, msgType, mac, "", nil)
	if err != nil {
		return nil, err
	}
	delete(m, "discord_channel_id")
	body := map[string]any{"discord_channel_id": channelID}
	for k, v := range extra {
		body[k] = v
	}
	sealed, err := discordrelay.Seal(cfg.BackendKey(), msgType, cfg.DeviceID, body, now)
	if err != nil {
		return nil, err
	}
	m["enc"] = discordrelay.SealVersion
	m["sealed"] = sealed
	return m, nil
}
