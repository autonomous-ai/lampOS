# Claude Code runtime — work log

Parity pass 2026-07-07 against the hardened codex runtime (`runtimes/codex/PROGRESS.md`).
This file tracks only what was verified/changed in that pass — it does not restate the
original `feature/claude-code` branch history.

## Progress

- [x] Stubs honesty: RefreshModelsConfig / UpdatePrimaryModel / CompactSession return
  `domain.ErrNotSupportedByRuntime` (never bare nil) — the EnsureOnboarding-presync
  fallback applies model changes from config.json, so callers still converge.
- [x] Glue wired: HAL enums / orchestrator / config plumbing for the claudecode runtime.
- [x] Info uplink: `claudecode_version` reported + logs source (journal mapping) added.
- [x] Persona determinism verified: `ensureClaudeMDBlock` re-asserts the OS block
  (with `@SOUL.md`/`@IDENTITY.md`/... imports) on every `EnsureOnboarding` run —
  exact-block fast path, stale-block strip, re-inject. `UpdateIdentityName` rewrites
  IDENTITY.md only; no CLAUDE.md refresh needed (@imports re-read at session start).
- [x] Channels honesty verified: telegram DEVICE-OWNED (`telegram_poll.go`,
  codex mirror — the native plugin proved undebuggable in the field: no
  journal logs, silent allowlist drops, silent death on restart races;
  presync removes stale `~/.claude/channels` state so no plugin ever competes
  for getUpdates); discord ALSO device-owned since the same date (`discord.go`
  discordgo session — see the dedicated entry below; GetConfiguredChannel
  checks DiscordBotToken); whatsapp → `domain.ErrChannelNotSupported`; stale
  stubs.go comment (claimed discord unsupported) fixed.
- [x] Bridge ported to Go: bridge.py → `runtimes/claudecode/gatewayd` (`os-server
      claudecode-gatewayd` subcommand, codex-gatewayd file layout); presync no longer
  materializes bridge.py; install.sh drops the python3/websockets prereqs; unit
  ExecStart=/usr/local/bin/os-server claudecode-gatewayd (install.sh + gateway_unit.go).
- [x] Slack inbound (device-owned, HTTP mode): mirror of runtimes/codex/slack.go —
  `domain.SlackBridge` on ClaudeCodeService (slack.go + slack_sender.go), bff-proxy
  → MQTT slack_event path, allowlist `slack_user_id`, busy-wait inject (silent run,
  eyes ack), reply via chat.postMessage on the `result` event (emitFinal →
  finishSlackTurn, stripForChannel ported to hal.go); SlackSender proactive path;
  slack AddChannel/Refresh = honest no-op (creds read live); no streaming. Tests
  mirror codex slack_test.go. NOT device-verified.
- [x] Device-run fixes 2026-07-07 (chat pipeline DEVICE-VERIFIED on lamp .93):
  (a) `IS_SANDBOX=1` asserted in gatewayd child env — claude refuses
  `--dangerously-skip-permissions` under uid 0 without it (device runs as
  root); (b) presync strips a trailing `/v1` off `llm_base_url` before
  writing `ANTHROPIC_BASE_URL` (claude appends `/v1/messages` itself —
  unstripped base hit `/v1/v1/messages` → 404 surfaced as "model Auto-AI may
  not exist"); (c) presync writes `ANTHROPIC_API_KEY` ONLY — campaign-api
  401s the `Authorization: Bearer` form and claude prefers
  `ANTHROPIC_AUTH_TOKEN` when both are set; (d) translator emitFinal now
  emits the whole reply as one assistant delta before chat.final (codex
  contract) — without it the shared consumer had nothing to flush at
  lifecycle.end: no TTS, no tts_send for web chat / Flow Monitor.
- [x] Telegram moved to DEVICE-OWNED (2026-07-07): `telegram_poll.go` +
  `telegramRuns` tracker + emitFinal/handleError reply routing + typing
  keeper, 1:1 codex mirror (hermetic test ported); presync/install.sh drop
  the telegram plugin and presync removes `~/.claude/channels/telegram/`
  (getUpdates 409 guard); AddChannel/Refresh telegram → honest no-op (creds
  read live per poll). NOT device-verified (deploy pending — device offline
  mid-session).
- [x] ~~Discord device-owned~~ — SUPERSEDED: Discord now goes through the cloud relay (`discord.go` removed; see docs/discord-relay.md). Original entry (2026-07-07, same session as telegram):
  `discord.go` — discordgo gateway session (DM + guild + message-content
  intents), accept filter (allowlisted `discord_user_id`, DM or
  @mention-in-`discord_guild_id`, mention stripped), busy-wait inject (flow
  source `discord`, silent run, native typing keeper), emitFinal reply
  chunked at the 2000-char limit — 1:1 codex mirror, tests ported. With no
  plugin channels left: presync §3 reduced to `rm -rf ~/.claude/channels` +
  CLAUDECODE_CHANNELS no longer written (gatewayd passthrough kept, unused);
  install.sh drops bun + plugin marketplace entirely; AddChannel/Refresh are
  honest no-ops for all three channels (creds read live). NOT device-verified.
