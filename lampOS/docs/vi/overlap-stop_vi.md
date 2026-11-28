# Nghe lời ngắt khi đèn đang nói

Cảm biến 7 thử một bộ nhận dạng lời nói cục bộ trên luồng mic 16 kHz sau bước khử tiếng vọng. Bộ nhận dạng Vosk small English US 0.15 dùng từ vựng mở và giấy phép Apache-2.0. Công cụ phát lại ngoại tuyến chỉ ghi nhận tín hiệu yêu cầu dừng. Công tắc dự kiến trên sản phẩm mặc định tắt. [Phương pháp đã chốt](https://github.com/autonomous-ai/lampOS/issues/8923#issuecomment-7237422960) · [Lệnh phát lại và mã SHA-256](../verification/overlap-stop-8923/README.md)

Cứ 20 ms, công cụ đưa thêm 320 mẫu PCM vào bộ nhận dạng và đọc kết quả tạm thời hoặc kết quả mà bộ nhận dạng vừa chốt. Mỗi phát ngôn chỉ có thể phát một tín hiệu dừng và hai tín hiệu cách nhau ít nhất một giây. Bộ nhận dạng không nhận tên cảnh, lời thoại dự kiến hoặc mốc bắt đầu nói. Khi bản thu bị đứt, cần đánh dấu phần thiếu và bắt đầu lại từ đoạn liên tục kế tiếp.

Để chấm mục tiêu 400 ms, phải đối chiếu thời điểm có tín hiệu với lúc người dùng thực sự bắt đầu nói và kiểm tra rằng loa của đèn đang phát. Thời gian tính toán trên máy phát lại được ghi riêng. Bản thu gốc, lời nhận dạng từng dòng và bảng kê SHA-256 được giữ trên máy thu. [Yêu cầu thử nghiệm](https://github.com/autonomous-ai/lampOS/issues/8923)
