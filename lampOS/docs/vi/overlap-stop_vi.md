# Nghe lời ngắt khi đèn đang nói

Cảm biến 7 thử một bộ nhận dạng lời nói cục bộ trên luồng mic 16 kHz sau bước khử tiếng vọng. Bộ nhận dạng Vosk small English US 0.15 dùng từ vựng mở và giấy phép Apache-2.0. Công cụ phát lại ngoại tuyến chỉ ghi nhận tín hiệu yêu cầu dừng. Công tắc dự kiến trên sản phẩm mặc định tắt. [Phương pháp đã chốt](https://github.com/autonomous-ai/lampOS/issues/8923#issuecomment-7237422960) · [Lệnh phát lại và mã SHA-256](../verification/overlap-stop-8923/README.md)

Cứ 20 ms, công cụ đưa thêm 320 mẫu PCM vào bộ nhận dạng và đọc kết quả tạm thời hoặc kết quả mà bộ nhận dạng vừa chốt. Mỗi phát ngôn chỉ có thể phát một tín hiệu dừng và hai tín hiệu cách nhau ít nhất một giây. Bộ nhận dạng không nhận tên cảnh, lời thoại dự kiến hoặc mốc bắt đầu nói. Khi bản thu bị đứt, cần đánh dấu phần thiếu và bắt đầu lại từ đoạn liên tục kế tiếp.

Để chấm mục tiêu 400 ms, phải đối chiếu thời điểm có tín hiệu với lúc người dùng thực sự bắt đầu nói và kiểm tra rằng loa của đèn đang phát. Thời gian tính toán trên máy phát lại được ghi riêng. Bản thu gốc, lời nhận dạng từng dòng và bảng kê SHA-256 được giữ trên máy thu. [Yêu cầu thử nghiệm](https://github.com/autonomous-ai/lampOS/issues/8923)

## Kết quả từ các bản thu ngày 28 tháng 11 năm 2026

Nhóm phát giọng nói tổng hợp trong lúc đèn dbda đang nói ở mức âm lượng 35%. Lượt R8892d tại `a3fa627b5e` có bản thu mic 16 kHz liên tục sau khử tiếng vọng. Trong hai nhóm `stop-follow` và `r-stop`, đèn bỏ lỡ cả năm yêu cầu dừng. Khi phát lại chính bản thu đó, bộ nhận dạng đã chốt phát hiện hai yêu cầu.

| Yêu cầu | Sự kiện dừng do đèn ghi | Tín hiệu khi phát lại | Có sự kiện hoặc tín hiệu |
|---|---:|---:|---:|
| R8892d, năm yêu cầu trong hai nhóm | 0/5 | 2/5 | 2/5 |
| R8892d, toàn bộ yêu cầu dừng kể cả đối chứng | 4/9 | 3/9 | 6/9 |
| S16492-stops, năm yêu cầu trong hai nhóm | 2/5 | Thiếu bản thu âm | Chưa đo |
| S16492-stops, toàn bộ tám yêu cầu dừng | 5/8 | Thiếu bản thu âm | Chưa đo |

Ba trong năm yêu cầu ở R8892d khiến đèn tạm ngừng rồi nói tiếp. Hai trong ba yêu cầu này bị coi là tiếng của chính đèn. Hai yêu cầu còn lại không để lại sự kiện ngắt lời trong nhật ký. Cả năm đều không dẫn đến lần gửi mới để nhận dạng lời nói. Ở bản ổn định, trong hai lần được bộ chấm ghi nhận là dừng, một lần nhận ra bốn từ. Lần còn lại gửi 1.450 ms âm thanh và nhận được không từ nào. Cả ba yêu cầu dừng trong nhóm đối chứng trên mỗi bản đều đến được bước xử lý lệnh dừng.

Với câu ngắn “Stop”, bộ nhận dạng phát tín hiệu sau mốc phát của bộ thử 1.132 ms. Đối chiếu dạng sóng của câu mẫu với bản thu mic sau khử vọng cho thấy câu nói bắt đầu sau mốc đó 487 ±58 ms, nên độ trễ phát hiện là 645 ±58 ms. Kết quả đối chiếu có độ tin cậy thấp hơn cho câu dài “never mind” cho độ trễ 1.983 ±40 ms. Với câu đối chứng “never mind”, độ trễ là 831 ±27 ms. Một lần đèn xác nhận ngắt lời ban đầu được ước tính mất 396 ms. Khi đối chiếu với dạng sóng của chính câu đó, độ trễ là 521 ±29 ms. Cả hai lượt đều thiếu bản ghi mẫu âm cuối còn nghe được từ loa. Các bản thu ở khâu trước đó cũng còn thiếu nên chưa thể tách riêng ảnh hưởng của việc căn chỉnh tín hiệu tham chiếu, khử vọng và triệt vọng. Phương án này vẫn chỉ chạy ngoại tuyến. [Báo cáo đánh giá và bảng kê dữ liệu](https://github.com/autonomous-ai/lampOS/issues/8923#issuecomment-7237950265)
