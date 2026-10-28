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
   - `[ask] policy` chỉ là `nudge`, không bao giờ là lời chào, tin tức hay lời nhắc;
   - nói, tin nhắn, cái nhìn và mọi chủ đề khác đều bị từ chối.
   Mở rộng giới hạn này là quyết định của trưởng nhóm.
4. **Kiểm tra.** Thẻ được đánh dấu `made: here` (nhóm "Tạo trên đèn này"
   của `lamp-cloud`). Sau đó `lamp-skills pack` (#1698) kiểm tra thư mục:
   mọi kiểm tra của một lần nạp, kể cả quyền, và gói được đọc lại như khi
   cài. Một kỹ năng làm nhiều hơn những gì nó khai báo bị từ chối ở đó.
   - **Sửa một lần** (#2813 phần 2). Khi bước kiểm tra từ chối câu trả lời
     đầu tiên, việc được giao thêm một lần nữa cho cùng nơi nhận, trước khi
     nói gì. Bản giao lại là bản yêu cầu cũ, kèm các dòng của bước kiểm tra
     (thư mục được thay bằng `<folder>`, tối đa 600 byte) trước lời yêu cầu
     của người dùng, nên vẫn nằm trong 2.000 byte của phần việc. Chỉ khi
     yêu cầu từ ứng dụng (nếu có) vẫn đang được làm. Câu trả lời của lần
     sửa được kiểm tra một lần: giữ và nói ra, hoặc nói là không giữ. Không
     bao giờ sửa lần thứ hai (`repairs.json`, giữ trước khi giao, dưới một
     khóa, nên dù hai tiến trình cùng đọc cũng chỉ một lần được gửi). Nếu
     việc giao lại không thành, câu trả lời đầu tiên được nói là không giữ
     ngay. Chỉ lỗi của bước kiểm tra
     được sửa; sai định dạng hay xin dùng nhiều hơn đèn và đầu thì được nói
     là không giữ ngay.
5. **Giữ lại, không cài.** Gói nằm trong thư mục riêng của giọng nói,
   `/var/lib/lampOS/voice/made-skills` (0700, người dùng của giọng nói):
   - giữ tối đa 5, cái cũ nhất bị bỏ cùng gói của nó;
   - mỗi cái được giữ cùng SHA-256 và những gì nó xin dùng (`made.json`);
   - cùng một việc giao được đọc lại thì trả về cái đã giữ, không kiểm tra
     lại.
6. **Nói ra, và hỏi.** Đèn nói kỹ năng sẽ làm gì (mô tả trong thẻ), sẽ
   dùng gì ("đèn của nó và việc nó có đang nói hay không"), rồi hỏi đúng
   từng chữ: "Shall I put it on the lamp?"
   - Các tệp không bao giờ được đọc ra.
   - Một kỹ năng không được giữ thì được nói rõ lý do.
   - Lời hỏi được ghi cạnh gói (`offered_at_unix_ms` trong `made.json`)
     khi tin được nghe trọn ở đèn, hoặc gửi trong tin nhắn, không phải lúc
     đọc tin: đèn kể tin sau, vào lúc thích hợp. Lời hỏi cũng được ghi khi
     tin phát hết mọi từ nhưng có chỗ ngắt, không ai nói chen
     (`news_played_through`, `TaskNews::played_through`). Tin đó chưa tính
     là đã nghe nên vẫn được kể lại, nhưng câu hỏi đã được nói, nên một lời
     "có" vẫn được nhận (fourteen-P10 trên #2814). Lượt bị cắt ngang hoặc
     không có tiếng thì vẫn không tính là đã hỏi.
7. **Cài khi người dùng nói "có" thành tiếng** (quyết định của trưởng nhóm,
   20/10 08:37 trên #1711: lời đồng ý bằng giọng nói gắn với SHA-256, một
   ngoại lệ của quy định 18:33 chỉ cho kỹ năng tạo trên đèn; ứng dụng không
   phải xác nhận thêm). Khi người dùng đồng ý, mô hình gọi
   `put_on_made_skill`:
   - chỉ lời "có" nói thành tiếng mới tính; gõ chữ hay câu do đèn tự viết
     thì không cài gì;
   - chỉ một lời "có" rõ ràng (`known_faces::plain_agreement`): "No, not
     yet" mà mô hình hiểu nhầm là "có" thì không cài gì;
   - chỉ lời hỏi mới nhất, trong vòng 120 giây kể từ khi được nghe;
   - giọng nói chỉ gửi cho helper quản lý SHA-256 của kỹ năng đó
     (`{"install_made": SHA256}`), helper chỉ cài đúng các byte đó và kiểm
     lại giới hạn;
   - helper cũng tự kiểm tra lời hỏi theo đồng hồ của chính nó
     (`made_skill::waiting`): chỉ kỹ năng được hỏi gần nhất, trong vòng 120
     giây và chưa được cài, mới được cài, nên một SHA-256 đã lưu không bao
     giờ được cài về sau bằng bất kỳ đường nào; lời hỏi mới nhất được xét
     trước: khi nó đã được cài hoặc đồng hồ lùi về trước nó thì không có gì
     chờ, và một lời hỏi cũ hơn không bao giờ thế chỗ;
   - không có helper thì người dùng nghe là không cài được; helper không
     trả lời trong 8 giây thì nghe là đang cài;
   - không bao giờ chạy khi chưa bật `LAMP_MAKE_SKILL`.

**Còn mở, chờ trưởng nhóm:**
- Đăng nhập Harness trên đèn ở nhà.

Gỡ một kỹ năng tạo ra bằng giọng nói sẽ đến cùng với việc cài nó.
