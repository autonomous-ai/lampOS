# Hẹn giờ và lời nhắc khi mất mạng (`LAMP_OFFLINE_RINGS`, #8632, #8708)

Bản tiếng Việt của mục "Timers and reminders with no link" trong
[offline.md](../offline.md) và "With no link" trong
[schedule.md](../schedule.md). Mã nguồn là nguồn sự thật duy nhất; tài liệu
tiếng Anh là bản chính.

Tắt trừ khi đặt đúng `on`; bật trong lamp-voice của sản phẩm từ lần bật
của #8708, được đọc bởi giọng nói và tiến trình con Schedule của nó (dùng
chung môi trường của unit). Phát hiện trên lamp-06c1 ngày 25/11 (hàng kiểm
tra của #8632): một hẹn giờ đặt trước khi mất mạng 10 phút đến hạn lúc mạng
đang mất, im lặng 409 s, rồi chỉ reo 5 s sau khi mạng trở lại. Một lần reo
vốn là một lượt của mô hình, và cổng của nó cần một phiên đám mây sẵn sàng.

- **Hẹn giờ hoặc lời nhắc đến hạn vẫn reo đúng giờ:** khi mạng mất quá thời
  gian chờ 5 s (`bus_out::LINK_GRACE_US`) và không có phiên nào sẵn sàng. Hẹn
  giờ reo "Your timer is done." từ bộ nhớ đệm giọng nói, bằng ngôn ngữ của
  chủ đèn, được tạo sẵn lúc khởi động khi còn mạng; phát trọn vẹn thì được
  ghi là đã nói.
- **Lời nhắc reo bằng chính lời của nó (#8708):** khi còn mạng, tiến trình
  con Schedule (tiến trình duy nhất vốn đã đọc lời của lời nhắc) tạo cho mỗi
  lời nhắc (không bao giờ cho hẹn giờ) một đoạn âm thanh riêng: "You have a
  reminder." và lời của nó, qua dịch vụ giọng nói của tuyến thứ hai
  (`TtsCache::render_once`), trên một luồng riêng, không bao giờ trong nhịp
  200 ms. Gói tin gửi cho giọng nói vẫn chỉ có id và thời gian, không bao giờ
  có lời. Đoạn âm thanh lưu ở
  `/var/lib/lampOS/voice/schedule-delivery/ring-clips/<store_id>-<id>.wav`
  (0700, trong thư mục giao nhắc của consumer), kèm một tệp
  `.json` giữ mã băm SHA-256 của lời và `voice|model|language`; một trong hai
  đổi thì được tạo lại. Khi mất mạng, giọng nói đọc đoạn âm thanh theo id và
  kiểm tra nó với lời của mục đến hạn: đúng thì phát; nhiều lời nhắc trễ gộp
  vào một lần reo (`Offer::also`) phát lần lượt từng đoạn.
- **Dọn dẹp:** cuối mỗi lượt đọc kho (khoảng mỗi giây), đoạn âm thanh có id
  không còn trong Schedule bị xoá (đã nói, bị huỷ, id cũ của một lần sửa,
  người đó bị `Forget`), và mọi đoạn của một `store_id` khác. Khi công tắc
  tắt, mọi đoạn âm thanh bị xoá lúc giọng nói hoặc tiến trình con Schedule
  của nó khởi động (#8894). Tạo thất bại
  (mất mạng) thì thử lại sau 60 s, rồi gấp đôi mỗi lần, tối đa mỗi giờ một
  lần.
- **Không có đoạn âm thanh riêng** (đặt khi mất mạng, hoặc tạo thất bại):
  phát câu chung "You have a reminder.", và mục đó được đánh dấu "đã reo,
  chưa nói" trong sổ giao của tiến trình con (`delivery::SCHEMA` 3, còn giữ
  sau khi khởi động lại). Nó không reo lại khi mất mạng, và khi mạng trở lại
  nó được nói bằng lời của nó một lần, như một lượt của mô hình; rồi dấu đó
  bị xoá. Trong lúc chờ, nó không chặn gì: tiến trình con bỏ qua nó, nên một
  hẹn giờ hay lời nhắc đến hạn sau trong cùng lần mất mạng vẫn reo đúng giờ,
  và không gì được gộp vào nó; giọng nói báo cho tiến trình con khi mạng trở
  lại (`LinkBack`, P2 của Reviewer-lead trên #8800). Đây là điểm 5 của first
  builder (c7183858037), sửa lỗ hổng đã biết của #8632. Không có đoạn âm thanh nào cả thì nó chờ mạng như trước.
- **Giới hạn dung lượng (điểm thêm 2 của first builder):** thư mục giữ tối
  đa 122.918.400 byte (`ring_clips::MOST_BYTES`, khoảng 123 MB): một đoạn âm
  thanh và tệp kèm cho mỗi mục trong tối đa 128 mục của Schedule, mỗi đoạn
  dài nhất 20 s ở 24 kHz 16-bit (960.044 byte). 300 byte lời cộng câu mở đầu
  chạm mức 20 s ở tốc độ nói (khoảng 15 ký tự mỗi giây), nên đó là trường hợp
  xấu nhất thật (ở 16 kHz sẽ khoảng 82 MB). Quá mức này thì không tạo thêm,
  và nhật ký ghi một dòng.
- **Khôi phục cài đặt gốc xoá chúng (điểm thêm 1):** các đoạn âm thanh là lời
  nhắc của người dùng dưới dạng âm thanh, nên chúng nằm trong
  `/var/lib/lampOS/voice/schedule-delivery`, thư mục mà kế hoạch reset của sản
  phẩm xoá; chúng không chiếm đường dẫn riêng trong kế hoạch, vì kế hoạch đã
  đủ 64 đường dẫn (#7771).
- **Thư mục cũ bị xoá khi giọng nói khởi động (các điểm của Reviewer-lead trên
  #8671):** đèn chạy bản dựng giữa #8906 và #8671 giữ đoạn âm thanh ở
  `/var/lib/lampOS/voice/ring-clips` (`ring_clips::OLD_DIR`), không đường dẫn
  nào của kế hoạch reset bao gồm. Mỗi lần giọng nói phục vụ khởi động, nó xoá
  các tệp thường và symlink trong đó (symlink bị xoá như một liên kết, không
  bao giờ đi theo), rồi xoá thư mục bằng `remove_dir` thường, và nhật ký ghi
  một dòng `lamp-voice: the old offline ring clips folder was removed (N files)`.
  Mục khác (một thư mục) được giữ lại cùng thư mục cũ, và nhật ký ghi
  `... was left: not empty (N files removed)`. Không có thư mục cũ thì không
  làm gì và không ghi gì; bước này không bao giờ động đến thư mục mới. Một
  lần khôi phục cài đặt gốc chạy trước lần khởi động đầu của giọng nói mới để
  lại thư mục cũ; lần khởi động tiếp theo của giọng nói xoá nó. Mã này PARKED (#8906,
  #8671): bỏ khi không đèn nào còn có thể chạy bản dựng giữa hai PR đó.
- **Tắt công tắc thì mọi đoạn âm thanh bị xoá (#8894, P3 2 của Reviewer-lead
  trên #8800):** khi công tắc tắt, mọi đoạn âm thanh bị xoá lúc giọng nói
  hoặc tiến trình con Schedule của nó khởi động. Tiến trình con xoá mọi tệp
  trong thư mục khi khởi động; nếu tắt nói của Schedule thì tiến trình con
  không bao giờ chạy, nên chính giọng nói xoá lúc khởi động. Tắt công tắc cần
  khởi động lại, nên đoạn âm thanh của một lời nhắc đã xoá (đã nói, bị huỷ,
  bị sửa, hoặc người đó bị `Forget`) không bao giờ còn lại. Mỗi lần khởi động
  chỉ đọc thư mục một lần, ghi một dòng nhật ký nếu có tệp bị xoá, không tạo
  gì và không dùng mạng. Khôi phục cài đặt gốc vẫn xoá hết.
- **Mỗi vòng không đọc đĩa vô ích (P3 1 của Twelve trên #8687):** chưa thấy
  phiên nào thì không đọc đoạn âm thanh nào; một mục đã thấy đang chờ thì
  không đọc lại ở các vòng sau, và việc chờ chỉ được ghi nhật ký một lần.
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
    giọng nói không bao giờ dừng vì việc này. Khi các lần gia hạn quyền của
    worker đang bị bỏ qua (đang kẹt), cũng không gửi; chỉ gửi khi một lần gia
    hạn được nhận lại (P3 2 của Twelve trên #8687).
  - Để biết lần reo sớm nhất, tiến trình con Schedule của giọng nói bắt đầu
    một lượt đọc kho nhiều nhất mỗi giây một lần (`UPCOMING_WALK_EVERY_US`),
    mỗi nhịp (200 ms) một trang tối đa 8 mục, chỉ lấy id và thời gian, không
    bao giờ lấy lời, và báo cho giọng nói khi kết quả thay đổi
    (`FromWorker::Upcoming`). Đây là đọc cục bộ qua socket, không ra mạng.
- **Giới hạn:** một lượt hoàn tất chưa tới một phút trước hạn thì bỏ qua
  kiểm tra. Nếu mạng mất sau lượt đó, lần reo được thử như một lượt của mô
  hình trước, như trước đây, và có thể reo trễ vài giây.
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
| Dung lượng tối đa của thư mục đoạn âm thanh | 122.918.400 byte (`MOST_BYTES`) | `ring_clips.rs` |
| Đoạn âm thanh dài nhất được giữ | 20 s (`MOST_FRAMES`, có sẵn) | `say/tts_cache.rs` |
| Tạo lại sau khi thất bại | 60 s, gấp đôi, tối đa 1 giờ (`WARM_EVERY`, `WARM_EVERY_MOST`, có sẵn) | `say/tts_cache.rs` |

## Chạy lại hàng kiểm tra (lamp-06c1)

Cần `LAMP_OFFLINE_RINGS=on` (có sẵn trong unit lamp-voice của sản phẩm) và
bộ nhớ đệm giọng nói đã được tạo khi còn mạng. Đạt khi: một hẹn giờ và một lời nhắc đến hạn
trong lúc mất mạng mỗi cái reo trong vòng 5 s sau hạn, một lần; không nói lại
gì sau khi mạng trở lại; 10 câu hỏi giữ giới hạn của chúng; không tiếng động
ngắn nào nhận câu báo thứ hai. Thêm trường hợp "không ai nói gì trong lúc mất
mạng": một hẹn giờ đến hạn sau hơn 60 s không có lượt nào, ghi lại thời gian
reo tính từ hạn (`late_by_ms` của `offline_ring`) và dòng nhật ký kiểm tra
mạng trước khi reo.

Hàng của #8708 (không cần người): đặt một lời nhắc khi còn mạng, đoạn âm
thanh của nó xuất hiện; cắt mạng, nó reo bằng lời của nó trong vòng 5 s sau
hạn, một lần; huỷ một cái và sửa một cái, đoạn âm thanh cũ mất trong khoảng
1 s; khởi động lại khi còn mạng rồi cắt mạng, nó vẫn reo; đặt một cái trong
lúc mất mạng, câu chung phát, rồi lời của nó được nói một lần sau khi có
mạng lại; `Forget` người thử nghiệm của bộ kit, các đoạn âm thanh của họ mất.
