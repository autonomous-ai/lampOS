# Ngữ cảnh Memory riêng tư và khung hình nói

## Quyền theo từng yêu cầu

Ngữ cảnh riêng tư gắn với khóa của đúng yêu cầu: boot của worker, phiên và số thứ tự câu nói. Quyền cuối cùng phải khớp người được chọn, bằng chứng khuôn mặt đang nói và bản ghi nhận diện Eyes còn hạn. Việc một người khác xuất hiện sau đó không cho phép yêu cầu cũ đổi sang dữ liệu của họ. Hội thoại Room không được gắn nhãn lại dữ liệu riêng tư thành dữ liệu công khai.

Cổng bảo vệ dữ liệu riêng tư áp dụng cả khi `LAMP_SECOND_SCOPED_CONTEXT` tắt; công tắc đó quyết định cách chọn và lưu ngữ cảnh, không bỏ qua kiểm tra trước và sau khi đọc ghi chú, lịch sử hoặc dữ liệu đã lưu đệm. Với âm thanh được mang sang câu tiếp theo, cả khoảng âm thanh gốc và khoảng âm thanh mới phải độc lập chứng minh cùng một người nói.

Bản ghi Eyes v2 ghim boot, publisher, generation và người của yêu cầu. Chỉ yêu cầu đã ghim bằng chứng `seen` dương tính mới có thể tạm chờ khi nhận đúng bản ghi `checking`; trạng thái này không cấp dữ liệu riêng tư. `seen` phù hợp có thể tiếp tục công việc còn chờ trong thời hạn gốc. `absent` phù hợp chỉ duy trì yêu cầu đã ghim, không nhận một người mới và không kéo dài hạn bằng chứng 30 giây tính từ cuối âm thanh. `Lost`, quên, đóng quyền riêng tư, thay boot hoặc generation làm mất quyền của yêu cầu cũ; nhận diện lại không hồi sinh quyền đó.

Công việc chờ không giữ khóa Days/phiên; tối đa một callback đang chạy. Thăm dò 20 ms vẫn nằm trong hạn phản hồi đầu tiên và hạn bằng chứng ban đầu. Dữ liệu riêng tư được kiểm tra lại trước khi chuyển cho mô hình. Quyền ghi nhớ khuôn mặt theo lời yêu cầu dùng bằng chứng riêng, không bị nhầm với quyền tiết lộ dữ liệu riêng tư. Memory của Room giữ đúng nguồn người nói; việc sửa ghi chú của một người không được đổi nguồn của dữ liệu người khác.

## Đếm khung hình nói khi đọc lại sau lỗi

Mỗi khung hình của Eyes, xác định bằng boot và số thứ tự của bên phát, chỉ được tính một lần. Mẫu bị thiếu hoặc lỗi đọc bus tạm thời không xóa khóa chống trùng. Nếu lần đọc sau trả về cùng khung hình vẫn còn hạn, khung đó không trở thành một quan sát dương tính mới và không được dùng để tăng bằng chứng cho ngữ cảnh riêng tư hoặc việc ghi nhớ khuôn mặt theo yêu cầu.

Các mẫu thiếu vẫn chỉ được tính một lần mỗi 500 ms; khung hình có số thứ tự mới vẫn có thể được tính ngay. Ngưỡng hiện tại không đổi: hai khung hình dương tính riêng biệt, mỗi khung chỉ có đúng một khuôn mặt, có thể chứng minh một câu ngắn; đường thu hẹp ngữ cảnh cũ vẫn cần ít nhất ba quan sát. Kiểm tra quyền riêng tư, quên, thời hạn và boot vẫn áp dụng. Đây không phải bằng chứng nhận dạng hay liveness.

Xem [hợp đồng Memory bằng tiếng Anh](../memory.md). Thay đổi này chưa được xác nhận trên đèn; các cổng kiểm tra đầy đủ và lượt đọc quyền riêng tư vẫn là điều kiện trước khi chấp nhận.

Các cổng nguồn cục bộ đầy đủ trên Mac và Linux AMD64 đã đạt ở cây `665d44694`; ARM64 của PR, lượt đọc quyền riêng tư độc lập, cặp binary Voice/Eyes tương thích và lượt chạy trên đèn vẫn là những điều kiện riêng. Ghi chú CLI của Eyes cũ chỉ chứng minh cú pháp, không chứng minh khởi động hay nhận diện bình thường. Xem thêm [Eyes](../eyes.md), [đường thoại thứ hai](../second-voice.md) và [kết quả cổng cục bộ](../verification/common-checking-20261023/README.md).
