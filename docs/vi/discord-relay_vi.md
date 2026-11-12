# Discord qua cloud relay

Tin nhắn Discord đến thiết bị, và câu trả lời rời thiết bị, đều đi qua cloud bằng MQTT. Thiết bị
**không giữ bot token Discord** và **không mở kết nối nào tới Discord**: cả os-server lẫn mọi agent
runtime (OpenClaw, Hermes, Claude Code, Codex, OpenCode, PicoClaw) đều không bao giờ thấy token.

```
Discord ──▶ cloud relay ──▶ MQTT fa_channel  {cmd:"discord_event", …}      ──▶ os-server ──▶ agent
Discord ◀── cloud relay ◀── MQTT fd_channel  {type:"discord_reply", …}     ◀── os-server ◀── agent
```

Code: `system/discordrelay/` (relay), `system/server/device/delivery/mqtt/discord_event_handler.go`
(MQTT vào/ra), `system/server/agent/delivery/http/handler_event_agent.go` (nhận câu trả lời đã
xong), `system/agent/discord_relay.go` (dọn dẹp khi khởi động).

## Setup

Discord được cấu hình như mọi channel khác: `channel: "discord"` trong `POST /api/device/setup`,
`POST /api/device/channel`, body Wi-Fi provisioning, trang Settings cục bộ, hoặc lệnh MQTT
`add_channel`. Thiết bị chỉ lưu ai được phép nói chuyện với nó:

| Field | Ý nghĩa |
|-------|---------|
| `discord_user_id` | **Bắt buộc.** Tài khoản Discord duy nhất mà thiết bị lắng nghe. |
| `discord_guild_id` | Tuỳ chọn. Server duy nhất được tính; không có thì chỉ nhận tin nhắn riêng (DM). |
| `discord_managed` | Được chấp nhận và lưu (`true`). Chế độ relay là chế độ duy nhất, nên request không gửi field này vẫn hành xử như nhau. |

`discord_bot_token` (hoặc `config.bot_token` của MQTT) trong bất kỳ request nào đều **bị bỏ qua**:
không lưu, không ghi vào config của runtime, không log. Trang setup không còn hỏi token. Lệnh MQTT
`add_channel` cho Discord đọc `config.managed`, `config.guild_id`, `config.user_id`.

`config.json` giữ `discord_managed`, `discord_guild_id`, `discord_user_id`.

## Tin nhắn xuống: `discord_event`

```json
{"cmd":"discord_event","event_id":"…","discord_user_id":"…","guild_id":"…",
 "discord_channel_id":"…","author_username":"…","bot_user_id":"…",
 "mentions_bot":true,"text":"…"}
```

os-server chỉ nhận tin nhắn khi thoả tất cả điều kiện sau; ngược lại bỏ (và log lý do, không log
nội dung):

- Thiết bị đã setup Discord (`discord_managed` và có `discord_user_id`).
- Người gửi là `discord_user_id`.
- Tin nhắn riêng (`guild_id` rỗng) luôn hợp lệ. Tin nhắn trong server phải đến từ
  `discord_guild_id` **và** mention bot; phần mention bị gỡ khỏi nội dung.
- `event_id` chưa gặp trong 5 phút gần nhất (relay có thể gửi lại).

Tin nhắn được nhận trở thành một lượt chat bình thường trên runtime đang chạy, bất kể runtime nào:

```
[discord] Message from <username> [id:<user id>]:
<text>
[via:discord]
```

Tin chờ (tối đa 2 phút) cho agent xong lượt hiện tại rồi mới gửi. Run được đánh dấu silent nên câu
trả lời **không** được đọc thành tiếng. Tối đa 8 tin có thể đang chờ cùng lúc.

Một lượt Discord không bao giờ bị gộp với tin nhắn khác. Các runtime có thể gộp tin mới vào lượt
đang chạy (Hermes native steering, Codex `turn/steer`) trả cùng một câu trả lời cuối cho mọi tin
được gộp, nên gộp sẽ đưa cuộc trò chuyện trong phòng vào kênh Discord, hoặc đưa câu trả lời Discord
ra loa. Vì vậy:

- Giọng nói, giọng nói ambient và chat web/điện thoại đến trong lúc một lượt Discord đang chạy sẽ
  đợi lượt đó kết thúc, rồi chạy theo thứ tự thành lượt riêng.
- Tin Discord đến trong lúc một lượt khác đang chạy cũng đợi như vậy, rồi chạy thành lượt riêng.
- Hai tin Discord không bao giờ dùng chung một lượt, dù đến từ kênh nào, nên câu trả lời không bao
  giờ sang kênh khác.
- Tin cùng đích đến vẫn được gộp như trước: giọng nói vào lượt giọng nói được steer, không thêm
  độ trễ.

Lượt Discord lỗi hoặc bị huỷ kết thúc như mọi lượt khác: tín hiệu typing dừng và các tin đang đợi
phía sau được chạy. Không tin nào đang đợi bị bỏ âm thầm: tin mà runtime không thể giữ đợi (Codex
giữ tối đa 8 tin, không giữ lâu hơn thời gian tối đa của một lượt) bị từ chối và kết thúc với lỗi.

Không có message xác nhận cho `discord_event`.

## Tin nhắn lên: `discord_reply`, `discord_typing`

Cả hai dùng envelope thiết bị chuẩn (`device`, `type`, `id`, `version`, `mac`, `time`, …) cộng với
channel mà lượt chat xuất phát:

```json
{"type":"discord_reply","discord_channel_id":"…","text":"…"}
{"type":"discord_typing","discord_channel_id":"…"}
```

- **Typing** gửi khi tin được nhận và mỗi 8 giây cho tới khi lượt chat kết thúc (tối đa 10 phút).
- **Reply** gửi một lần khi lượt chat xong, với cùng đoạn text đã làm sạch mà mọi channel khác nhận
  (đã gỡ marker phần cứng và tag). Lượt không có gì để nói (`NO_REPLY`, chỉ hành động phần cứng,
  hoặc lỗi) không gửi reply và dừng typing. Cloud tự chia reply dài cho vừa giới hạn của Discord.

Channel id luôn lấy từ `discord_event` đã khởi động lượt chat; agent không thể chọn channel khác.

## Tin nhắn được niêm phong (sealed)

Cloud có thể gửi `discord_event` ở dạng niêm phong, để không gì nằm giữa cloud và thiết bị đọc
được nội dung hay các Discord id (`system/discordrelay/seal.go`):

```json
{"cmd":"discord_event","enc":"v1","sealed":"<base64>"}
```

- Khoá: HMAC-SHA256 của `discord-relay/v1` với khoá là API key cloud của thiết bị. Mỗi thiết bị
  có khoá riêng, không lưu thêm gì.
- Mã hoá: AES-256-GCM, nonce ngẫu nhiên 12 byte, `sealed` = base64(nonce ‖ ciphertext). Dữ liệu
  bổ sung là `<kind>|<device id>`, nên tin niêm phong cho thiết bị này hoặc loại tin này không mở
  được thành thiết bị hay loại tin khác.
- Nội dung bên trong là JSON mà tin nhắn lẽ ra mang ở dạng rõ, cộng `ts` (unix giây). Tin niêm
  phong cũ hơn (hoặc sớm hơn) 15 phút bị bỏ.

Thiết bị mở tin niêm phong và niêm phong `discord_reply`, `discord_typing` của lượt đó theo cùng
cách; các trường định tuyến của envelope vẫn ở dạng rõ, còn `discord_channel_id` và `text` nằm
trong `sealed`. Tin đến ở dạng rõ thì được trả lời ở dạng rõ. Tin niêm phong không mở được bị bỏ
và ghi log không kèm nội dung. Cloud quyết định thiết bị nào nhận tin niêm phong; thiết bị không
có cài đặt cho việc này.

## Nâng cấp thiết bị đang giữ token

Khi khởi động, thiết bị tự chuyển sang chế độ relay:

1. **config.json.** Khi nạp mà thấy `discord_bot_token` (hoặc `discord_app_token`), đặt
   `discord_managed: true`, giữ guild id và user id, rồi ghi lại file không còn token. Struct config
   không có field cho token nên không gì có thể ghi lại token.
2. **File riêng của mọi runtime** (`system/agent/discord_relay.go`, bước đầu của chuỗi khởi động,
   chạy mỗi lần boot, không làm gì khi đã sạch):
   - OpenClaw: gỡ `channels.discord` khỏi `openclaw.json`, tắt entry plugin discord.
   - Hermes: gỡ mọi dòng `DISCORD_*` khỏi `~/.hermes/.env`, và xoá các file trạng thái Discord
     (`gateway/discord_command_sync_state.json`, `cache/discord_capabilities.json`), và gỡ các
     entry Discord cũ khỏi `~/.hermes/channel_directory.json`. Mọi nền tảng khác trong file được
     giữ nguyên; file được thay nguyên tử, giữ mode và owner, và file thiếu hoặc không đọc được thì
     để nguyên. Bước này chạy bất kể runtime nào đang hoạt động.
   - PicoClaw: tắt `channel_list.discord` trong `config.json`, gỡ settings khỏi `.security.yml`.
   - Claude Code, Codex, OpenCode không giữ gì ngoài `config.json`.
3. **Bản sao config của runtime** được dọn tương tự, ở bất cứ đâu cạnh file chính:
   `openclaw.json.bak`, `openclaw.json.bak.N`, `openclaw.json.last-good` (mọi `openclaw.json.*`
   là JSON), và các file JSON hermes lưu trữ khi import từ OpenClaw
   (`~/.hermes/migration/openclaw/<stamp>/archive/*.json`). Object Discord bị gỡ các key credential
   (object `channels.discord` bị xoá). File không phải JSON được để nguyên.
4. Database đang chạy và kho lịch sử (`state.db`, `openclaw.sqlite`, curator backups của hermes)
   không bao giờ bị ghi lại.
5. Nếu file của runtime **đang chạy** thay đổi, runtime đó được restart một lần để session Discord
   đã mở (nếu có) đóng lại.

Script presync của Hermes và PicoClaw cũng gỡ cấu hình Discord mỗi lần runtime khởi động.

## Ghi chú hành vi

- Nếu cloud relay chưa được bật cho loại thiết bị, hoặc thiết bị offline, tin nhắn Discord đơn giản
  là không đến; thiết bị không retry hay poll Discord.
- Đổi runtime không cần làm gì thêm: relay chỉ nói chuyện với runtime qua các entry point chat chung,
  và channel reconcile áp lại "Discord do relay xử lý" cho runtime mới.
- Telegram, Slack, WhatsApp và iMessage không đổi.
