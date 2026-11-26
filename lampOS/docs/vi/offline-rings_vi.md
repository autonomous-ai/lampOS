# Hẹn giờ và lời nhắc khi mất mạng (`LAMP_OFFLINE_RINGS`, #8632)

Bản tiếng Việt của mục "Timers and reminders with no link" trong
[offline.md](../offline.md) và "With no link" trong
[schedule.md](../schedule.md). Mã nguồn là nguồn sự thật duy nhất; tài liệu
tiếng Anh là bản chính.

Tắt trừ khi đặt đúng `on`. Phát hiện trên lamp-06c1 ngày 25/11 (hàng kiểm
tra của #8632): một hẹn giờ đặt trước khi mất mạng 10 phút đến hạn lúc mạng
đang mất, im lặng 409 s, rồi chỉ reo 5 s sau khi mạng trở lại. Một lần reo
vốn là một lượt của mô hình, và cổng của nó cần một phiên đám mây sẵn sàng.

- **Hẹn giờ hoặc lời nhắc đến hạn vẫn reo đúng giờ:** khi mạng mất quá thời
  gian chờ 5 s (`bus_out::LINK_GRACE_US`) và không có phiên nào sẵn sàng, nó
  reo từ một đoạn âm thanh trong bộ nhớ đệm giọng nói: "Your timer is done."
  cho hẹn giờ, "You have a reminder." cho lời nhắc, bằng ngôn ngữ của chủ
  đèn, được tạo sẵn lúc khởi động khi còn mạng. Phát trọn vẹn thì mục đó được
  ghi là đã nói: khi mạng trở lại không nói lại gì về nó. Không có đoạn âm
  thanh nào được giữ thì nó chờ mạng như trước.
- **Quyết định của first builder (26/11, điểm 1): phương án C.** Lời nhắc khi
  mất mạng luôn reo bằng câu chung "You have a reminder.", không bao giờ bằng
  lời của chính nó. Phương án B (đọc lời nhắc bằng lời của nó, tạo sẵn khi
  còn mạng và xoá khi mục rời khỏi Schedule) là việc tiếp theo, với bước
  kiểm tra quyền riêng tư riêng, không nằm trong PR này.
- **Kiểm tra mạng trước khi reo (điểm 2):** ở tuyến thứ hai khi là giọng duy
  nhất của đèn, mạng chỉ được coi là mất sau khi một lượt thất bại vì mạng.
  Nên nếu không ai nói gì trong lúc mất mạng, lần reo trước đây được thử như
  một lượt của mô hình trước và reo trễ vài giây. Giờ đây, với công tắc bật:
  - Khoảng 10 s trước khi lần reo sớm nhất đến hạn
    (`offline_rings::PROBE_BEFORE_DUE_US`), giọng nói nhờ worker chạy **một
    lần** chính bước kiểm tra có sẵn "no service host answers"
    (`second_provider::link`): một kết nối TCP trần tới từng máy chủ dịch vụ,
    không gửi gì, chờ tối đa 4 s (`PROBE_WITHIN`).
  - Bỏ qua nếu một lượt đã hoàn tất, hoặc một lần kiểm tra đã thấy có máy
    chủ trả lời, trong vòng 60 s trước đó
    (`offline_rings::LINK_CONFIRMED_WITHIN_US`).
  - Không có lần reo nào sắp đến hạn thì không kiểm tra gì. Ngoài lần đó
    không có thăm dò định kỳ nào mới; khi mạng đã được coi là mất, các lần
    kiểm tra mỗi 5 s có sẵn chạy như trước để biết khi nào mạng trở lại.
  - Nếu kiểm tra thấy mạng mất (xong muộn nhất 6 s trước hạn), đến hạn mạng
    đã mất quá thời gian chờ 5 s, nên lần reo phát từ đoạn âm thanh đúng giờ.
  - Nếu socket điều khiển của worker đang đầy (worker kẹt trong một lời gọi
    chậm, #8204), yêu cầu kiểm tra chưa được gửi và được gửi lại ở vòng sau;
    giọng nói không bao giờ dừng vì việc này.
  - Để biết lần reo sớm nhất, tiến trình con Schedule của giọng nói bắt đầu
    một lượt đọc kho nhiều nhất mỗi giây một lần (`UPCOMING_WALK_EVERY_US`),
    mỗi nhịp (200 ms) một trang tối đa 8 mục, chỉ lấy id và thời gian, không
    bao giờ lấy lời, và báo cho giọng nói khi kết quả thay đổi
    (`FromWorker::Upcoming`). Đây là đọc cục bộ qua socket, không ra mạng.
- **Không có câu thứ hai cho một tiếng động ngắn:** một câu tiếp theo có dưới
  400 ms tiếng nói, kết thúc trong vòng 8 s sau câu báo mất mạng, không nhận
  thêm câu báo nào.

## Các giới hạn

| Giới hạn | Giá trị | Ở đâu |
|---|---|---|
| Tiếng nói dưới mức này thì câu tiếp theo khi mất mạng không có câu báo | 400 ms (`SCRAP_NO_LINE_MS`) | `offline_rings.rs` |
| Khoảng sau câu báo mất mạng áp dụng quy tắc đó | 8 s (`SCRAP_AFTER_LINE_US`) | `offline_rings.rs` |
| Mạng mất bao lâu thì lần reo phát từ đoạn âm thanh | 5 s (`LINK_GRACE_US`, có sẵn) | `bus_out.rs` |
| Kiểm tra mạng bao lâu trước hạn reo sớm nhất, một lần | 10 s (`PROBE_BEFORE_DUE_US`) | `offline_rings.rs` |
| Mạng đã được xác nhận trong khoảng này thì bỏ qua kiểm tra | 60 s (`LINK_CONFIRMED_WITHIN_US`) | `offline_rings.rs` |
| Bước kiểm tra chờ máy chủ trả lời tối đa | 4 s (`PROBE_WITHIN`, có sẵn) | `second_provider/link.rs` |
| Lượt đọc kho tìm lần reo sớm nhất bắt đầu nhiều nhất | mỗi 1 s (`UPCOMING_WALK_EVERY_US`) | `reminders/consumer.rs` |

## Chạy lại hàng kiểm tra (lamp-06c1)

Cần `LAMP_OFFLINE_RINGS=on` trong một drop-in thử nghiệm và bộ nhớ đệm giọng
nói đã được tạo khi còn mạng. Đạt khi: một hẹn giờ và một lời nhắc đến hạn
trong lúc mất mạng mỗi cái reo trong vòng 5 s sau hạn, một lần; không nói lại
gì sau khi mạng trở lại; 10 câu hỏi giữ giới hạn của chúng; không tiếng động
ngắn nào nhận câu báo thứ hai. Thêm trường hợp "không ai nói gì trong lúc mất
mạng": một hẹn giờ đến hạn sau hơn 60 s không có lượt nào, ghi lại thời gian
reo tính từ hạn (`late_by_ms` của `offline_ring`) và dòng nhật ký kiểm tra
mạng trước khi reo.
