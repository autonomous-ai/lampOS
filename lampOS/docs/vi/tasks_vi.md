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
- **Nhật ký** ghi `a skill made by voice is kept, NAME VERSION, sha256 …`
  hoặc lý do không giữ, không bao giờ ghi các tệp.
