# Thiết lập: quốc gia Wi-Fi từ múi giờ (#7707)

Ghi chú này tương ứng với mục
[Wi-Fi country from the time zone](../setup.md#wi-fi-country-from-the-time-zone-7707)
trong tài liệu thiết lập tiếng Anh.

Khi kết nối Wi-Fi, `Config::wifi_country` chọn quốc gia cho wpa_supplicant từ
dữ liệu múi giờ cục bộ. Trong lần thiết lập đầu, `run_configured` chỉ truyền
`timezone` đã được chấp nhận của yêu cầu khi `Save::keeps_settings()` là
`true`. Khi không có múi giờ đó, đọc `Config.timezone_file` đã lưu (thường
là `/etc/timezone`). Thiết lập khôi phục và helper `wifi.set` của app chỉ dùng
múi giờ đã lưu: một yêu cầu khôi phục không thể đổi quốc gia bằng cách gửi múi
giờ mới.

Bộ tra cứu đọc `zone.tab` dưới `Config.zoneinfo`, chỉ nhận mã quốc gia gồm
đúng hai chữ cái ASCII viết hoa. Thử tên múi giờ được chọn trực tiếp trước,
sau đó mới giải bí danh là liên kết tượng trưng qua tệp `TZif` hợp lệ có
đường dẫn chuẩn vẫn nằm trong thư mục zoneinfo đó. Bí danh là bản sao tệp
hoặc liên kết cứng mà không có dòng riêng trong bảng dùng giá trị dự phòng:
không so sánh nội dung tệp hay inode. Tiền tố `posix/` và `right/` được chuẩn
hóa khi tra bảng. Dữ liệu múi giờ thiếu, không hợp lệ hoặc không có ánh xạ dùng lại
`Config.country`, là `US` trong cấu hình sản phẩm. Giá trị dự phòng sai định
dạng vẫn bị kiểm tra cấu hình từ chối; việc suy ra quốc gia không bỏ qua kiểm
tra này. Không định vị qua mạng và không thêm trường yêu cầu hay phản hồi.

Với yêu cầu HTTP thiết lập hoặc khôi phục, việc tra cứu nằm trong luồng nền
kết nối hiện có, sau khoảng chờ 2 giây để phản hồi đến điện thoại trước khi
hotspot tắt. Đổi Wi-Fi từ app tra cứu trong helper Wi-Fi hiện có, bằng múi giờ
đã lưu. Kiểm tra SSID/mật khẩu và cấu hình wpa_supplicant, thời hạn kết nối,
xác nhận từ app và khôi phục cấu hình cũ giữ nguyên cách hoạt động.
Mục tiêu độ trễ tra cứu cục bộ là **dưới 10 ms**, từ lúc chọn múi giờ đến khi
trả mã quốc gia, bao gồm đọc tệp cục bộ. Ngày 24/11/2026, kiểm thử tích hợp
bản dựng native macOS chưa tối ưu đo các trường hợp múi giờ yêu cầu, bí danh
liên kết tượng trưng, múi giờ đã lưu và múi giờ không biết, mỗi trường hợp
1.000 lần gọi lặp lại. Mỗi lần gọi đầu dưới 0,5 ms và p95 của mỗi trường hợp
dưới 1,3 ms, nhưng lớn nhất là 93,3 ms: **chưa đạt đầy đủ mục tiêu 10 ms**.
Kiểm thử chép `zone.tab` đã cài vào cây TZif fixture tạm thời; việc tạo fixture
và chép bảng làm ấm bộ nhớ đệm hệ thống tệp, nên những lần gọi đầu này không
phải phép đo với hệ thống tệp chưa được lưu đệm. Lệnh chạy từ `lampOS`:
`cargo test -q -p lamp-setup wifi_country_local_lookup_timing -- --ignored --nocapture`.
Phép đo chỉ bao gồm tra cứu cục bộ, không phải độ trễ quét hay kết nối trên
thiết bị.
Lần đo thứ hai có lần gọi đầu 13,4 ms cho múi giờ yêu cầu, 2,1 ms cho bí danh
và 1,2 ms cho múi giờ đã lưu; các lần gọi lặp lại có p95 dưới 0,4 ms và lớn
nhất 4,1 ms. Mức lớn nhất 93,3 ms ở lần đo trước vẫn được giữ, mục tiêu 10 ms
vẫn chưa đạt đầy đủ; cả hai lần đo đều không xác nhận hiệu năng khi hệ thống
tệp chưa được lưu đệm hay nguyên nhân khác biệt về thời gian.

Lần quét mạng đầu tiên diễn ra trước khi điện thoại gửi múi giờ thiết lập.
Chọn quốc gia cho bước kết nối sau đó chưa chứng minh lần quét này tìm được
những kênh nào. Hai phép kiểm tra trên thiết bị vẫn riêng biệt và chưa được
xác minh:

| Kiểm tra trên thiết bị | Trạng thái |
|---|---|
| Tìm thấy mạng kênh 13 trong lần quét thiết lập đầu | Chưa đo |
| Kết nối với mạng đó sau khi bước kết nối chọn quốc gia | Chưa đo |
