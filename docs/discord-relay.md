# Discord through the cloud relay

Discord messages reach the device, and its answers leave it, through the cloud over
MQTT. The device keeps **no Discord bot token** and opens **no connection to
Discord**: neither os-server nor any agent runtime (OpenClaw, Hermes, Claude Code,
Codex, OpenCode, PicoClaw) ever sees one.

```
Discord ──▶ cloud relay ──▶ MQTT fa_channel  {cmd:"discord_event", …}      ──▶ os-server ──▶ agent
Discord ◀── cloud relay ◀── MQTT fd_channel  {type:"discord_reply", …}     ◀── os-server ◀── agent
```

Code: `system/discordrelay/` (the relay), `system/server/device/delivery/mqtt/discord_event_handler.go`
(MQTT in/out), `system/server/agent/delivery/http/handler_event_agent.go` (collects the finished
reply), `system/agent/discord_relay.go` (startup clean-up).

## Setup

Discord is set up like any other channel: `channel: "discord"` in `POST /api/device/setup`,
`POST /api/device/channel`, the Wi-Fi provisioning body, the local Settings page, or the MQTT
`add_channel` command. The device stores only who may talk to it:

| Field | Meaning |
|-------|---------|
| `discord_user_id` | **Required.** The one Discord account the device listens to. |
| `discord_guild_id` | Optional. The one server whose messages count; without it only direct messages are accepted. |
| `discord_managed` | Accepted and persisted (`true`). Relay mode is the only mode, so a request that omits it behaves the same. |

A `discord_bot_token` (or MQTT `config.bot_token`) in any request is **ignored**: it is never
stored, never written to a runtime's config, and never logged. The setup page no longer asks
for one. MQTT `add_channel` for Discord reads `config.managed`, `config.guild_id`,
`config.user_id`.

`config.json` keeps `discord_managed`, `discord_guild_id`, `discord_user_id`.

## Messages down: `discord_event`

```json
{"cmd":"discord_event","event_id":"…","discord_user_id":"…","guild_id":"…",
 "discord_channel_id":"…","author_username":"…","bot_user_id":"…",
 "mentions_bot":true,"text":"…"}
```

os-server accepts a message only when all of these hold; anything else is dropped (and logged
with the reason, never the text):

- Discord is set up on the device (`discord_managed` and a `discord_user_id`).
- The sender is `discord_user_id`.
- A direct message (`guild_id` empty) is always fine. A server message must come from
  `discord_guild_id` **and** mention the bot; the mention is removed from the text. The @mention is not needed in a channel listed in `discord_open_channels`, in a thread of such a channel, or in a thread where the owner has already mentioned the bot (`parent_channel_id` marks a thread). A message that addresses another person, a role or everyone (`mentions_others`) still needs its own mention. One new mention brings a thread back after a restart. The owner and server checks always apply.
- The `event_id` was not seen in the last 5 minutes (the relay can redeliver).

An accepted message becomes a normal chat turn on the active runtime, whichever it is:

```
[discord] Message from <username> [id:<user id>]:
<text>
[via:discord]
```

It waits (up to 2 minutes) for the agent to finish its current turn, then is sent. The run is
marked silent, so the reply is **not** spoken. Up to 8 messages can be waiting at once.

A Discord turn is never merged with another message. Runtimes that can fold a new message into
the turn already running (Hermes native steering, Codex `turn/steer`) give every folded-in
message the same final reply, so merging would put the room's conversation into the Discord
channel, or the Discord answer onto the speaker. Instead:

- Speech, ambient speech and web or phone chat that arrive while a Discord turn runs wait for it
  to end, then run in order as their own turn.
- A Discord message that arrives while another turn runs waits the same way, then runs as its own
  turn.
- Two Discord messages never share a turn, whichever channel they came from, so a reply never
  crosses channels.
- Messages for the same destination keep merging as before: speech into a speech turn is steered
  with no added delay.

A Discord turn that fails or is cancelled ends like any other turn: its typing signal stops and
the messages waiting behind it run. Nothing waiting is dropped silently: a message a runtime
cannot keep waiting (Codex holds at most 8, and none for longer than a turn can last) is rejected
and ends with an error.

There is no acknowledgement message for `discord_event`.

## Messages up: `discord_reply`, `discord_typing`

Both use the standard device envelope (`device`, `type`, `id`, `version`, `mac`, `time`, …) plus
the channel the turn came from:

```json
{"type":"discord_reply","discord_channel_id":"…","text":"…"}
{"type":"discord_typing","discord_channel_id":"…"}
```

- **Typing** is sent when the message is accepted and every 8 seconds until the turn ends
  (10 minutes at most).
- **Reply** is sent once, when the turn finishes, with the same sanitised text every other
  channel gets (hardware markers and tags removed). A turn with nothing to say (`NO_REPLY`, only
  hardware actions, or an error) sends no reply and ends the typing signal. The cloud splits long
  replies for Discord.

The channel id always comes from the `discord_event` that started the turn; the agent cannot
choose another.

## Sealed messages

The cloud can send a `discord_event` sealed, so nothing between the cloud and the device can
read the text or the Discord ids (`system/discordrelay/seal.go`):

```json
{"cmd":"discord_event","enc":"v1","sealed":"<base64>"}
```

- Key: HMAC-SHA256 of `discord-relay/v1` keyed with the device's cloud API key. Each device has
  its own key and nothing else is stored.
- Cipher: AES-256-GCM, random 12-byte nonce, `sealed` = base64(nonce ‖ ciphertext). The
  additional data is `<kind>|<device id>`, so a message sealed for one device or one kind does
  not open as another.
- The plaintext is the JSON the message would carry in clear, plus `ts` (unix seconds). A sealed
  message more than 15 minutes old (or ahead) is dropped.

The device opens what arrives sealed and seals its `discord_reply` and `discord_typing` for that
turn the same way; the routing fields of the envelope stay in clear and `discord_channel_id` and
`text` move inside `sealed`. A message that arrives in clear is answered in clear. A sealed
message that does not open is dropped and logged without its content. The cloud decides which
devices get sealed messages; the device has no setting for it.

## Upgrading a device that has a token

On startup the device switches itself to relay mode:

1. **config.json.** When it loads and finds `discord_bot_token` (or `discord_app_token`), it sets
   `discord_managed: true`, keeps the guild and user ids, and rewrites the file without the token.
   Its config structure has no field for a token, so nothing can write one back.
2. **Every runtime's own files** (`system/agent/discord_relay.go`, first step of the startup
   sequence, every boot, a no-op once clean):
   - OpenClaw: `channels.discord` removed from `openclaw.json`, the discord plugin entry disabled.
   - Hermes: all `DISCORD_*` lines removed from `~/.hermes/.env`, and its Discord state files
     (`gateway/discord_command_sync_state.json`, `cache/discord_capabilities.json`) deleted, and
     the stale Discord entries removed from `~/.hermes/channel_directory.json`. Every other
     platform in that file is kept; the file is replaced atomically with its mode and owner kept,
     and a missing or unreadable file is left as it is. This runs whichever runtime is active.
   - PicoClaw: `channel_list.discord` disabled in `config.json`, its settings removed from
     `.security.yml`.
   - Claude Code, Codex, OpenCode keep nothing outside `config.json`.
3. **Copies of runtime config** get the same clean-up, wherever they sit beside the live files:
   `openclaw.json.bak`, `openclaw.json.bak.N`, `openclaw.json.last-good` (any `openclaw.json.*`
   that is JSON), and the JSON files hermes archived when it imported from OpenClaw
   (`~/.hermes/migration/openclaw/<stamp>/archive/*.json`). Discord objects lose their
   credential keys (a `channels.discord` object is removed). Files that are not JSON are left as they are.
4. Live databases and history stores (`state.db`, `openclaw.sqlite`, the hermes curator backups)
   are never rewritten.
5. If the **active** runtime's file changed, that runtime is restarted once so a Discord session
   it had already opened closes.

Presync scripts for Hermes and PicoClaw also strip Discord settings on every runtime start.

## Behaviour notes

- If the cloud relay is not set up for the device's type, or the device is offline, Discord
  messages simply do not arrive; nothing on the device retries or polls Discord.
- Switching runtimes needs nothing: the relay talks to the runtime only through the generic
  chat entry points, and the channel reconcile re-applies "Discord handled by the relay" to the
  new runtime.
- Telegram, Slack, WhatsApp and iMessage are unchanged.


**People other than the owner.** Someone in `discord_guest_user_ids` who @mentions the bot in the connected server gets a short answer from a direct model call, limited to 10 an hour each. The message never reaches the agent, so a guest has no tools, memory, connected accounts or device control, and each message stands alone. It counts toward the owner's usage.

**Threads.** In a server channel the agent can begin a reply with `[[thread: title]]`. The device removes that text and sends `thread_message_id` and `thread_name` with `discord_reply`, and the reply goes in a new thread on the owner's message. If the thread cannot be opened, the reply stays in the channel.
