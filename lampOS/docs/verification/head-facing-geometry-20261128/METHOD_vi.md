# Cảm biến 4: đánh giá hình học hướng đầu đã đóng băng

Đây là đánh giá ngoại tuyến, dùng năm điểm mốc của bộ phát hiện YuNet có sẵn,
một khuôn mặt 3D tổng quát cố định và OpenCV solvePnP. Không huấn luyện mô hình
mới, cài đặt, bật hoặc thay đổi tính năng trên đèn. Báo cáo đánh giá kèm theo
ghi kết quả; kiểm tra mã nguồn không tự chứng minh độ chính xác.

Các công cụ đo bằng Python, tệp mô hình, ảnh, kế hoạch đầy đủ, manifest dữ liệu
và cấu hình runtime được giữ riêng. Quy tắc đóng góp của kho chỉ cho phép đưa
mã Python huấn luyện tạo mô hình lên kho; đánh giá này không thuộc ngoại lệ đó.
Tên tệp dưới đây mô tả bản lưu riêng, không phải liên kết tải mã công khai.

## Phương pháp và danh mục mã nguồn

Ứng viên chính đã đóng băng là `yunet_five-nose-ray-hfov70-minbox4-v2`.
Phiên bản mã nguồn là `opencv-pnp-canonical-nose-ray-hfov70-minbox4-v2`.

| Tệp lưu riêng | Vai trò | SHA-256 |
| --- | --- | --- |
| `detector.py` | Đầu vào YuNet, giải mã và điều kiện nhận khuôn mặt | `a31c9b098b45a1778324237458613182fa78ebadf035d0954217a5d3de01d334` |
| `geometry.py` | Điểm tương ứng chuẩn, PnP và quy ước góc | `dad6f51b08af309e7546ba5f43f62e273a706873529555848cb3ae86d3dd2430` |
| `mesh_model.py` | Đối chứng mesh tùy chọn; luồng chính không import | `c04d74f0f5835671a98a4b6ddf5215d71bd37d5ee165394bb069ac9b0b1ffb6a` |
| `replay.py` | Replay có khóa, kết quả không xác định, đầu ra gốc và thời gian | `a188daca80f05fc5ca0ac0374d7a851af0418c9363221abe68dbad68012c2f45` |
| `flatten_pnp.py` | Bộ chuyển đầu vào bộ chấm có ràng buộc hash | `b9b5212e0216e43ab0d2b4b8f9f5f72b3d956f94e7f5d1d085b0c056363ba25e` |
| `score.py` | Chọn ngưỡng chỉ trên tập tune và chấm test một lần | `fd767b0e84b02034ac58e0fa9256ddef80cbe7a3ce778f4a3c2d2e7c786f2d9f` |
| `derive_plan.py` | Sinh các trường hợp theo thứ tự từ công thức cố định | `673f39c79e90ee486e0572703e87d8ce3cb0a5ecd302b6ec4c225a4c940bc8c9` |
| `local_replay.py` | Xác nhận runtime trong cùng tiến trình | `02f1de0df244735684c16d97010ea988fad7e803b6ddbf63660babbc5e0d215f` |

Danh mục riêng còn giữ ba mô-đun unit test gốc, kiểm tra phép chiếu toán học,
kiểm tra bộ chuyển và bộ chấm, giấy phép dạng văn bản, OBJ chuẩn và biên bản
sẵn sàng mã nguồn trước nghiên cứu. Các trường `NOT_RUN` và số test trong
biên bản cũ không phải kết quả đánh giá hiện tại hay kết quả kiểm tra đóng gói.

Ảnh tổng hợp đầu vào RGB có kích thước 320×240. YuNet nhận khung BGR float32 320×256
có phần đệm, theo quy tắc lấy mẫu tâm pixel đã đóng băng. Ngưỡng điểm là 0,6,
NMS IoU là 0,3; phải có đúng một hộp khuôn mặt hữu hạn, đầy đủ, không bị cắt
bởi mép ảnh. Luồng chính giữ kích thước hộp tối thiểu bốn pixel của bộ giải mã.
Quy tắc chiều rộng 7,5% cũ chỉ còn ở các đối chứng tùy chọn tách biệt.

Khuôn mặt tổng quát dùng tọa độ chuẩn MediaPipe theo centimet, đổi cơ sở bằng
`diag(1,-1,-1)` và không dời gốc. Năm điểm tương ứng là trung bình góc mắt
33/133, trung bình 263/362, mũi 1 và góc miệng 61/291. EPNP được tinh chỉnh
bằng LM (20 vòng, epsilon 1e-8). Điểm không hữu hạn hoặc suy biến, độ sâu hay
phép quay không hợp lệ, pháp tuyến quay lưng, hoặc RMS tái chiếu vượt 5% đường
chéo hộp mặt đều cho kết quả không xác định rõ ràng.

PnP giả định góc nhìn ngang 70°, pixel vuông, tâm ảnh
`((W-1)/2,(H-1)/2)` và không méo ống kính. Giả định khớp công thức ảnh tổng
hợp theo thiết kế, chưa được đo trên đèn thật hay ảnh Pointing'04. Góc chuẩn
và góc dự đoán lấy yaw dương sang phải, pitch dương **lên trên**, tương đối
với tia từ đầu mũi giải phẫu đến máy ảnh. Pitch Euler trong hệ máy ảnh có quy
ước dấu khác và được giữ riêng. Tư thế chuẩn, điểm chiếu chuẩn và hộp mặt
chuẩn không đi vào đường dự đoán detector/PnP.

## Phân tập, chấm điểm và chuẩn bị dữ liệu

[Công thức của Reviewer](https://github.com/autonomous-ai/lampOS/blob/3a8934944acba1c17f3cb0267aae778410f5809e/lampOS/tools/sensors/vision-freeze-20261128-v1.json)
có SHA-256 `f8b57acf50c5cac4f472326fbe5a73bf0df3b3aa348235ce453e42c0c8e4423a`.
Kế hoạch có thứ tự có SHA-256
`321a78927cd87c585d1cc7a53bc7fcbc7f8dc22bfcdbe08a03fb5b3b4eb31e8f`.
Giữ đủ 5.472 dòng: 2.736 dòng train chưa có ảnh (A00–A11), 1.368 dòng tune
(A12–A17) và 1.368 dòng test (A18–A23). Mỗi tập đánh giá gồm 1.296 trường
hợp chính và 72 trường hợp chẩn đoán biên riêng. Việc huấn luyện và hiệu chuẩn
từ dữ liệu train vẫn bị tạm dừng. Bảy ảnh A00 dùng phát triển được loại khỏi
nghiên cứu.

Mỗi danh tính kết hợp ba khoảng cách mũi–máy ảnh 0,5/1/1,5 m, ánh sáng ban
ngày và ấm tối, 18 tư thế đầu, cùng yaw vai độc lập 0°/75°. Nhãn đầu không
xác nhận khả năng ước lượng vai. Miền hướng mặt tham chiếu là
`abs(yaw) <= 60`, `-20 <= pitch_up <= 25`, bao gồm các biên.

Chỉ dự đoán tune-primary được chọn ngưỡng nguyên: giới hạn yaw 1–90°, pitch
dưới −60–0°, pitch trên 0–60°. Quy tắc chọn tối đa số đúng thấp nhất theo
lớp/ô điều kiện, sau đó số đúng thấp nhất theo lớp, rồi tổng số đúng; nếu hòa,
chọn gần miền tham chiếu nhất và cuối cùng theo thứ tự từ điển. Mỗi lớp cần
ít nhất 616/648 đúng; mỗi lớp ở mọi ô khoảng cách/ánh sáng cần 103/108.
Kết quả không xác định vẫn là thất bại trong mẫu số ban đầu. Dòng biên không
tham gia chọn ngưỡng hay tỷ lệ chính. Tune không đạt vẫn là không đạt; test
sau đó chỉ để chẩn đoán, không thể sửa kết luận ấy.

Tái lập cần đúng bản kế hoạch đã lưu, manifest đã hiện thực hóa, ảnh được
phép sử dụng, mô hình đúng hash và mã nguồn không đổi. Dự đoán gốc, biên bản,
đầu vào bộ chuyển, ngưỡng đã chọn, lần chấm test và runtime phải liên kết bằng
SHA-256. Thiếu dòng, đổi thứ tự hoặc thay đầu vào đều bị từ chối. Không ghi
đè lần chạy cũ. Toàn bộ 2.790 ảnh Pointing'04 của người đã từng được đánh giá
chỉ là chẩn đoán riêng, không dùng để fit, hiệu chuẩn hoặc tuyên bố tập chưa
từng thấy; không phân phối lại ảnh.

Một lần kiểm tra đóng gói riêng đã sinh lại kế hoạch bằng Python 3.11.13.
Quá trình sinh thoát thành công, nhưng sai khác khoảng 1e-16 trong phần tử ma
trận được ghi JSON làm SHA-256 thành
`3c5cdce6c53bbb995e341fd7eb9e3daefdffd39fe6f9abe830baf45c5cc491a1`.
Cổng kiểm tra hash chính xác đã từ chối. Phải dùng đúng byte của kế hoạch đã
lưu; không nới bộ chấm hoặc thay bằng JSON gần bằng về số. Kiểm tra đó chưa
xác định phiên bản Python sinh kế hoạch ban đầu. Kế hoạch và dữ liệu đã kiểm
định trước đó không bị thay đổi.

Luồng chính chỉ cần NumPy và OpenCV, không cần ONNX Runtime. Môi trường đo
cục bộ là Python 3.11.13, OpenCV 4.10.0, NumPy 1.26.4 trên x86_64. Mã yêu
cầu một luồng OpenCV nhưng bản thư viện thực tế báo 16. Thời gian từ đọc/giải
mã tệp qua detector/PnP đến kết quả không phải độ trễ thu ảnh–công bố hoặc
kết quả đủ chuẩn trên ARM. Đối chứng mesh tùy chọn có phụ thuộc runtime riêng,
không thay đổi kết quả chính.

## Nguồn, giấy phép và giới hạn

Mô hình YuNet cố định có 232.589 byte, SHA-256
`8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4`.
[Thư mục mô hình chính thức](https://github.com/opencv/opencv_zoo/tree/48b10fa505aeb51fbd127fc60a5c660040e823b7/models/face_detection_yunet)
nêu rõ [giấy phép MIT](https://github.com/opencv/opencv_zoo/blob/48b10fa505aeb51fbd127fc60a5c660040e823b7/models/face_detection_yunet/LICENSE)
áp dụng cho mọi tệp trong thư mục. Bản giấy phép lưu có SHA
`c83b8120c50ccbd4c4f96edf53141bdd566ebb8f8e9227e415326aa1b1aba958`.
Tài liệu này không phân phối lại mô hình.

[Khuôn mặt chuẩn MediaPipe](https://github.com/google-ai-edge/mediapipe/blob/2f787dbfe2cb65152d3be409aaff8484a4e8f21b/mediapipe/modules/face_geometry/data/canonical_face_model.obj)
là tệp văn bản 45.999 byte, SHA
`8bac80443397e113f41a8b565ea72c59390bc031d9defab289dba7bc0c54e618`.
Bản [giấy phép Apache](https://github.com/google-ai-edge/mediapipe/blob/2f787dbfe2cb65152d3be409aaff8484a4e8f21b/LICENSE)
lưu có SHA `8707eef0533987efc5b155d64761eeb6e20793f50b9bd1a68dad1cf4719d0ed8`.
Bản [giấy phép OpenCV 4.10.0](https://github.com/opencv/opencv/blob/4.10.0/LICENSE)
riêng có SHA `cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30`.
Giấy phép mã nguồn không tự cấp phép cho các trọng số huấn luyện không liên quan.

[Công thức avatar A00–A23 dùng chung](https://github.com/autonomous-ai/lampOS/tree/3fb28e9e207091cf20835c7d64e8d3f9d56dcff7/lampOS/training/synthetic-avatars-20261128)
dùng MPFB 2.0.18 commit `d9648f3257a0b9712281d466245f30d656250154`, các
macro cố định và chín target khuôn mặt theo thứ tự. Mã chương trình MPFB có
giấy phép GPL-3.0-or-later; mesh, target, rig và tài sản đầu ra được chọn có
giấy phép CC0-1.0 riêng. Mục C/D của [tổng quan giấy phép](https://github.com/makehumancommunity/mpfb2/blob/d9648f3257a0b9712281d466245f30d656250154/LICENSE.md)
nêu quyền với tài sản và đầu ra tạo bằng script/render. Các điều khoản này
hỗ trợ sử dụng thương mại đầu ra từ đầu vào CC0 được chọn, không biến mã
chương trình hay mã hỗ trợ thành CC0. SHA bản tổng quan:
`5cefb60680cb9efd4550a2e65d719021cfbec9dad3658481d11d48ae863ce04d`;
SHA [văn bản CC0](https://github.com/makehumancommunity/mpfb2/blob/d9648f3257a0b9712281d466245f30d656250154/LICENSE.ASSETS.md)
`f6089cba01cb570a24712b41ab8a586ccd3cc5ef53dc266ca50b95c288956d2c`;
SHA [giấy phép chương trình](https://github.com/makehumancommunity/mpfb2/blob/d9648f3257a0b9712281d466245f30d656250154/LICENSE.CODE.md)
`26c03bc049c15fc5b9e52d791a06f1a6bc32f89edc8f8c701ab0aadf66e85b82`.

42 tệp ngoài được chọn có bản ghi CC0 riêng theo tài sản/header: 38 tệp của
[gói tài sản hệ thống](https://static.makehumancommunity.org/assets/assetpacks/makehuman_system_assets.html)
và bốn tệp của [gói kính](https://static.makehumancommunity.org/assets/assetpacks/glasses01.html).
SHA biên bản tài sản:
`bb5c47367f2b4971aaa519408c5667c71b7ae6ff8dec044b9e685b90b1e9c9ed`.
Hai mẫu kính có header AGPL mâu thuẫn đã bị loại. Không tuyên bố mọi tài sản
cộng đồng đều có cùng giấy phép. Header đã trích xuất có hash; các trang HTML
gói tài sản đã đọc không được lưu thành snapshot có hash.

Blender 4.5.17 EEVEE Next, 32 mẫu, tạo PNG không mất dữ liệu từ cảnh tự viết.
24 hình học đầu trung tính khác nhau dùng chung topology và rig nền, sáu
chất liệu da, ba nhóm tóc, một kiểu kính và một bộ trang phục. Chỉ 12 danh
tính có ảnh đánh giá. Các lựa chọn thủ tục có tương quan không phải người
thật độc lập hay bằng chứng bao phủ dân số. Đầu tĩnh và hai kiểu sáng không
tái hiện tương tác thật, ánh nhìn, quang học đèn đã đo hoặc nhãn đầu/vai chung.

## Biên bản kiểm tra đóng gói

Lần thử đóng gói xác minh 26 tệp sao chép chính xác (328.080 byte) theo danh
mục và biên dịch cú pháp các tệp Python đã sao chép. Sinh kế hoạch thoát 0,
sau đó cổng SHA chính xác thất bại như mô tả trên. Các kiểm tra unit, toán học,
bộ chấm và bộ chuyển còn lại **chưa chạy** trong lần đó. Sáu unit test gốc có
nạp mô hình được loại rõ ràng khỏi bộ kiểm tra không suy luận dự kiến. Việc
chuẩn bị tài liệu không truy cập mô hình, ảnh nghiên cứu, tác vụ huấn luyện,
tải dữ liệu mới hay thiết bị. Kiểm tra đã đánh giá trước đó và kết quả EVAL
cuối có biên bản riêng, không bị thay thế bởi kiểm tra đóng gói chưa hoàn tất.

[Kết quả đánh giá](README.md) · [Kết quả tiếng Việt](README_vi.md)
