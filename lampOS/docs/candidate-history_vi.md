# Lịch sử ứng viên, chưa nối vào runtime (#8971)

`eyes::candidate_history` giữ quan sát cục bộ có giới hạn cho bộ đọc xác nhận
sau này. [English](candidate-history.md).

**Không có nơi gọi trong runtime, giới hạn hoạt động mặc định, kết luận hiện
diện, mô hình, lưu trữ, trường bus hay công tắc.** Mặt dịch chuyển có thể là ảnh,
màn hình chuyển động hoặc lỗi nối track. Lịch sử và chất lượng so sánh đều không
chứng minh người thật. [Đánh giá tĩnh](https://github.com/autonomous-ai/lampOS/pull/8980)
và [chống giả mạo](https://github.com/autonomous-ai/lampOS/pull/8982) thất bại nên
hiện diện tiếp tục chỉ là quan sát. [API chất lượng chung](https://github.com/autonomous-ai/lampOS/pull/8981)
là phần riêng.

## Tái sử dụng và hai lỗi đã tái hiện

Bản nháp cũ xử lý từng mặt, ghép các tâm gần nhau. Hai mặt trong một khung có
thể dùng chung ô và bị coi là chuyển động. Nó còn đo độ dịch trước khi hết hạn
điểm gốc: các khung đứng yên liên tiếp giữ gốc cũ, rồi một lần dịch ngoài cửa sổ
vẫn xác nhận. Đã [tái hiện trên đúng nguồn bản nháp](https://github.com/autonomous-ai/lampOS/issues/8971#issuecomment-7241111762).

Tái dùng `bus_out::Ids::of` để gán id theo lô phía trước, không viết thêm bộ ghép
hình học. Nó đánh dấu track đã được lấy trong khung. Lịch sử mới nhận id số tạm,
từ chối id trùng, không nối alias hay danh tính tìm lại. Điều này không chứng
minh độ chính xác nối track; vẫn cần đánh giá trước khi dùng xác nhận. Thứ tự
runtime hiện tại giữ nguyên; chuyển gán id lên trước cổng tương lai là tích hợp
riêng.

`History::observe` nhận cả khung. Nó hết hạn gốc **trước** khi đo độ dịch. Quan
sát đầu chưa có độ dịch; quan sát sau cần khoảng thời gian chụp dương. Kết quả
là số, không phải người đã xác nhận. Không thêm ngưỡng chuyển động.

Đây là cửa sổ theo điểm gốc, không phải lịch sử trượt chính xác của mọi cặp.
Đặt lại gốc có thể bỏ mất chuyển động gần đây hữu ích. Phải đo tổn thất bảo thủ
này trước khi thành phần khác dùng; không tuyên bố tỷ lệ giữ người.

## Hợp đồng

`History::new(epoch, Limits)` cần các thời lượng khác không, truyền rõ ràng:

| Giới hạn | Ý nghĩa |
|---|---|
| `window_us` | Tuổi tối đa của quan sát gốc trong cửa sổ |
| `maximum_gap_us` | Khoảng cách lớn nhất giữa hai lần chụp của track còn giữ |
| `maximum_frame_age_us` | Tuổi khung lớn nhất khi được xử lý |
| `MAX_TRACKS = 8` | Giới hạn tài nguyên cố định, không phải số người đã đo |

Thời gian chụp và xử lý phải dùng **cùng đồng hồ đơn điệu**. Epoch là thế hệ tăng
dần do nơi gọi quản lý cho vòng đời camera/quyền riêng tư, không phải giấy phép
camera. Sequence và thời gian chụp tăng nghiêm ngặt trong epoch; thời gian xử lý
không lùi. Khung tương lai, cũ, lặp hoặc đảo thứ tự từ chối cả lô và ngắt lịch
sử. Dấu thời gian hợp lệ vẫn được tiêu thụ nếu danh sách ứng viên sai, nên không
thể phát lại cùng khung với ứng viên khác. Dấu thời gian không hợp lệ không đẩy
mốc thời gian lên.

Tâm ứng viên phải hữu hạn và trong [0,1] của phần ảnh thật. Quá tám ứng viên,
trùng id hoặc bất kỳ tâm sai nào từ chối cả lô; không cắt bớt hay thành công một
phần. `usable=false` là chất lượng do nơi gọi đưa vào, không phải hiện diện.
Id vắng mặt hoặc không dùng được mất cửa sổ tại khung đó. Id xuất hiện lại hoặc
thay thế bắt đầu mới, kể cả cùng vị trí. Quy tắc bảo thủ này không quy định thời
gian giữ hiện diện của sản phẩm.

Trước độ dịch theo thời gian, nơi gọi phải xác lập `CameraInterval::KnownStill`
cho **toàn khoảng**, với chứng cứ chuyển động đầy đủ, còn mới. `Moved` hoặc
`Unknown` đặt lại gốc tại khung hiện tại; chỉ khoảng hợp lệ tiếp theo mới tạo
độ dịch. Có thể dùng ngữ cảnh thời điểm chụp của `speaking::JointHistory`, nhưng
một giá trị khớp gần nhất không đủ chứng minh cả khoảng đứng yên. Thị sai cần
diễn giải có hiệu chỉnh khác; thành phần này không suy ra thị sai.

`close()` xóa mọi thứ và từ chối khung. `begin_epoch` đóng trước rồi yêu cầu thế
hệ mới hơn nghiêm ngặt; thế hệ sai hoặc quay vòng vẫn đóng. Số đã trả về **không
phải token thẩm quyền**. Nơi gọi tương lai phải nối đúng mặt, ảnh và lần chụp,
kiểm tra lại độ mới, quyền camera/quyền riêng tư ở mỗi lần sử dụng có tác động;
không dùng những số này làm quyền nhận diện, lưu, chào hay phát kết quả.

## Giới hạn và xác minh

Dùng tám ô tùy chọn cố định, đầu ra tám phần tử, không cấp phát heap. Kiểm tra
và tìm track quét tối đa 8×8 phần tử. Chỉ giữ tâm, id số tạm và thời gian; không
giữ khung, dấu mặt, tên hay lời. Bộ đếm bão hòa; kiểm tra thời gian trước phép
trừ; epoch không quay vòng. Không có worker, tác vụ trễ hay I/O chặn cần hủy.

Đối chứng sinh sẵn phủ hai dạng lỗi, id riêng từ tracker hiện có, khung lặp/đảo,
đồng hồ xử lý lùi, khung cũ/tương lai, biên bao gồm bằng, đóng/mở quyền riêng tư,
epoch tràn, id trùng, hình học sai, đảo thứ tự đủ tám track, từ chối vượt số
lượng, id vắng/không dùng được/thay thế, khoảng quá dài và camera chuyển động/
chưa rõ. Chúng kiểm tra cơ chế, không đo hiện diện vật lý, nối track hay thời
lượng sản phẩm.

`cargo run --release -p lamp-eyes --example candidate_history_measure` đo từ
khung tám ứng viên đến lịch sử: lần đầu rồi 1.000 lần, giữ mọi thời gian. Ngân
sách máy thử p95 ≤100 μs cho khung tám ứng viên. Không gồm detector, tracking,
chất lượng, xác thực cả khoảng camera, camera thật hay phát kết quả. Chưa đo
ARM64, tải giọng nói hoặc độ trễ xác nhận thực; benchmark không nối runtime.

Trên iMac Intel dùng chung (macOS 26.6.2, Rust 1.96.0, release), lần chạy đã
chốt với dữ liệu sinh sẵn mất **1.470 μs ở khung đầu**, rồi **trung vị 0.237 μs
/ p95 0.255 μs / tối đa 0.578 μs** qua 1.000 khung tám ứng viên tiếp theo.
Giữ riêng toàn bộ 1.001 thời gian gốc; phép tính riêng khớp mọi thống kê đã
báo cáo. Chỉ đạt ngân sách phép tính trên máy thử, không phải chỉ tiêu đầu-cuối
hay trên thiết bị.
[Số tổng hợp và SHA mã nguồn/quy trình](verification/candidate-history-20261128/numeric.json).

Xác minh: 16 đối chứng tập trung và 526 test ở các target của eyes đều đạt;
năm test thủ công/mô hình đã có vẫn bị bỏ qua. Định dạng toàn workspace và
Clippy nghiêm ngặt mọi target đạt. Lượt toàn bộ đầu tiên không mở được Unix
socket cục bộ trong sandbox; chạy lại với quyền socket cục bộ thì đạt.
Cả hai lượt không dùng đèn, mô hình học máy hay người thật.
