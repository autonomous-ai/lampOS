# EVAL: phương pháp so với đầu người dựng hình

Ngày 28 tháng 11 năm 2026; tiếp nối [EVAL PnP đã không đạt](README_vi.md).

**SET_BAR: FAIL cho cả hai ứng viên. Chưa đủ điều kiện, không huấn luyện, không đổi sản phẩm và không truy cập thiết bị.** PnP có sai số đáng kể trên nhãn ảnh thật; 6DRepNet gốc tốt hơn nhiều trên ảnh thật nhưng kém hơn nhiều trên bộ dựng hình. Kết quả gợi ý cả giới hạn phương pháp lẫn khác biệt phân bố ảnh dựng/ảnh thật; không chứng minh nhãn dựng sai, không tách riêng nguyên nhân, không chứng minh độ chính xác trên đèn. Kiểm tra hình học trước đó vẫn giữ nguyên. Đây là một mô hình đối chứng đã công bố, không phải khẳng định mô hình tốt nhất năm 2026.

## Giao thức cố định và mẫu số

[Giao thức cuối](https://github.com/autonomous-ai/lampOS/issues/8847#issuecomment-7240908034) được công bố trước suy luận mới. Tái sử dụng nguyên dự đoán PnP trên ảnh dựng; chạy nguyên lõi PnP một lần trên 2.000 ảnh thật; chạy 6DRepNet độc lập một lần trên mỗi bộ. Không chọn lại ngưỡng, huấn luyện hoặc bỏ mẫu kiểm thử. Giữ cả 36 nhãn thật vượt ±90° ở ít nhất một trục Euler gốc. Hai lần gọi detector độc lập cho kết quả giống nhau trên cả 2.000 ảnh thật; 6DRepNet không phụ thuộc việc PnP thành công.

Nón đã chọn cố định: `abs(yaw) <= 63`, `-3 <= pitch_up <= 10`. Nón chẩn đoán tham chiếu đã khai báo: `abs(yaw) <= 60`, `-20 <= pitch_up <= 25`; nhãn lớp ảnh dựng dùng nón tham chiếu. Nếu dự đoán đúng hoàn toàn góc tia chuẩn, nón hẹp chỉ bắt **360/648 (55,56%)** trường hợp hướng vào và loại 648/648 hướng ra. Nón tham chiếu đạt 648/648 cả hai chiều. Đây là lệch quy tắc quyết định đối với ước lượng đã hiệu chuẩn, không phải chặn trên toán học cho mọi mô hình có sai lệch. Không thay nón sau kết quả.

Ảnh dựng có 1.296 mẫu chính, 648 mỗi lớp, cùng 72 mẫu biên riêng. Mỗi lớp phải đạt 616/648; mỗi lớp trong từng ô khoảng cách/ánh sáng phải đạt 103/108. Mọi unknown tính là thất bại trong mẫu số gốc, không thành loại hướng ra đúng. Các ảnh dựng tương quan không phải người độc lập. Ảnh thật không có khoảng cách, ánh sáng, nội tại camera hay tia mũi đến camera được đo; không tạo các nhãn đó.

## Tia mũi đến camera thật trong ảnh dựng

Các cột lần lượt là phương pháp/bộ; bắt hướng vào và loại hướng ra với nón cố định; hai số với nón tham chiếu; unknown.

| Method / set | Locked facing | Locked away | Reference facing | Reference away | Unknown |
| --- | ---: | ---: | ---: | ---: | ---: |
| PnP / renders | 302/648 (46.60%) | 445/648 (68.67%) | 620/648 (95.68%) | 260/648 (40.12%) | 43 |
| 6DRepNet / renders | 311/648 (47.99%) | 500/648 (77.16%) | 526/648 (81.17%) | 334/648 (51.54%) | 40 |

Mỗi ô dưới đây đều không đạt ở cả hai phương pháp. Các số trong ô là bắt hướng vào / loại hướng ra / unknown.

| Distance / light | PnP facing / away / unknown | 6DRepNet facing / away / unknown |
| --- | ---: | ---: |
| 0.5 m / day | 49/108 · 91/108 · 2 | 47/108 · 93/108 · 0 |
| 0.5 m / warm dim | 57/108 · 83/108 · 9 | 51/108 · 84/108 · 8 |
| 1 m / day | 49/108 · 84/108 · 6 | 51/108 · 88/108 · 6 |
| 1 m / warm dim | 46/108 · 68/108 · 5 | 53/108 · 88/108 · 5 |
| 1.5 m / day | 45/108 · 63/108 · 8 | 54/108 · 75/108 · 8 |
| 1.5 m / warm dim | 56/108 · 56/108 · 13 | 55/108 · 72/108 · 13 |

Mẫu biên riêng: PnP 17/48 hướng vào, 16/24 hướng ra; 6DRepNet 15/48 và 18/24; không có unknown. Chúng không tham gia mẫu số chính hay chọn ngưỡng. PnP chính có 36 không thấy mặt, bốn nhiều mặt và ba nghiệm quay ngược. 6DRepNet giữ các lỗi detector đó, không có ba lỗi riêng của PnP.

## Chẩn đoán chung theo pháp tuyến camera

Hai bên dùng pháp tuyến mặt so với trục quang học, không phải tia lệch trục đến ống kính. Với ma trận CV vật lý R: `h = R * [0,0,-1]`, `yaw = atan2(h.x,-h.z)`, `pitch_up = atan2(-h.y,hypot(h.x,h.z))`. Nhãn thật có 1.514 trong nón và 486 ngoài nón tham chiếu. Nhãn camera của ảnh dựng có 648 mỗi lớp nhưng không nhất thiết cùng mẫu với nhãn tia.

| Method / set | Locked facing | Locked away | Reference facing | Reference away | Unknown |
| --- | ---: | ---: | ---: | ---: | ---: |
| PnP / renders | 34/648 (5.25%) | 606/648 (93.52%) | 606/648 (93.52%) | 347/648 (53.55%) | 43 |
| 6DRepNet / renders | 235/648 (36.27%) | 520/648 (80.25%) | 500/648 (77.16%) | 354/648 (54.63%) | 40 |
| PnP / AFLW2000 | 1185/1514 (78.27%) | 210/486 (43.21%) | 1451/1514 (95.84%) | 118/486 (24.28%) | 120 |
| 6DRepNet / AFLW2000 | 408/1514 (26.95%) | 436/486 (89.71%) | 1412/1514 (93.26%) | 359/486 (73.87%) | 87 |

Unknown thật: PnP 120/2.000; 6DRepNet 87/2.000. Cả hai có 52 nhiều mặt, bốn mặt bị cắt, 31 không thấy mặt; PnP thêm 33 nghiệm quay ngược. Cả ba lượt mới không có ngoại lệ xử lý, mất đầu vào hoặc bỏ hàng.

## Sai số quay gốc, chỉ trên đầu ra đo được

Các cột: phương pháp/bộ; đo được/kế hoạch; MAE yaw, pitch, roll; sai số SO(3) trung bình.

| Method / set | Measured / planned | Yaw MAE | Pitch MAE | Roll MAE | Mean SO(3) error |
| --- | ---: | ---: | ---: | ---: | ---: |
| PnP / renders | 1253/1296 | 11.06° | 23.48° | 18.31° | 19.44° |
| 6DRepNet / renders | 1256/1296 | 15.38° | 20.59° | 16.57° | 21.45° |
| PnP / AFLW2000 | 1880/2000 | 7.49° | 14.94° | 10.28° | 17.21° |
| 6DRepNet / AFLW2000 | 1913/2000 | 3.13° | 6.09° | 4.72° | 5.56° |

Quy ước gốc: `M = Rz(roll) Ry(yaw) Rx(pitch)`; CV vật lý là `D Mᵀ D`, `D = diag(1,-1,-1)`. Chuyển đổi từ mã nguồn đã cố định trước suy luận và đối chiếu đại số vẽ trục cùng mã quay trong bản sao dữ liệu. Sai số Euler là chênh lệch tuyệt đối, không quấn hoặc chặn. SO(3) riêng chỉ chặn đối số acos để tránh sai số dấu phẩy động. Mọi công bố sai số phải kèm độ phủ đầu ra đo được.

Tia dự đoán thật so với nhãn camera dưới đây chỉ là **proxy khác hệ quy chiếu**; không phải độ chính xác góc tia hay nhãn hướng vào camera:

| Method / set | Locked facing | Locked away | Reference facing | Reference away | Unknown |
| --- | ---: | ---: | ---: | ---: | ---: |
| PnP / AFLW2000 | 535/1514 (35.34%) | 345/486 (70.99%) | 1422/1514 (93.92%) | 176/486 (36.21%) | 120 |
| 6DRepNet / AFLW2000 | 172/1514 (11.36%) | 443/486 (91.15%) | 1115/1514 (73.65%) | 417/486 (85.80%) | 87 |

## Thời gian CPU và bộ chuyển đổi

Các cột: phương pháp/bộ; lần đầu, trung vị các lần sau, p95 các lần sau (ms); số lần sau.

| Method / set | First call, ms | Later median, ms | Later p95, ms | Later calls |
| --- | ---: | ---: | ---: | ---: |
| PnP / renders | 14.288 | 13.450 | 18.815 | 1367 |
| 6DRepNet / renders | 211.032 | 168.749 | 228.435 | 1367 |
| PnP / AFLW2000 | 12.863 | 8.248 | 13.299 | 1999 |
| 6DRepNet / AFLW2000 | 149.299 | 154.442 | 247.282 | 1999 |

Đo từ RGB đã giải mã đến xong detector, crop, mô hình và hình học. Đọc/kiểm hash/giải mã và nạp mô hình được ghi riêng trong [JSON tổng hợp](method-comparison-numeric.json). Lần đầu là cold, không có lượt làm nóng thêm. Số PnP ảnh dựng là số lịch sử, chưa gồm đại số camera/Euler bổ sung. Thời gian ảnh dựng gồm 72 mẫu biên; độ chính xác chính không gồm. Thời gian mọi lần gọi giữ cả detector từ chối sớm; JSON còn có thời gian riêng khi gọi mô hình pose.

Máy Intel iMac20,1, i7-10700K, 64 GiB, macOS 26.6.2; Python 3.11.13, NumPy 1.26.4, OpenCV 4.10.0, Pillow 10.4.0, ONNX Runtime 1.18.1. ORT CPU, batch một, tuần tự, một luồng intra/inter; OpenBLAS đặt một. OpenCV yêu cầu một nhưng báo 16 với GCD. Máy dùng chung, không cô lập; đây không phải độ trễ ARM, camera đến công bố, hay tác động giọng nói.

6DRepNet dùng YuNet độc lập giống hệt (score 0,6; NMS 0,3; tối thiểu bốn pixel; đúng một hộp không bị cắt), padding chéo trục 20% theo demo gốc, RGB PIL bilinear Resize(224)/CenterCrop(224), chuẩn hóa ImageNet. Không dùng hộp hay landmark chuẩn. Bộ chuyển đổi PIL mới khác benchmark Rust bilinear trước đó, không chứng minh tương đương sản phẩm. ONNX FP32 batch một opset 13 có 157.316.333 byte; checkpoint có 157.315.191 byte.

## Nguồn, giấy phép và giới hạn tái lập

[Mã 6DRepNet gốc](https://github.com/thohemp/6DRepNet/tree/464b2ba55c3d9b3d3b707c6271cf329a983ded20) dùng MIT; tác giả [xác nhận MIT cho trọng số](https://github.com/thohemp/6DRepNet/issues/27#issuecomment-1214677538). [README](https://github.com/thohemp/6DRepNet/blob/464b2ba55c3d9b3d3b707c6271cf329a983ded20/README.MD) nêu huấn luyện 300W-LP, đánh giá AFLW2000; mã huấn luyện khởi tạo backbone RepVGG-B1g2 ImageNet. Chưa kiểm độc lập trùng ảnh/danh tính huấn luyện-kiểm thử. Tên checkpoint không chứng minh huấn luyện trên AFLW2000. [Điều khoản YuNet, mặt chuẩn và tài sản dựng](METHOD_vi.md) không đổi.

Toàn bộ 2.000 cặp JPEG/MAT lấy từ [bản sao AFLW2000-3D phiên bản 1](https://www.kaggle.com/datasets/mohamedadlyi/aflw2000-3d) không cần tài khoản, sau khi HTTP gốc trả 404 và HTTPS hết hạn. ZIP SHA-256 `97b7b41068f1757dc82ac5f01ad2bc5d15f0f83bffab96ee56607537e3cd7f90`, 87.473.487 byte; khác checksum ZIP TFDS gốc nên không khẳng định xác thực từng byte từ nhà phát hành. [Điều khoản AFLW gốc](https://www.tugraz.at/institute/icg/research/team-bischof/learning-recognition-surveillance/downloads/aflw/) giới hạn nghiên cứu phi thương mại và phân phối lại; metadata bản sao không ghi đè điều khoản. Đây là chẩn đoán nghiên cứu, không xác nhận quyền dùng dữ liệu trong sản phẩm. Không công bố ảnh, nhãn, dự đoán từng ảnh, script hay đường dẫn riêng. Trích dẫn AFLW: *Annotated Facial Landmarks in the Wild: A Large-scale, Real-world Database for Facial Landmark Localization*, hội thảo ICCV, 2011.

Đạt 21 kiểm tra adapter, tám kiểm tra chấm điểm, 11 kiểm tra ma trận; bốn tensor tự tạo khớp PyTorch gốc trong 0,001°; ba mảng RGB tiền xử lý khớp hoàn toàn. Phép đếm độc lập đạt 53 kiểm tra, không import scorer, mở ảnh hoặc gọi mô hình: độ đủ/thứ tự/hash, mọi số lớp/ô chính, MAE Euler gốc, không tạo MAE cho proxy, detector thật giống hệt. Đây là kiểm tra bằng mã độc lập do cùng builder làm, không phải xác nhận của reviewer độc lập.

| Artifact | SHA-256 |
| --- | --- |
| Frozen comparison protocol | `94a984770b293455c87b13d3614c937e2110f348c0fc528e3f57bfc6ca78061d` |
| Real adapter manifest | `47e466c7bdec8f9570eb07aa001544b37f5179876b6855a776cfb096ebaa3739` |
| Synthetic adapter manifest | `4fbe3bf5f311299459effad2929630cd1621c72b279b1006259685bf550f95d8` |
| 6DRepNet checkpoint | `9bdcd0e072df0d21e92288b40ea1d1b3cf7173c44728ab9c45c4d673f361e018` |
| 6DRepNet ONNX export | `8b9236159f30c7285d8b8e0b1f0cf5bbf2506f35a256e4cb62119ed7a9ffa1c6` |
| Private complete report | `2fbb7c8e9e699def4efff935a729e7fd332de8fb8b279acd50aed51f2c3e5f8b` |
| PnP real predictions | `f3b250b6715c2a063edad03364cc42b5b234815b3865c043660601db7793a232` |
| 6DRepNet real predictions | `283bf05fdd1419a78161946ed782ae8908c61159f3a15e6116d6c0c0f4a577c2` |
| 6DRepNet synthetic predictions | `1ba9cb7144cbd799efc16f02d3d9956e4b3cdc8b75da4b309a3878ef97bbe1ac` |


Giao thức riêng còn buộc hash nguồn/runtime/đầu vào, bản sao, oracle và dữ liệu đã xem trước. Tái lập cần quyền dùng đầu vào chính xác và mã nguồn đã giữ; chỉ JSON tổng hợp không đủ tái lập suy luận. Giữ nguyên mọi thất bại trước đó và metadata không có.

[EVAL gốc](README_vi.md) · [English comparison](method-comparison.md) · [Số tổng hợp](method-comparison-numeric.json)
