# Giao việc: kỹ năng tạo bằng giọng nói

Trang này chỉ dịch phần mới về việc giao một kỹ năng để tạo. Toàn bộ tài
liệu giao việc là [tasks.md](../tasks.md), bằng tiếng Anh; nếu hai trang khác
nhau, bản tiếng Anh đúng.

## Kỹ năng tạo bằng giọng nói (#1685, tắt trừ khi `LAMP_MAKE_SKILL=on`)

Cờ `--tasks-make-skill` của worker, sau `--tasks-needs`, cho mô hình hàm
`make_skill(request)` ([skills_vi.md](skills_vi.md)).
- **Chuẩn bị.** Đi như của `start_agent_task`: `prepare`, rồi
  `confirm_agent_task` khi người dùng đồng ý, trong vòng 120 giây, như cùng
  một việc đang chờ. `text` của nó là bản hướng dẫn và yêu cầu (tối đa 180
  byte), trong giới hạn 1.000 byte của phần tasks. Bản hướng dẫn bắt đầu bằng
  "Write one lamp skill (lampOS lamp-skills)"; nhờ đó giọng nói nhận ra lại
  việc này.
- **Câu trả lời cho mô hình.** Mô hình được báo `"skill": <yêu cầu>`, không
  bao giờ là bản hướng dẫn, và `next` bảo nó đọc lại yêu cầu.
- **Khi việc xong.** Tin về việc (`task_news::read_once_with`) trích yêu cầu
  làm lời của việc đó, ở mọi trạng thái. Xong ở `idle` với một câu trả lời,
  câu trả lời được lấy làm hai tệp của kỹ năng, được giữ như skills_vi.md
  nói, và được nói riêng (không bao giờ cùng tin khác, không bao giờ đọc ra).
- **Khi tắt,** tin về việc như vậy giống mọi việc khác, có trích yêu cầu.
- **Nhật ký** ghi `a skill made by voice is kept, version VERSION, sha256 …`
  hoặc lý do không giữ, chỉ kèm mã thoát của `lamp-skills`: không bao giờ ghi
  các tệp, tên kỹ năng hay lời của `lamp-skills`, vốn có thể trích lời của tác tử.
- **Khi đặt lại đèn,** `/var/lib/lampOS/voice/made-skills` bị xóa cùng các kho
  khác của người dùng.
- **Khi tắt,** một lời gọi `make_skill` không làm gì.

### Lời nhờ có tên là một lần giao việc (tắt trừ khi `LAMP_TASKS_NAMED_ASK=on`, #1722)

Trên lamp-4ace ngày 2026-10-21, khi đã đọc được hai người trong đội (Claude
Code và Codex, #2297), những lời nhờ được phát như "Hey Lamp, ask Claude Code to
write a two-line poem about the rain." lại đi vào `not_for_me`, hoặc đèn tự trả
lời ("poetry's mine to write, no need to ask anyone"): mỗi lượt chạy chỉ 0 đến 3
trên 3. Người dùng đã nêu tên ai sẽ làm, vậy mà đèn vẫn từ chối.

Với `LAMP_TASKS_NAMED_ASK=on` (voice chỉ chuyển nó cho worker của tuyến thứ
hai, qua `second_route_worker_environment`, và worker đó đọc nó; khi phần tasks
bật): lượt
chat đầu tiên của một câu nói có tin nhắn cuối là lời người dùng, và lời đó nhờ
đích danh một người trong đội làm việc gì, sẽ được gửi kèm
`tool_choice: {"type": "function", "function": {"name": "start_agent_task"}}`
(`agent_tasks::named_ask`, `second_provider::worker::named_asks`,
`net::Chat::forcing`). Mô hình vẫn tự viết nội dung việc và `who`, nhưng không
thể từ chối giao việc hay tự làm. Lượt sau một lần gọi kết thúc bằng kết quả
của lần gọi đó, và được hỏi như trước.

- **Là lời nhờ:** "ask", "tell", "get" hoặc "have" đứng ngay trước tên đầy đủ
  của agent, theo sau là "to" ("ask Claude Code to …", "tell Codex to …"), hoặc
  "can", "could", "would", "will" + "you ask/get <tên> …".
- **Không phải lời nhờ:** khi chỉ nhắc đến ("I told Claude Code yesterday",
  "is Codex done?", "what did Claude Code say?", "did you ask Codex to …",
  "tell Claude Code I'm late"); một lời phủ định ngay trước động từ ("don't
  ask Claude Code to …", "do not tell …", "never ask Codex to …", "not get …");
  một câu người dùng tự hỏi mình ("should I ask Claude Code to …", "shall I …",
  "do I …"); khi tên không thuộc đội ("ask Devin to …" lúc
  Devin chưa được cài); khi tên bị nghe sai ("Claude Co", "cloud code"; ghi chú
  của đội vẫn nhận ra các tên này). Khi chưa đọc được ai trong đội, chỉ nhận
  Claude Code.
- **Đánh đổi:** các quy tắc có thể cho qua một câu nói với người khác (trong
  phòng yên tĩnh mà không có `LAMP_SPOKEN_TO`, hoặc như một cuộc trò chuyện
  đang diễn ra). Hiện nay mô hình vẫn có thể nói câu đó không dành cho đèn
  (`not_for_me`). Khi câu đó nêu tên một agent ("…, ask Claude Code to fix the
  build"), việc ép gọi lấy mất lựa chọn đó: nó thành một lần giao việc được
  chuẩn bị sẵn và một câu hỏi cho cả phòng. Tiếng "có" nghe được vẫn canh lần
  bắt đầu: `start_agent_task` chỉ chuẩn bị, và không có gì chạy nếu không có
  một tiếng "có" nghe được sau đó trong vòng 120 giây.
- **Khi tắt:** yêu cầu gửi đi giống hệt như trước, từng chữ (có test so sánh
  nội dung). `tool_choice` không bao giờ được gửi kèm các hàm có tên của một
  câu bị giữ lại.
- **Relay:** đã kiểm tra trên lamp-06c1 ngày 2026-10-21 lúc 03:4x: với câu
  "What's two plus two?", chat DeepSeek qua relay (tắt chế độ suy nghĩ) gọi
  đúng hàm mà `tool_choice` chỉ định 3 trên 3 lần, và 0 trên 3 lần khi không có
  `tool_choice`.

**Probe** (`cargo run -p lamp-voice --example probe_handover`, chạy ở nơi voice
chạy, bằng quyền root, với các công tắc tasks của unit voice): mỗi lời nhờ đã
phát được gửi như lượt đầu của một câu nói, kèm đúng system message, ghi chú
của đội, ghi chú gọi tên, các hàm và client chat mà worker thứ hai dựng; hai
người trong đội được giữ sẵn như cách phần tasks sẽ trả lời. Với
`PROBE_HANDOVER_HISTORY=on`, mỗi lượt chạy trước hết nhờ "tell me a story about
a clever fox, at least eight sentences" và lưu câu trả lời thật vào thread như
một lượt trao đổi. Probe chỉ in số đếm. Trên lamp-06c1 ngày 2026-10-21, mỗi câu
10 lần:

| Cách viết | Kể chuyện trước | Thơ mưa | Thơ biển | Haiku (Codex) |
|---|---|---|---|---|
| A (ghi chú hiện tại) | không | 10/10 | 10/10 | 10/10 |
| B (#2337, đã revert) | không | 10/10 | 9/10 (1 tự làm) | 10/10 |
| C (quy tắc lên đầu) | không | 10/10 | 10/10 | 10/10 |
| A | có | 10/10 | 10/10 | 10/10 |
| B | có | 10/10 | 10/10 | 9/10 (1 tự làm) |
| C | có | 10/10 | 10/10 | 10/10 |
| D (A, `LAMP_TASKS_NAMED_ASK=on`) | có | 10/10 | 10/10 | 10/10 |

Như vậy probe chưa tái hiện được kết quả 0 trên 3 của lamp-4ace: những gì lượt
chạy thật gửi thêm (ghi chú của bộ nhớ, các dòng của skill, các hàm tìm kiếm
web, tên vừa nghe thấy) không có trong probe. D giúp việc giao việc không còn
phụ thuộc vào những thứ đó. Chưa được phát thử trên đèn.
