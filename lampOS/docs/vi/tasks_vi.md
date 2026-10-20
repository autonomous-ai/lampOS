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

## Nhiều tác tử: nhờ đèn giao việc, không phải nói với người khác (#1722)

Trên lamp-4ace lúc 00:06 ngày 2026-10-21, khi luồng thứ hai đã đọc hai tác tử
(Claude Code và Codex), chỉ 1 trong 3 lần giao việc được chuẩn bị (trước đó là
3 trong 3): "ask Claude Code to …" bị trả lời bằng `not_for_me`, hoặc đèn nói
đó không phải việc của nó. Văn bản cho hai tác tử nói "Claude Code and Codex"
cùng nhau, "one of the person's agents" (nghe như một người), "it is theirs to
do, not yours", và "neither Claude Code or Codex nor", một câu sai ngữ pháp.

Khi đọc được từ hai tác tử trở lên:
- `start_agent_task` nói "asks to have Claude Code or Codex do something";
- ghi chú của phiên mở đầu bằng "Claude Code and Codex are the person's agents
  on their office computer, never anyone in the room: … they are asking you to
  hand it over, not talking to someone else, so never call not_for_me for it.";
  ngoại lệ của nó nói "names neither Claude Code nor Codex nor …".

Khi chưa đọc tác tử nào hoặc chỉ có một, cả hai văn bản giữ nguyên từng byte.
Đánh đổi: "ask Claude Code to …" nói giữa hai người trong phòng sẽ được coi là
giao việc, và đèn vẫn hỏi trước khi bắt đầu (`LAMP_TASKS_ASKED`). Bật cùng
`LAMP_TASKS_WHO=on`, không có công tắc riêng. Chưa chạy thử trên đèn.
