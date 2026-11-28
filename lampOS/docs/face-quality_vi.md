# Chất lượng khuôn mặt dùng chung, chưa hoạt động (#8971)

`eyes::quality::measure(face, picture, native_frame)` đo khung đã giải mã.
`FaceQuality::fit_to_compare(&ComparisonLimits)` áp dụng giới hạn bên gọi cung
cấp. Chưa có bên gọi lúc chạy, setting, trường bus, suy luận, lưu ảnh, so khớp
hoặc thay đổi hiện diện. [English](face-quality.md).

[Đánh giá 5b](https://github.com/autonomous-ai/lampOS/pull/8980) không có ngưỡng
hiện diện tĩnh đạt yêu cầu. Fit không chứng minh người sống, danh tính, đồng ý
hoặc quyền lưu. Hiện diện vẫn chỉ quan sát theo
[lead](https://github.com/autonomous-ai/lampOS/issues/8971#issuecomment-7241776080).

## Phép đo và dữ liệu thiếu

Điểm và hộp thô giữ tọa độ ảnh làm việc. Chiều cao/phần diện tích bị cắt dùng
**ảnh hữu ích**, không dùng đệm đen. Khoảng mắt lấy từ landmark; khoảng mắt
pixel gốc dùng scale giải mã thật. Đây là ước lượng, không chứng minh chi tiết
mắt đã phân giải. Phóng lớn crop không tạo thêm pixel gốc.

Góc tuyệt đối dùng lại `facing::yaw_deg`, heuristic năm landmark, chưa phải góc
đầu thật được kiểm chứng. 5a phân tầng bằng 6DRepNet; không chuyển ngưỡng số
trực tiếp từ bộ ước lượng đó sang heuristic này.

`NativeLumaLaplacianV1` là phương sai tổng thể Laplacian của luma nguyên, bốn
láng giềng cách một pixel gốc, lấy mẫu trong hộp thô. Không resize, nội suy hay
cấp phát crop. Lưới giới hạn có thể alias hoa văn; kết quả phụ thuộc nội dung,
JPEG, ánh sáng. Chưa có ngưỡng sử dụng đạt yêu cầu.

Thiếu pixel gốc hoặc ngưỡng mờ cho `unavailable`. Kích thước, scale, buffer,
landmark hoặc giới hạn sai cho `invalid`. Mọi landmark phải trong hộp thô và
ảnh hữu ích; mắt trùng không cho góc dùng được. Lý do khác: `low_score`, `small`,
`turned`, `blurred`, `clipped`. Bảy bit không lặp lý do. Biên gồm giá trị bằng;
thiếu hoặc sai không được coi là đạt.

Kiểm tra layout gốc gồm kích thước, byte, scale decoder chính xác và kích
thước hữu ích. Chúng không chứng minh buffer cùng lần phơi sáng. Bên gọi phải
ràng buộc mặt/ảnh làm việc/ảnh gốc cùng capture, kiểm tra độ mới và epoch camera/
riêng tư, rồi kiểm tra quyền lúc so sánh/lưu/công bố. Cần giới hạn batch và hủy
dữ liệu cũ. Module không cấp các quyền này, không thêm worker hoặc giữ khung.

## Toàn bộ số và giới hạn

| Số/giới hạn | Ý nghĩa và nguồn |
|---|---|
| Canvas 320×256 | `picture::{WIDTH, HEIGHT}` hiện có; phần hữu ích có thể nhỏ hơn |
| Cạnh gốc tối đa 4.096 | Dùng lại giới hạn decoder |
| 128 mẫu/trục, tối đa 16.384 stencil | Giới hạn công việc, không phải ngưỡng chính xác |
| Ít nhất 2 stencil | Một giá trị không thể hiện biến thiên; crop nhỏ hơn là unavailable |
| Luma `(77R + 150G + 29B) >> 8` | Định nghĩa nguyên có phiên bản; tổng hệ số 256 |
| Bốn láng giềng trừ 4×tâm | Khoảng cách một pixel gốc |
| 7 bit lý do | Enum cố định, không chứa tên, lời nói hoặc pixel |
| Điểm tối thiểu trong [0,1] | Bên gọi cấp; không có mặc định sản phẩm |
| Khoảng mắt gốc tối thiểu hữu hạn >0 | Bên gọi cấp, cần kiểm chứng riêng |
| Góc ước lượng tối đa trong [0°,90°] | Cần bằng chứng đúng bộ ước lượng |
| Phần cắt tối đa trong [0,1] | Bên gọi cấp; chính sách không cắt cần 0 |
| Metric mờ và phương sai hữu hạn không âm | Cần metric/ngưỡng phù hợp đã đo; thiếu là unavailable |

Không có `ComparisonLimits` mặc định hoặc biến môi trường. Số trong fixture
chỉ kiểm tra số học, không phải đề xuất cấu hình.

## Tái sử dụng

V1 `faceid/recognizer.py` kiểm tra chiều cao, phần cắt và phương sai Laplacian
trên crop căn chỉnh 112 pixel; `faceid/geometry.py` từ chối landmark ngoài hộp/
ảnh. Giữ bảo vệ hình học, dùng lại scale và heuristic góc Rust. Đổi fallback
độ sắc nét vô hạn khi V1 thiếu crop thành unavailable. Metric mờ gốc lấy mẫu
khác metric crop căn chỉnh, không dùng lại ngưỡng V1. Không thêm mô hình hoặc
phụ thuộc runtime cũ.

## Kiểm tra và độ trễ

Test tự tạo kiểm tra scale gốc/làm việc, padding/phần cắt, thiếu ngưỡng, buffer/
giới hạn/landmark sai, lý do, biên, Laplacian ảnh phẳng/bàn cờ và giới hạn công
việc. Không đo độ chính xác nhận dạng hay người sống.

`cargo run --release -p lamp-eyes --example quality_measure` đo khung đã giải
mã + mặt được cung cấp → kết quả; không gồm tạo khung, JPEG, detector, lập lịch,
công bố. Báo lần gọi đầu cho ảnh tự tạo 320×240, 1280×960, 4096×4096, rồi 500
lần một mặt và 500 lần nhóm tám mặt, không bỏ ngoại lệ. Tám là tải benchmark,
không phải giới hạn batch sản phẩm. Mục tiêu host: p95 ≤1 ms/mặt, ≤8 ms/nhóm.
Chưa đo ARM64 hoặc tải voice. Không dùng mô hình, camera, đèn hoặc dữ liệu ngoài.

Đo trên Intel iMac20,1/i7-10700K dùng chung, macOS x86_64, ngày 28-11-2026:

| Khung gốc | Lần đầu, ms | p50 / p95 một mặt, ms | p95 / lớn nhất tám mặt, ms |
|---|---:|---:|---:|
| 320×240 | 0,250 | 0,225 / 0,335 | 2,644 / 6,334 |
| 1280×960 | 0,844 | 0,224 / 0,257 | 2,140 / 4,672 |
| 4096×4096 | 0,691 | 0,244 / 0,962 | 7,533 / 14,652 |

Đạt mục tiêu p95 host. Một mặt chậm nhất 1,901 ms. Lần đầu là cho mỗi kích
thước, không khởi động process mới cho từng kích thước. Giữ cục bộ toàn bộ
3.000 thời gian lặp và ba lần đầu; [số tổng hợp và hash mã](verification/face-quality-20261128/numeric.json)
được công bố. Kiểm tra scalar riêng tính lại percentile và hash (27 kiểm tra).
Hộp giả lập toàn khung dùng tối đa stencil; không đo chính xác detector/nhận
dạng hoặc phần cứng cô lập.

Mac `cargo test --locked --offline --release -p lamp-eyes --all-targets` đạt
522 test, gồm 12 test quality mới. Giữ năm ignore có sẵn: ba kiểm tra cần mô
hình/runtime, một tác vụ viết fixture người thủ công và một báo cáo chi phí
diagnostic. Clippy nghiêm ngặt mọi target và format workspace đạt.
