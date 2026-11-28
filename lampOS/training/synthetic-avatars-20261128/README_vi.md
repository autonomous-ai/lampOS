# Công thức avatar tổng hợp dùng chung

Bộ nguồn này giúp nghiên cứu hướng đầu và khuôn mặt đang nói dùng chung các
avatar A00–A23 theo
[công thức vision-public-synthetic-20261128-v1](https://github.com/autonomous-ai/lampOS/blob/3a8934944acba1c17f3cb0267aae778410f5809e/lampOS/tools/sensors/vision-freeze-20261128-v1.json)
của Reviewer cho #8964 và #8847. Ảnh đã kết xuất, mã băm từng tệp phụ kiện và
mã băm hình học trung tính thực tế sẽ được chốt riêng. Commit này không cho
phép huấn luyện và không khẳng định đã có bộ dữ liệu hoàn chỉnh hay kết quả
độ chính xác.

## Danh tính cố định

`identity-parameters-v1.json` ghi chính xác tham số hình thái và thứ tự áp
dụng các target. A00–A11 dùng để huấn luyện, A12–A17 để tinh chỉnh, A18–A23
để kiểm thử. Mọi biến thể và nhóm nguồn của một avatar giữ nguyên vai trò
giữa các nghiên cứu. Không gán cùng tên Axx cho những hình thái khác nhau.

Mỗi nhóm sáu avatar có đủ sáu vật liệu da, mỗi kiểu tóc (không tóc, ngắn,
dài) xuất hiện hai lần và ba avatar đeo kính. Đây là thuộc tính do người
tạo dữ liệu đặt, không phải nhãn suy luận về người thật. Các avatar dùng
chung một bộ quần áo. Nhóm tổng hợp nhỏ này không đại diện cho dân số.

`shared_identity_v1.py` áp dụng tham số qua các dịch vụ MPFB. Nó không suy
luận, huấn luyện, gọi mạng hay lấy mẫu ngẫu nhiên. Khởi tạo bản MPFB đã ghim
trong Blender, rồi truyền `HumanService`, `TargetService`, thư mục `data`
của MPFB, thư mục tài nguyên lõi/kính và một dòng avatar vào
`construct_identity`. Giữ nguyên thứ tự JSON của các target khuôn mặt.
Hàm trả về mô hình và rig với tỷ lệ 0.1, rig mặc định, vật liệu GAMEENGINE
và mức chia lưới bằng không. Máy ảnh, tư thế đầu/vai độc lập, ánh sáng và
kết xuất thuộc bộ tạo cảnh được chốt riêng.

Gốc khuôn mặt là đỉnh 297, đánh số từ không, của lưới MakeHuman cơ sở
(chóp mũi). Tệp nguồn ghi hệ trục trung tính và cách tính mã băm hình học.
Giữ chỉ số đỉnh và tập đỉnh; ghi hình học thực tế cùng phiên bản nền tảng
trước khi dùng đầu ra. Cùng tên hoặc tham số chưa chứng minh lưới thực tế
giống nhau từng byte.

## Nguồn và quyền sử dụng

`shared-identity-source-v1.json` ghim Blender 4.5.17, MPFB 2.0.18 tại commit
`d9648f3257a0b9712281d466245f30d656250154`, mã băm nguồn, tên vật liệu/phụ kiện
và liên kết nhà phát hành. Mã chương trình MPFB dùng GPL;
[điều khoản tài nguyên](https://github.com/makehumancommunity/mpfb2/blob/d9648f3257a0b9712281d466245f30d656250154/LICENSE.ASSETS.md)
xác định lưới và target hình thái lõi là CC0. Tài nguyên hệ thống và kính
được chọn có hồ sơ CC0 riêng. Không áp dụng điều khoản đó cho mọi tài nguyên
cộng đồng. Hai mẫu kính có thông tin giấy phép mâu thuẫn đã bị loại.

Máy chủ gói đầy đủ gặp lỗi hoặc quá chậm. Bộ tạo dữ liệu giữ lại các lần
thử và tải từng thành viên ZIP bằng byte range, kiểm tra header, kích thước
và CRC rồi băm dữ liệu giải nén. Phải chốt danh mục từng tệp tài nguyên và
ảnh trước khi huấn luyện. Giữ tài nguyên, ảnh, bản xem thử và mô hình ngoài
git. Mỗi bộ dữ liệu phải nhỏ hơn 5.000.000.000 byte, tính tài nguyên chung
một lần.

## Kiểm tra và phạm vi hiện tại

Kiểm tra nguồn đã xác nhận 24 ID, seed, vai trò, cân bằng thuộc tính trong
từng nhóm sáu, sự tồn tại của tất cả target khuôn mặt trong cây MPFB đã
ghim, mã băm JSON/nguồn và cú pháp Python. 36 phép kiểm tra hình học đầu/vai
của A00 khớp nhãn theo tia máy ảnh trong sai số float32 đã công bố. Một ảnh
xem thử A00 riêng dùng da, mắt và quần áo thật từ tài nguyên. Đây là kiểm
tra dựng mô hình; chưa có ảnh kiểm thử, trọng số đã huấn luyện hay phép đo
trên đèn trong gói nguồn này.
Hàm dựng dùng chung cũng tái tạo chính xác mã băm hình học trung tính và
tập đỉnh của A00 so với bộ dựng đã tạo ảnh xem thử.

Thứ tự hiện tại của chủ sản phẩm là thử `solvePnP` với năm điểm YuNet trước,
chỉ huấn luyện mạng nếu hình học không đạt các ngưỡng đã chốt. Bộ nguồn này
không thay đổi dịch vụ hay công tắc sản phẩm. Trưởng nhóm thực hiện mọi lần
thử trên bo mạch; kết quả ngoại tuyến không cho phép merge hoặc triển khai.
