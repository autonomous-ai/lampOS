# Bản ghi kết thúc dịch vụ của Cloud

Xem [exit note của Cloud](../cloud.md#crashes-the-exit-note). Recorder chạy
root qua `ExecStopPost=-+...`; dấu `+` không xóa umask. Voice giữ `UMask=0077`,
Cloud đọc với người dùng `lampos`. Recorder đặt 0644 trên descriptor tệp tạm
trước sync và đổi tên; thư mục là 0755, chỉ chủ sở hữu có quyền ghi. Trạng thái
và hàng đợi riêng tư vẫn là 0600. Mỗi unit giữ tám bản ghi mới nhất.

Trước khi công bố bản ghi mới, recorder sửa quyền bản ghi cũ của cùng unit và
boot, cũ trước. Kết thúc thành công cũng sửa quyền, không tạo bản ghi hay gọi
systemd; thư mục chưa có vẫn không được tạo. Khôi phục diễn ra ở lần dừng tiếp
theo của unit với recorder mới, không chỉ khi Cloud khởi động lại.

Chỉ tệp thường, một hard link, thuộc recorder, không qua symlink, tối đa 4.096
byte, có tên chuẩn và nội dung khớp unit/boot/thời điểm cùng các trường đã biết
mới được sửa quyền trên chính descriptor đã kiểm tra. Byte được giữ nguyên.
Tệp khác unit/boot, lỗi định dạng hoặc có trường mới chưa biết không được mở
rộng quyền; đường đọc thường vẫn chấp nhận trường bổ sung. Thư mục symlink hoặc
có quyền ghi ngoài chủ sở hữu bị từ chối. Thư mục cũ 0700 chỉ thành 0755 sau khi
các bản ghi đủ điều kiện đã được sửa.

Cloud phân biệt lỗi mở/đọc với lỗi JSON, không ghi nội dung vào journal. Lỗi
truy cập bản ghi chuẩn cũ chưa xử lý giữ các bản ghi mới hơn của cùng unit đến
lượt poll đọc được hoặc loại bỏ tệp cũ; unit khác vẫn tiến hành. Nhờ đó mốc đã
đọc không vượt qua bản ghi cũ khi sửa quyền. Tệp bị xóa, JSON/UTF-8 lỗi và thời
điểm đã xử lý không gây chặn. Boot của tệp không đọc được chưa xác định, nên
chặn này bảo thủ; `/run` được xóa khi khởi động máy.

Kiểm thử host dùng tiến trình con umask 0077, định dạng bản ghi được lưu ngày
21 tháng 10 và lỗi truy cập được chèn có kiểm soát. Backend3 xác nhận tệp sự cố gốc #8413 chưa từng
được lưu và đã mất sau khởi động lại. Đã kiểm tra giao cho hàng đợi, chưa chứng minh đọc root→lampos trên đèn hoặc
giao tới broker công ty. Mục tiêu p95 host là dưới 50 ms cho sửa quyền, tuần tự
hóa, ghi/sync/đổi tên và loại bản ghi cũ; không gồm truy vấn systemd có giới hạn,
poll Cloud mỗi 10 giây hay MQTT. Trên macOS: lần đầu 19,315 ms; 200 lần lặp
với 45 unit, mỗi unit giữ tám bản ghi, có p50 12,781 ms, p95 16,957 ms và
tối đa 19,735 ms. Đạt mục tiêu host không chứng minh độ trễ thiết bị hay giao
báo cáo đầu cuối. Switch và điều kiện đồng ý không thay đổi.
