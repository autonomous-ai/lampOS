# Đánh giá mô hình chống giả mạo: chưa đạt (#8971)

Ngày 28 tháng 11 năm 2026, Fourth. [English](README.md),
[bằng chứng số](numeric.json). Đây là đánh giá ngoại tuyến: không thay đổi sản
phẩm, huấn luyện, truy cập đèn, triển khai hay phê duyệt ngưỡng hoạt động.

**MiniFASNetV2 không đạt đồng thời không nhận nhầm vật thể và giữ lại ≥95% cảnh
người thật trong bộ này.** Ngưỡng chọn từ tập hiệu chỉnh loại bỏ tất cả người
thật. Quy tắc argmax cố định nhận nhầm ảnh in và bỏ sót người. Độ trễ p95 trên
máy chủ thử cũng vượt mục tiêu. Hiện diện tiếp tục chỉ là quan sát.

## Quy trình và mô hình đã chốt

[Quy trình được đăng trước suy luận](https://github.com/autonomous-ai/lampOS/issues/8971#issuecomment-7242335162),
SHA-256 `8bd405357881e3c767309c635e25cd69ac0b399febc68778d2b31a94e8e75df2`.
[Ngưỡng hiệu chỉnh được đăng trước khi chấm tập kiểm tra](https://github.com/autonomous-ai/lampOS/issues/8971#issuecomment-7242353570),
SHA-256 `84625408a02ee37fe7b79828bb5483e6b47ecac0d1dfd2a530bad5a03dbe8beb`.
Không đổi ngưỡng, mô hình hoặc lựa chọn đầu vào sau đó.

Dùng đúng một [checkpoint Minivision](https://github.com/minivision-ai/Silent-Face-Anti-Spoofing/tree/b6d5f04ad78778917853b25c778acef6d5626d15):
`2.7_80x80_MiniFASNetV2.pth`, 1.849.453 byte, 434.560 tham số, SHA-256
`a5eb02e1843f19b5386b953cc4c9f011c3f985d0ee2bb9819eea9a142099bec0`.
Mã và trọng số nằm trong kho Apache-2.0; chưa thấy hạn chế riêng cho trọng số.
Các câu hỏi xác nhận giấy phép checkpoint chưa được trả lời. Tác giả nói dữ
liệu [tự thu thập cho tình huống ứng dụng](https://github.com/minivision-ai/Silent-Face-Anti-Spoofing/issues/25#issuecomment-678321691),
[khoảng năm triệu ảnh mỗi lớp](https://github.com/minivision-ai/Silent-Face-Anti-Spoofing/issues/25#issuecomment-678095382),
không dùng bộ huấn luyện mã nguồn mở. Không thể kiểm toán độc lập sự trùng lặp,
đồng ý hoặc nguồn gốc dữ liệu. Đánh giá này không phê duyệt phân phối mô hình.

Giữ phép mở rộng vùng 2,7 lần, dịch vùng ở biên, cắt gồm pixel cuối và nội suy
tuyến tính của nhà phát hành về 80×80. Đầu vào **BGR float32 0–255**, không chia
255. Lớp 1 là thật. Chỉ nạp tensor, đối chiếu chặt cấu trúc đã đọc. Đây không
phải tổ hợp hai mô hình của bản demo. Detector là YuNet đã chốt ở ngưỡng 0,6,
không thêm RetinaFace.

Đường cơ sở nhận khi argmax là lớp 1. Quy tắc hiệu chỉnh thêm `p_real ≥ t`;
lưới 0..1 bước 0,01 và 1,0000001 để loại tất cả. Chỉ dùng tập hiệu chỉnh:
không nhận nhầm vật thể, không có dòng lỗi xử lý, tối đa số cảnh người chính
được giữ, hòa thì chọn t thấp hơn. **t=1,00** được chọn: giữ 0/67 người, nhận
nhầm 0/956 vật. Cơ sở đạt 34/67 và 196/956. Không điểm lưới nào đạt; không suy
ra rằng mọi mô hình hay họ ngưỡng khác đều thất bại.

## Dữ liệu và mẫu số

Dùng lại toàn bộ **3.636 ảnh 5b** cùng kết quả detector từ
[đánh giá tĩnh](https://github.com/autonomous-ai/lampOS/pull/8980), giữ nguyên
chia nhóm nguồn hiệu chỉnh/kiểm tra. Kết quả tĩnh trước đó đã biết; không gọi
đây là bộ hoàn toàn chưa từng thấy. Tập kiểm tra có 67 cảnh người chính, YuNet
phát hiện 45. 22 cảnh bỏ sót vẫn nằm trong mẫu số toàn bộ đường xử lý; bộ lọc
chỉ loại bỏ không thể khôi phục chúng. Tỷ lệ có điều kiện được báo riêng.

Bộ ngoài có **23 tệp nguồn, 129 biến thể**, chỉ dùng kiểm tra:

- [Mẫu công khai CC-BY-4.0 của Axon Labs](https://huggingface.co/datasets/AxonData/Display_replay_attacks/tree/f5560f659e185ded62c2f09ff502426b0588406e):
  toàn bộ năm ảnh thật và năm video màn hình. Lấy trước năm vị trí 10/30/50/70/90%
  mỗi video: 25 khung tương quan, không phải 25 video độc lập. Không truy cập bộ
  đầy đủ trả phí.
- [Bản lọc CC-BY-4.0 của Persona](https://huggingface.co/datasets/withpersona/persona-fas-bench-v1/tree/726d025dc1d187f553a3faff3e438d879e7a57e8):
  toàn bộ 13 ảnh `physical_photo` đang có, không phải cả benchmark. Thẻ gốc ghi
  14, bản lọc còn 13. Một ảnh có phần mặt người cầm phía sau ảnh in; ba biến thể
  được gắn nhãn chưa rõ trước suy luận và giữ lại. Chỉ **12** nguồn là mẫu âm
  ảnh in không mơ hồ.

Thu toàn khung vào 320×240, không phóng lớn, tỷ lệ tương đối 1/.65/.4, viền đen,
JPEG Q85 4:2:0. Tỷ lệ 1 là chính, hai tỷ lệ nhỏ để chẩn đoán. Đây không phải
phép đo khoảng cách, quang học, ánh sáng hay JPEG thật của đèn. Manifest đã
duyệt có SHA-256
`6b6baca67a7871f30f0b42beb3423a47630238ac4fb9b9e1ee5ff354920d1099`.

Đã xem nội dung so với nhãn nhà phát hành, không chứng thực độc lập cách chụp.
Danh tính và tính độc lập giữa người chưa được xác minh. Người dựng không phải
người thật được chụp cho PAD; màn hình/ảnh in ghép của 5b không phải chụp lại
vật thật. Mốc ≥99% của 5b tĩnh giữ nguyên; bài tiếp theo dùng mốc ≥95% của lead.
Không công bố ảnh, danh tính, bản ghi từng mẫu hay trọng số.

## Kết quả kiểm tra

| Quy tắc | Cảnh người 5b được giữ | Khi đã có mặt được phát hiện | Cảnh vật 5b được nhận |
|---|---:|---:|---:|
| YuNet hiện có ≥0,6 | 45/67 (67,2%) | 45/45 | 283/960 (29,5%) |
| MiniFAS argmax đơn | 27/67 (40,3%) | 27/45 (60,0%) | 152/960 (15,8%) |
| t=1,00 đã chốt | 0/67 | 0/45 | 0/960 |

152 lỗi của cơ sở gồm 74/268 cảnh màn hình ghép, 77/268 ảnh in ghép, 1/308 cảnh
vật công khai. Các họ vật khác không có mặt được nhận, thường vì YuNet không
tìm thấy mặt; không phải thành công riêng của PAD. Nhận 33/36 người dựng chỉ
là chẩn đoán, không chứng minh người sống. Không có lỗi xử lý.

| Tỷ lệ ngoài; cơ sở | Ảnh thật được giữ | Video màn hình có ít nhất một khung được nhận | Ảnh in âm rõ ràng được nhận |
|---|---:|---:|---:|
| 1,0, chính | 3/5 | 0/5 (0/25 khung) | 12/12 |
| .65 | 0/5 | 0/5 | 0/12 |
| .4 | 0/5 | 0/5 | 0/12 |

Ảnh in có người phía sau được nhận ở tỷ lệ 1 nhưng không tính là lỗi phòng
trống. t=1 loại mọi biến thể ngoài, gồm cả năm ảnh thật ở mỗi tỷ lệ. Không nhận
tấn công nhưng cũng không giữ người thật không phải thành công. Năm nguồn dương
không đủ kết luận độ tin cậy toàn đội đèn; khung lặp và biến thể có tương quan.

Detector tìm thấy mặt ở mọi biến thể ảnh thật ngoài. Tỷ lệ 1 có khoảng cách mắt
15–30 px (ba mặt), 40–48 (một), 48–60 (một); .65 có 15–30 (bốn), 30–40 (một);
.4 có 0–15 (ba), 15–30 (hai). Bằng chứng số chia mọi mặt được phát hiện thành
0–15/15–30/30–40/40–48/48–60/60–80/80+ px. Đây là khoảng cách mốc quan sát được,
không đảm bảo chi tiết quang học. Không tự gán cỡ cho mặt bị bỏ sót.

## Độ trễ và xác minh

iMac20,1/i7-10700K dùng chung, macOS x86_64; Python 3.11.13, PyTorch 2.2.2,
OpenCV 4.10.0, NumPy 1.26.4. Torch intra/inter-op một luồng. Yêu cầu OpenCV một
luồng nhưng API báo 16; không coi đây là máy dành riêng cho phép đo.

Ranh giới **RGB đã giải mã + hộp mặt → cắt/tensor/mô hình/softmax**, không gồm
JPEG, detector, camera, lập lịch hay phát kết quả. Khởi tạo 172,705 ms, lần mặt
đầu 77,101 ms. Giữ toàn bộ 1.434 lần sau: trung vị **10,812 ms**, p95
**19,719 ms**, tối đa **83,276 ms**. **Không đạt** mục tiêu p95 ≤10 ms đã nêu.
Detector ngoài riêng: đầu 29,086 ms, trung vị/p95 sau 9,334/18,125 ms. Không cộng
hai phân vị để tuyên bố phân vị toàn tuyến. Chưa đo ARM64/NPU, tải giọng nói hay
thời gian xác nhận hiện diện thật.

10 kiểm tra tiền xử lý/chấm điểm đạt. Kiểm toán vô hướng riêng, cùng builder
nhưng cách tính khác, không nhập scorer hay chạy lại mô hình, đạt **15.794
assertion**: hash, thứ tự mẫu, xác suất, chọn ngưỡng, tổng kiểm tra/nhóm nguồn,
phân vị. Không có ảnh trùng byte hoặc dòng bị bỏ. Giữ cục bộ 3.765 dòng, 1.435
kết quả mặt, nguồn, trọng số, quy trình, script và thời gian. Chỉ công bố số
tổng hợp, nguồn và hash script.

## Phương án theo thời gian: chưa đạt chuẩn

Có thể dùng điểm Blendshape V2 9/10 (`eyeBlink`) cho chuỗi mở→nhắm→mở cùng một
track, ngưỡng trễ và thời gian khung tăng nghiêm ngặt. Bộ đọc biểu cảm hiện chỉ
đọc tối đa mỗi hai giây (`crates/eyes/src/seems.rs::READ_EVERY`), chưa phải bộ
lấy mẫu chớp mắt đã kiểm chứng. Cần xem đường mouth/mesh và quy tắc đồng ý trước
khi nối. Video phát lại cũng có chớp mắt; nghiên cứu còn vượt qua kiểm tra này
bằng ảnh in thao tác được ([Hofbauer và cộng sự, 2021](https://doi.org/10.1049/bme2.12015)).
Chớp mắt một mình không chứng minh hiện diện vật lý.

Thị sai cần dịch chuyển camera đã đo, hiệu chỉnh và chuyển động dư phụ thuộc
độ sâu sau bù chuyển động camera/mặt phẳng. Dịch tâm hộp hay chỉ quay đầu không
đủ. `speaking::JointHistory` nối khớp với thời gian chụp, chưa phải bộ ước lượng
độ sâu/thị sai. Cần đối chứng màn hình chuyển động, ảnh in cong, dữ liệu khớp
thiếu/cũ. Chỉ tận dụng một cái liếc tự nhiên vốn xảy ra trong phép thử được
phép sau này; không thêm chuyển động tự động cho đánh giá này.

**Chưa đo độ trễ trước hiện diện cho cả hai phương án.** Phải chờ hết chuỗi chớp
mắt, hoặc đủ chuyển động tự nhiên và nhiều khung hợp lệ. Không có sự kiện thì
không đảm bảo phát hiện trong thời gian hữu hạn. Hết hạn N cấu hình được trả về
chưa rõ, không tự tạo hiện diện hay khẳng định vắng mặt. Trước khi chọn N, đo
p50/p95 từ ứng viên đầu→xác nhận, tỷ lệ chưa rõ, giữ người thật tại hạn 2/5/10
giây trên bộ thời gian mới đã chốt. Đây là các mốc thử đề xuất, không phải giới
hạn sản phẩm đã đạt. Bộ này không có video chớp mắt thật đã gắn nhãn hoặc chân
lý chuyển động camera đèn, nên không bịa số đo.
