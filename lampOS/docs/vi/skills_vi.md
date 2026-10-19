# Kỹ năng vật lý: tạo bằng giọng nói

Trang này chỉ dịch phần mới về kỹ năng tạo bằng giọng nói. Toàn bộ thiết
kế kỹ năng là [skills.md](../skills.md), bằng tiếng Anh; nếu hai trang khác
nhau, bản tiếng Anh đúng.

## Tạo bằng giọng nói (#1685, tắt trừ khi `LAMP_MAKE_SKILL=on`)

"Lamp, tạo cho tôi một kỹ năng làm đèn dịu đi lúc 10 giờ tối." Khi bật,
mô hình có hàm `make_skill(request)` (`crates/voice/src/made_skill.rs`).
Diễn ra như sau:

1. **Giao việc như mọi việc khác.** Giọng nói chuẩn bị một lần giao việc
   cho phần tasks ([tasks_vi.md](tasks_vi.md)). Nội dung là bản hướng dẫn,
   rồi đến yêu cầu của người dùng:
   - Bản hướng dẫn là chữ cố định: định dạng kỹ năng và những gì một kỹ
     năng tạo ra được phép dùng. Vì thế tác tử trên máy tính không cần bộ
     tạo kỹ năng riêng.
   - Yêu cầu tối đa 180 byte; dài hơn thì bị từ chối, không bao giờ bị cắt.
   - Không có gì khác của cuộc trò chuyện đi kèm.
   - Mô hình chỉ đọc lại yêu cầu, không đọc bản hướng dẫn, và chỉ giao việc
     (`confirm_agent_task`) khi người dùng nói đồng ý. Tác tử là tác tử mà
     việc giao đang dùng.
2. **Trả về hai tệp.** Khi việc xong, câu trả lời phải đúng hai khối có rào,
   `skill.toml` và `SKILL.md`, mỗi khối tối đa 8 KiB. Các tệp về qua chính
   kênh của việc giao (câu trả lời của Harness); không có đường truyền nào
   khác.
3. **Giới hạn những gì kỹ năng tạo ra được dùng** (giới hạn của điều phối
   viên, 19/10):
   - `kind` là installable, có phiên bản;
   - `[permissions]` chỉ có `light`, `body`, và `topics` trong số
     `voice/state` và `room/presence`;
   - không có hàm cho mô hình (`tool`);
   - nói, tin nhắn, cái nhìn và mọi chủ đề khác đều bị từ chối.
   Mở rộng giới hạn này là quyết định của trưởng nhóm.
4. **Kiểm tra.** Thẻ được đánh dấu `made: here` (nhóm "Tạo trên đèn này"
   của `lamp-cloud`). Sau đó `lamp-skills pack` (#1698) kiểm tra thư mục:
   mọi kiểm tra của một lần nạp, kể cả quyền, và gói được đọc lại như khi
   cài. Một kỹ năng làm nhiều hơn những gì nó khai báo bị từ chối ở đó.
5. **Giữ lại, không cài.** Gói nằm trong thư mục riêng của giọng nói,
   `/var/lib/lampOS/voice/made-skills` (0700, người dùng của giọng nói):
   - giữ tối đa 5, cái cũ nhất bị bỏ cùng gói của nó;
   - mỗi cái được giữ cùng SHA-256 và những gì nó xin dùng (`made.json`);
   - cùng một việc giao được đọc lại thì trả về cái đã giữ, không kiểm tra
     lại.
6. **Nói ra.** Đèn nói kỹ năng sẽ làm gì (mô tả trong thẻ), sẽ dùng gì
   ("đèn của nó và việc nó có đang nói hay không"), và nói đúng từng chữ:
   "I've made it; putting it on needs a yes from the team first."
   - Các tệp không bao giờ được đọc ra.
   - Một kỹ năng không được giữ thì được nói rõ lý do.

**Còn mở, chờ trưởng nhóm:**
- **Cài một kỹ năng tạo ra lên đèn.** Một gói chưa ký do tác tử tạo đi
  ngược với quyết định 18:33, "chỉ những gói đã ký từ kênh cập nhật". SHA-256
  được giữ là thứ một lời đồng ý như vậy sẽ gắn vào, nên những gì người dùng
  nghe là những gì sẽ được cài, và không gì có thể mở rộng sau đó.
- Lời đồng ý bằng giọng nói đã đủ chưa, hay ứng dụng cũng phải xác nhận.
- Đăng nhập Harness trên đèn ở nhà.

Gỡ một kỹ năng tạo ra bằng giọng nói sẽ đến cùng với việc cài nó.
