# Đánh giá ngưỡng xác nhận có người, 28-11-2026

**Kết quả: không có cấu hình đạt yêu cầu trong lưới đã công bố.** Điểm khuôn mặt,
kích thước, phần bị cắt và điểm tư thế cơ thể chưa phân biệt được người đang hiện
diện với người trong ảnh/màn hình mà vẫn giữ đủ trường hợp người thật. Không dùng
kết quả này để bật chính sách hiện diện.

[Bản tiếng Anh đầy đủ](README.md) · [Số liệu tổng hợp](numeric.json) ·
[EVAL trên issue](https://github.com/autonomous-ai/lampOS/issues/8971#issuecomment-7241725349)

## Phạm vi và mẫu số

Piece 5b chạy ngoại tuyến, theo mã sản phẩm
`16e5d147bd7dc31dbf61d1ef7e0b1f68516700c3`. Hoàn tất 3.636 trường hợp, không có lỗi
xử lý. Không truy cập đèn, lấy dữ liệu khách hàng, huấn luyện, cài đặt, merge hoặc
đổi mặc định. Đơn vị đánh giá là ảnh, không phải một lần ngồi trước đèn hay một
khoảng phòng trống qua ngày/đêm.

| Mẫu số | Hiệu chỉnh | Kiểm tra |
|---|---:|---:|
| Nhóm nguồn, gồm danh tính nhân vật dựng | 221 | 226 |
| Người trong ảnh công khai, có nhãn mũi và hai mắt nhìn thấy | 67 | 67 |
| Người dựng chính diện, tập chính | 36 | 36 |
| Tất cả điều kiện có người, kể cả biến đổi khó | 840 | 852 |
| Trường hợp vật thể | 956 | 960 |
| Nguồn chưa xác định, không coi là âm tính sạch | 12 | 16 |
| Tổng số trường hợp | 1.808 | 1.828 |

435 cảnh COCO val2017 được lọc theo giấy phép và xem trước khi chạy mô hình.
Bảy nguồn chưa rõ giữ nguyên 28 biến thể với nhãn không xác định. Mọi biến thể
của một nguồn nằm cùng một tập; không có hai ảnh trùng byte. Nhân vật A12–A17
dùng hiệu chỉnh, A18–A23 dùng kiểm tra. Không biết danh tính người trong ảnh thật,
nên không khẳng định độc lập danh tính hoặc độc lập dữ liệu huấn luyện. Các biến
thể của một nguồn/nhân vật có tương quan.

Ảnh người công khai không đại diện cho người ngồi trước đèn. Tập bàn làm việc/bàn
ăn có mặt rõ được xem thủ công gồm 12 nguồn, mỗi tập sáu. Trên sáu ảnh kiểm tra,
ngưỡng hiện tại tìm được 5/6; luật mặt dự kiến tìm được 4/6. Tập nhỏ này không đủ
chứng minh tỷ lệ 99%. Nhãn COCO có ghế/bàn/laptop chỉ là tín hiệu đồng xuất hiện,
không chứng minh tư thế ngồi tại bàn.

Đầu vào JPEG 320×240, Q85, 4:2:0, không phóng lớn. Q85 là điều kiện thử tự chọn,
không phải chất lượng camera đã đo. Biến đổi gồm tỷ lệ 0,65, ánh sáng tối ấm giả
lập với hệ số RGB `[0.35, 0.2905, 0.224]`, và dịch ảnh đến cạnh. Tập chính công
khai chỉ dùng ảnh tỷ lệ 1, ánh sáng ban ngày, không dịch cạnh. Nhân vật dựng có
khoảng cách 0,5/1/1,5 m và hai cấu hình ánh sáng kế thừa đánh giá sensor 4; không
khẳng định khớp quang học hoặc lux của đèn.

Mẫu âm tính gồm cảnh vật công khai, 1.072 trường hợp ảnh in/màn hình ghép 2D và
216 hình dựng tượng bán thân/tai nghe/đèn tròn. Khung, chân đỡ và nền được tạo
thủ công. Đây không phải ảnh chụp màn hình vật lý; tượng có hình học ít đa giác.
Chúng chỉ kiểm tra một kiểu lỗi, không bao phủ đầy đủ cảnh thực tế.

## Quy tắc khóa trước

[Protocol được đăng trước suy luận](https://github.com/autonomous-ai/lampOS/issues/8971#issuecomment-7241581604).
[Ngưỡng được khóa trước khi chấm tập kiểm tra](https://github.com/autonomous-ai/lampOS/issues/8971#issuecomment-7241656460).
Lưới có 5.642 cấu hình, bảy họ. Chỉ chọn cấu hình không nhận nhầm vật thể trong
tập hiệu chỉnh, rồi tối đa hóa tỷ lệ giữ thấp hơn của hai tập người chính; hòa
thì dùng tổng số giữ được, sau đó thứ tự tăng ban đầu của lưới. Đạt yêu cầu cần
không nhận nhầm vật thể, không có mẫu âm tính lỗi xử lý và giữ ít nhất 99% ở
**mỗi** tập người chính. Với mẫu số hiện tại cần 67/67 và 36/36; ngay cả quan sát
hoàn hảo cũng không chứng minh tỷ lệ 99% trên quần thể.

| Tham số | Giá trị công bố trước |
|---|---|
| Điểm mặt | .60, .65, .70, .75, .80, .85, .90, .95, .99, 1.01 |
| Chiều cao mặt / chiều cao ảnh hữu ích | 0, .02, .04, .06, .08, .10, .15, .20, .25, .30, .40, .50, 1.10 |
| Phần diện tích bị cắt tối đa | 0, .001, .01, .05, .10, .25, .50, 1 |
| Điểm trung bình các khớp | 0 đến .95, bước .05, rồi 1.01 |

Các họ: từng tham số đơn; điểm mặt+kích thước+không bị cắt; thêm điểm cơ thể;
và thêm điều kiện mũi cơ thể nằm trong cùng hộp mặt. Điểm mũi tối thiểu 0,2 là
giới hạn có sẵn. Mọi điều kiện mặt phải áp dụng cho cùng một mặt. Phần cắt tính
theo ảnh hữu ích, không tính đệm đen. Biên điểm/kích thước/cơ thể dùng float32
như Rust. Giá trị lớn hơn 1 là đối chứng loại tất cả, không phải cấu hình sử dụng.

## Kết quả ngưỡng đã khóa

| Họ có 0 FP khi hiệu chỉnh | Ngưỡng khóa | Người công khai/dựng khi hiệu chỉnh | Người công khai/dựng khi kiểm tra | FP vật thể kiểm tra |
|---|---|---:|---:|---:|
| Điểm mặt | ≥.95 | 0/67; 0/36 | 0/67; 0/36 | 0/960 |
| Chiều cao mặt | ≥1.10 ảnh | 0/67; 0/36 | 0/67; 0/36 | 0/960 |
| Điểm cơ thể | ≥.70 | 0/67; 0/36 | 0/67; 0/36 | 0/960 |
| Điểm mặt+kích thước+không cắt | ≥.60; ≥1.10; 0 | 0/67; 0/36 | 0/67; 0/36 | 0/960 |
| Mặt+cơ thể, có/không ghép mũi | ≥.60; cao ≥.30; không cắt; cơ thể ≥.45 | 1/67; 4/36 | 1/67; 6/36 | 3/960 |

Tất cả hàng chọn có 0/956 FP khi hiệu chỉnh nhưng đều không giữ đủ người. Chỉ
xét phần cắt không có cấu hình 0 FP. Ba FP kiểm tra là hai ảnh in và một màn
hình. Không sửa ngưỡng sau khi khóa. Lưới kiểm tra đầy đủ chỉ để mô tả, không
chọn điểm hoạt động. Thất bại trên lưới không chứng minh mọi bộ phân loại đều thất bại.

## Ngưỡng hiện tại và cấu hình dự kiến

Các điểm dưới đây đã nằm trong lưới ban đầu, không chọn vì kết quả kiểm tra tốt.

| Luật trên tập kiểm tra | Người công khai | Người dựng | FP vật thể | Mọi điều kiện người |
|---|---:|---:|---:|---:|
| YuNet hiện tại .60 | 45/67 | 36/36 | 283/960 | 298/852 |
| Điểm mặt ≥.80 | 34/67 | 36/36 | 193/960 | 199/852 |
| Mặt ≥.80; cao ≥.06; không cắt | 33/67 | 36/36 | 191/960 | 171/852 |
| Trên + cơ thể ≥.20, mũi cùng mặt | 32/67 | 36/36 | 186/960 | 164/852 |
| Chỉ cơ thể ≥.20 | 65/67 | 36/36 | 629/960 | 673/852 |
| Chỉ cơ thể ≥.30 | 60/67 | 36/36 | 454/960 | 443/852 |

Luật mặt dự kiến mất 127 trong 298 trường hợp người mà ngưỡng hiện tại tìm được.
Trong 45 trường hợp chính công khai được ngưỡng hiện tại nhận, 44 có mũi chuẩn
nằm trong hộp mặt đạt điều kiện; luật mặt dự kiến có 33/33. Chỉ số hiện diện
không được đổi tên thành độ chính xác định vị đúng người hoặc nhận dạng danh tính.

| Họ âm tính kiểm tra | Ngưỡng hiện tại | Luật mặt dự kiến |
|---|---:|---:|
| Màn hình ghép | 139/268 | 94/268 |
| Ảnh in ghép | 138/268 | 95/268 |
| Cảnh vật công khai | 6/308 | 2/308 |
| Ảnh có khung; màn hình trò chơi | 0/4; 0/4 | 0/4; 0/4 |
| Tượng; tai nghe; đèn tròn | Mỗi họ 0/36 | Mỗi họ 0/36 |

277/283 FP ban đầu và 189/191 FP của luật mặt là ảnh in/màn hình ghép. Mô hình
có thể nhận đúng hình người nhưng kết luận sai về hiện diện vật lý. Trung bình
điểm 17 khớp không phải xác suất có người sống. Chưa đánh giá chuyển động người,
màn hình chuyển động hoặc chuyển động camera của chính đèn.

## Thực thi, kiểm tra và độ trễ

YuNet dùng đúng hash mô hình sản phẩm và hình học đệm trên-trái 320×256. Bộ thực
thi OpenCV CPU không được coi là tương đương từng bit với tract. MoveNet là
Lightning **float16 TFLite v4** công khai được chấp thuận, không phải int16 NPU
đang có trong sản phẩm. Mỗi ảnh độc lập dùng vùng vuông toàn ảnh 192×192, không
mang trạng thái theo dõi từ ảnh trước.

14 kiểm tra adapter đạt: hai fixture giải mã sai lệch dưới 0,000099 pixel mô
hình; bốn fixture crop khớp từng byte; ba fixture theo dõi sai lệch dưới 8e-8,
cùng kiểm tra hình học và hai ảnh trống tạo sẵn. Chín kiểm tra chấm điểm đạt.
Kiểm toán sau chạy đạt 77 kiểm tra, đếm lại độc lập bằng mã scalar toàn bộ
11.284 hàng lưới, xác nhận thứ tự chọn và hash nguồn/đầu vào. Đây là mã khác của
cùng người thực hiện, không phải reviewer độc lập. Không đo tương đương suy luận
float/int16; fixture số không có ảnh đầu vào gốc.

Máy dùng chung Intel iMac20,1/i7-10700K/64 GiB, macOS 26.6.2 x86_64; Python
3.11.13, LiteRT 1.0.1 và XNNPACK CPU, NumPy 1.26.4, OpenCV-headless 4.10.0.84,
Pillow 10.4.0. OpenBLAS và LiteRT yêu cầu một luồng; OpenCV yêu cầu một nhưng
báo 16. Không cô lập máy khỏi công việc khác.

Biên độ trễ là **RGB đã giải mã đến các đặc trưng đầu ra**, không gồm đọc file,
giải mã JPEG hoặc tạo mô hình. Tạo mô hình 34,839 ms; ảnh đầu 22,226 ms. Không
bỏ mẫu khởi động. 3.635 ảnh tiếp theo có trung vị/p95 17,952/21,890 ms, tối đa
42,719 ms. Riêng gọi MoveNet có trung vị/p95 7,025/8,278 ms. Không phải phép đo
ARM64 NPU, camera→bus, đường âm thanh hoặc tải giọng nói.

## Giấy phép, giới hạn và bước tiếp

[Trang đúng phiên bản MoveNet](https://raw.githubusercontent.com/tensorflow/tfhub.dev/master/assets/docs/google/models/movenet/singlepose/lightning/tflite/float16/lite/4.md)
ghi Apache 2.0 cho trọng số. Model card nêu COCO train2017 và video thể dục để
huấn luyện, COCO val2017 để đánh giá. Đây không phải miền kiểm tra mới độc lập.
YuNet theo MIT trong [thư mục OpenCV Zoo](https://github.com/opencv/opencv_zoo/tree/main/models/face_detection_yunet).
Nhân vật cũ theo CC0; hình học vật thể do tác giả tạo.
[Điều khoản COCO](https://raw.githubusercontent.com/cocodataset/cocodataset.github.io/master/dataset/termsofuse.htm)
ghi CC BY 4.0 cho nhãn; giấy phép ảnh riêng. Chỉ chọn ID giấy phép 4, 5, 7, 8
(CC BY 2.0, CC BY-SA 2.0, không có hạn chế bản quyền đã biết, tác phẩm chính phủ
Mỹ). Metadata/ghi công từng ảnh được giữ cục bộ; không chọn NC/ND. Không đưa ảnh,
trọng số, tên người hoặc bản ghi riêng vào git. Giữ toàn bộ nguồn, kết quả, lần
chuẩn bị lỗi và receipt cục bộ; chỉ công bố số tổng hợp.

Không có khoảng tin cậy chứng minh FP quần thể bằng 0 hoặc giữ 99%. Tiếp tục
phần đo và API chất lượng dùng chung nhưng giữ xác nhận hiện diện tắt. Cần kiểm
tra nhiều mặt cùng khung, lịch sử hết hạn, epoch camera/riêng tư, chuyển động
camera và màn hình chuyển động. Tests/First phụ trách ROW vật lý: phòng trống
qua đêm/ngày và những lần người ngồi thật. Ngưỡng kích thước/góc cho nhận dạng
và ngưỡng mờ là việc riêng, chưa được xác lập bởi nghiên cứu này.

Hash protocol, manifest, dự đoán, ngưỡng, kiểm toán và tổng hợp nằm trong
[numeric.json](numeric.json); bảng hash và cách đọc đầy đủ ở [bản tiếng Anh](README.md).
