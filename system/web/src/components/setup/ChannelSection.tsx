import { MessageSquare } from "lucide-react";
import { ConfiguredHint, LockedField, LockedPasswordField, SectionCard, LABEL_STYLE, INPUT_STYLE, FIELD_GAP } from "./shared";
import type { SetupChannelType } from "@/types";
import type { ChannelLoadedState } from "@/hooks/setup/types";

export function ChannelSection({
  active, channel, setChannel,
  channelLoaded,
  teleToken, setTeleToken, teleUserId, setTeleUserId,
  slackBotToken, setSlackBotToken, slackAppToken, setSlackAppToken, slackUserId, setSlackUserId,
  discordGuildId, setDiscordGuildId, discordUserId, setDiscordUserId,
}: {
  active: boolean;
  channel: SetupChannelType;
  setChannel: (v: SetupChannelType) => void;
  channelLoaded: ChannelLoadedState;
  teleToken: string; setTeleToken: (v: string) => void;
  teleUserId: string; setTeleUserId: (v: string) => void;
  slackBotToken: string; setSlackBotToken: (v: string) => void;
  slackAppToken: string; setSlackAppToken: (v: string) => void;
  slackUserId: string; setSlackUserId: (v: string) => void;
  discordGuildId: string; setDiscordGuildId: (v: string) => void;
  discordUserId: string; setDiscordUserId: (v: string) => void;
}) {
  return (
    <SectionCard id="channel" title="Messaging Channels" active={active} icon={<MessageSquare size={17} />}
      description="Optionally connect a chat app now, or configure one later in Settings.">
      <div style={{ marginBottom: FIELD_GAP }}>
        <label htmlFor="channel" style={LABEL_STYLE}>Channel</label>
        <select
          id="channel"
          value={channel}
          onChange={(e) => setChannel(e.target.value as SetupChannelType)}
          style={{ ...INPUT_STYLE, cursor: "pointer" }}
        >
          <option value="none">Not now</option>
          <option value="telegram">Telegram</option>
          <option value="slack">Slack</option>
          <option value="discord">Discord</option>
        </select>
      </div>
      {channel === "telegram" && (
        <>
          {channelLoaded.teleToken ? (
            <ConfiguredHint label="Bot Token" />
          ) : (
            <LockedPasswordField lockedInitially={false} label="Bot Token" id="tele_token" value={teleToken} onChange={setTeleToken} placeholder="123456:ABC-DEF..." />
          )}
          <LockedField lockedInitially={channelLoaded.teleUserId} label="User ID" id="tele_user_id" value={teleUserId} onChange={setTeleUserId} placeholder="123456789" />
        </>
      )}
      {channel === "slack" && (
        <>
          {channelLoaded.slackBotToken ? (
            <ConfiguredHint label="Bot Token" />
          ) : (
            <LockedPasswordField lockedInitially={false} label="Bot Token" id="slack_bot_token" value={slackBotToken} onChange={setSlackBotToken} placeholder="xoxb-..." />
          )}
          {channelLoaded.slackAppToken ? (
            <ConfiguredHint label="App Token" />
          ) : (
            <LockedPasswordField lockedInitially={false} label="App Token" id="slack_app_token" value={slackAppToken} onChange={setSlackAppToken} placeholder="xapp-..." />
          )}
          <LockedField lockedInitially={channelLoaded.slackUserId} label="User ID" id="slack_user_id" value={slackUserId} onChange={setSlackUserId} placeholder="U0123456789" />
        </>
      )}
      {channel === "discord" && (
        <>
          <LockedField lockedInitially={channelLoaded.discordGuildId} label="Guild ID" id="discord_guild_id" value={discordGuildId} onChange={setDiscordGuildId} placeholder="123456789" />
          <LockedField lockedInitially={channelLoaded.discordUserId} label="User ID" id="discord_user_id" value={discordUserId} onChange={setDiscordUserId} placeholder="123456789" />
        </>
      )}
    </SectionCard>
  );
}
