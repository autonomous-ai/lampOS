# Các câu cố định đã phát trong lượt trò chuyện kế tiếp của Second (#8546)

Ghi chú này tương ứng với phần
[Completed fixed lines in Second's next chat](../unplug.md#completed-fixed-lines-in-seconds-next-chat-8546)
trong tài liệu tiếng Anh; không thay thế toàn bộ quy trình chuẩn bị rút điện.

Nguồn gốc được Second ghi nhận từ lời gọi công cụ bằng giọng nói đã được chấp
nhận và phạm vi bộ nhớ có hiệu lực của lời gọi đó, không lấy từ tham số của mô
hình. Skills giữ nguồn gốc theo từng lần chạy. Voice chỉ nhận thông tin này từ
bên gọi Skills đã được xác minh, cho câu cố định chuẩn có sẵn, với ngôn ngữ và
nội dung đã kết xuất khớp nhau. Chỉ khi phát trọn câu và đạt `spoken` mới tạo
biên nhận. Lượt chat kế tiếp có cùng phạm vi có thể thấy câu thực sự đã phát;
đây chỉ là ngữ cảnh lịch sử, không cấp quyền nghe, chạy công cụ, ghi bộ nhớ hay
phát lại. Thread cục bộ của tiến trình cha và lịch sử tạm thời của worker là
hai phần riêng biệt. Văn bản tùy ý từ máy tính, mã ghép nối và nội dung đọc từ
dịch vụ không đi qua đường truyền lịch sử này.

Nguồn gốc hết hạn 600 giây sau khi lời gọi công cụ được chấp nhận. Bộ đếm
thời gian mặc định chờ 10 phút sau khi gập tay đạt Ready, nên câu thông báo hết
thời gian phát sau đó nằm ngoài cửa sổ truyền lịch sử này. Câu vẫn được phát
như trước; nguồn gốc cũ không được gia hạn. Nhánh “không/ở lại” ngay sau đó và
nhánh xác nhận tắt nguồn trong 30 giây có thể nằm trong giới hạn tuổi này;
việc chuyển lịch sử vẫn cần ngữ cảnh nguồn gốc còn hợp lệ.

Lệnh dừng tường minh xóa biên nhận chờ và chặn lịch sử cũ bằng epoch mới của
tuyến Second; kết thúc lượt bình thường không làm vậy. Worker đóng cuộc trò
chuyện theo mốc thời gian tạo nguồn gốc, vẫn giữ nguồn gốc mới hơn. Thay đổi
quyền riêng tư, forget (kể cả đang chờ khóa Thread) và nạp lại Thread đều vô
hiệu hóa bản sao lịch sử. Provider không khả dụng hoặc mở phiên provider mới
ngay trong cùng worker cũng vô hiệu hóa chúng; lịch sử cần phiên Ready ban đầu
khớp. Mỗi lần gọi chat còn yêu cầu quyền điều khiển đã được cài đặt và còn mới;
gia hạn quyền này không kéo dài hạn gốc 600 giây.

Nếu bộ phân tích của Voice phiên bản cũ từ chối trường bổ sung bằng
`not a say request` trước sự kiện `begun`, Skills thử lại đúng một lần sau khi
bỏ thông tin nguồn gốc, vẫn trong hạn chót ban đầu và các quyền hiện có.
Từ chối vì lý do khác, lỗi truyền tải hoặc phát dở không kích hoạt lần thử lại
này. Cách tương thích giữ hành vi phát lời thông thường nhưng không tạo biên
nhận lịch sử mới cho worker. Không chèn câu vào phiên Live hiện tại và không
chuyển câu sang tuyến khác; giới hạn hiện tại của Live không thay đổi.

Xem [giới hạn truyền và lưu giữ](conversation-thread_vi.md): tối đa 4 biên
nhận chờ, 16 nguồn gốc và 12 câu lịch sử; biên nhận phải còn trong 100 ms kể từ
khi phát xong, nguồn gốc có hạn tối đa 600 s. Đây là giới hạn của giao thức,
không phải kết quả đo độ trễ.
