# Câu cố định trong lịch sử của Second (#8546)

Ghi chú này tương ứng với phần
[Fixed lines in Second's history](../conversation-thread.md#fixed-lines-in-seconds-history-8546).
Một câu Skills, chẳng hạn câu hỏi trước khi tắt đèn để rút điện, có thể phát
xong ở tiến trình cha sau khi lượt gọi công cụ của Second đã kết thúc. Thread
cục bộ của tiến trình cha không chia sẻ bản chép lời với worker. Đường truyền
này chỉ gửi câu cố định đã phát xong về đúng worker Second ban đầu, dưới dạng
lịch sử trả lời tạm thời. Không chèn vào phiên Live hiện tại; giới hạn hiện có
của Live không thay đổi. Xem thêm [quy trình rút điện](unplug_vi.md).

- **Nguồn gốc và nội dung.** Khi gửi lời gọi công cụ bằng giọng nói đã được
  chấp nhận, Second ghi nhận boot của worker, phiên provider, request, call ID,
  epoch của tuyến, thế hệ quyền riêng tư mic/camera và thời điểm monotonic;
  phạm vi có hiệu lực của lời gọi được giữ cục bộ. Skills lấy thông tin này từ
  ngữ cảnh Voice đã được xác minh, không từ tham số mô hình. Voice chỉ nhận
  từ bên gọi Skills đã được xác minh, cho câu cố định chuẩn và chính xác;
  câu chuẩn, ngôn ngữ và câu đã kết xuất phải khớp. Câu chuẩn và câu kết xuất
  mỗi loại tối đa 4.096 byte; chuỗi boot tối đa 128 byte, call ID 256 byte.
  Văn bản tùy ý từ máy tính, mã ghép nối và nội dung đọc từ dịch vụ bị loại.
- **Hoàn tất và thứ tự.** Chỉ phát trọn câu tới `spoken` mới tạo biên nhận có
  chủ sở hữu lượt phát và thời điểm hoàn tất gốc. Hàng đợi riêng ở tiến trình
  cha giữ tối đa 4 biên nhận, gửi trước đầu vào provider thông thường đến sau.
  Khi được nhận, biên nhận phải còn trong 100 ms kể từ khi phát xong. Socket
  dữ liệu hoặc khóa Thread đang bận không cho đầu vào sau vượt qua biên nhận
  còn hạn; thu âm và điều khiển ưu tiên vẫn tiếp tục. Hàng đợi đầy, biên nhận
  hết hạn hoặc sai ngữ cảnh khiến biên nhận bị bỏ. 100 ms là giới hạn chấp
  nhận, không phải số đo độ trễ.
- **Phạm vi và thời hạn.** Worker giữ tối đa 16 nguồn gốc và 12 câu đã phát,
  với hạn 600 s tính từ thời điểm nguồn gốc được tạo. Nguồn gốc phải khớp bản
  đã đăng ký và câu chỉ thuộc phạm vi phòng/người ban đầu, không lấy khuôn
  mặt hay chủ phiên xuất hiện sau. Kết thúc lượt bình thường vẫn giữ quy
  thuộc lịch sử nếu không đổi; thu hẹp hoặc đổi nguồn phạm vi carried/seen
  sẽ vô hiệu hóa nó. Công tắc kể câu của phòng không đưa câu này sang lịch
  sử cá nhân. Ngừng/thay tuyến, thay đổi quyền riêng tư camera hoặc mic, kể
  cả đóng rồi mở lại, đều vô hiệu hóa bằng chứng. Provider không khả dụng hoặc
  mở phiên provider mới ngay trong cùng worker cũng vô hiệu hóa chúng; nhận
  lịch sử ban đầu cần phiên Ready khớp. Lặp lại trạng thái Ready không đổi
  vẫn giữ bằng chứng hiện tại. Lệnh dừng tường minh ở tiến
  trình cha xóa biên nhận chờ và tăng epoch của tuyến Second đang hoạt động,
  kể cả lúc rảnh; kết thúc lượt bình thường không làm vậy. Khi worker đóng
  cuộc trò chuyện, các nguồn gốc được tạo không muộn hơn mốc đóng bị thu hồi;
  nguồn gốc mới hơn vẫn được giữ nếu thông báo đóng đến trễ. Forget đang chờ
  khóa Thread đã vô hiệu hóa bản sao ngay bằng số phiên bản; xóa trạng thái
  chờ không làm bản sao sống lại. Nạp lại Thread cũng thu hồi chúng. Mỗi lần
  gọi dịch vụ chat kiểm tra lại phạm vi, tuyến/quyền riêng tư, phiên bản forget
  và hạn của quyền điều khiển đã được cài đặt còn mới. Quyền điều khiển hết
  hạn chặn lịch sử; gia hạn cùng ngữ cảnh không kéo dài hạn nguồn gốc 600 s.
  Sao chép sang vòng chat hay lần thử lại không tăng thời gian sống. Timeout
  rút điện mặc định xảy ra mười phút sau Ready, ngoài cửa sổ nguồn gốc này,
  nên câu nói sau timeout không được chuyển vào lịch sử worker.
- **Chỉ là lịch sử tạm thời.** Các câu được ghép theo thời điểm phát gốc với
  những lượt gần đây đã chọn, vẫn trong giới hạn chung 12 mục. Gửi lại cùng
  chủ sở hữu lượt phát chỉ có tính lặp an toàn khi nguồn gốc, nội dung và
  thời điểm giống hệt; lượt phát đã bị loại không được đưa vào lại. Một nguồn
  gốc có thể tạo nhiều câu được phát xong riêng biệt. Các mục này không lấy
  lời đang chờ xử lý, không mở cuộc trò chuyện, không vào tệp Thread, nhật ký
  bộ nhớ, bản tóm tắt đang chạy hay recall. Chúng không sống qua lần khởi
  động lại worker và không chuyển qua tuyến khác. Sự kiện `exact_line` và
  bản chép lời cục bộ của tiến trình cha vẫn là các phần riêng biệt.

Nếu Voice cũ từ chối metadata bằng `not a say request` trước `begun`, Skills
thử lại một lần không kèm metadata, trong hạn chót và quyền ban đầu. Các lỗi
khác hoặc phát dở không được thử lại theo cách này; khi tương thích như vậy,
không có biên nhận lịch sử mới cho worker.
