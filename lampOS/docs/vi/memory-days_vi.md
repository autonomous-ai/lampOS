# Quên một thông tin cụ thể trong bộ nhớ theo ngày

Bản tiếng Việt của mục quên có chủ đích trong
[memory-days.md](../memory-days.md).

Khi người dùng yêu cầu quên một ghi chú hoặc quan sát cụ thể, lamp xóa ghi chú
đó, các phiên bản của nó và những lượt hội thoại gốc đã tạo ra nó. Theo các từ
đặc trưng của ghi chú, lamp còn xóa:

- mọi lượt hội thoại hoặc quan sát có liên kết nguồn tới ghi chú (ở bất kỳ
  phạm vi nào) mà chính nội dung chứa ít nhất một từ đặc trưng;
- mọi lượt của chính người đó, có liên kết hay không, chứa đủ tất cả các từ
  đặc trưng (một bản sao của ghi chú).

Lượt chỉ được *xem* ghi chú mà không chứa từ nào của nó thì được giữ lại. Trên
lamp-06c1 (26/10), quên «dị ứng đậu phộng» từng xóa 75 lượt có liên kết, 73
lượt không có chữ nào về dị ứng. Nay chỉ xóa lượt gốc, câu «tránh đậu phộng vì
bạn dị ứng» và một câu «mình dị ứng đậu phộng» không liên kết, còn các câu hỏi
thời tiết được giữ.

Điều kiện của first builder (#3504): để sót một bản sao là lỗi quan trọng
nhất, nên lượt có liên kết bị xóa chỉ cần trùng một từ, chấp nhận xóa thừa vài
lượt không liên quan. Lượt không liên kết phải chứa đủ mọi từ, nếu không
«màu yêu thích là tím» sẽ xóa mọi câu có chữ «yêu thích».

Mã chuẩn hóa chữ hoa/thường và `đ`, bỏ từ quá ngắn hoặc quá thông dụng, coi
`s` số nhiều ở cuối từ tiếng Anh là tùy chọn, và ghép `allergy`, `allergies`,
`allergic` thành một từ. Ghi chú không có từ đặc trưng dùng cách xóa rộng cũ,
cộng các bản sao nguyên văn (khớp trọn từ; văn bản dưới ba ký tự, như «B2»,
chỉ khi trùng nguyên một trường, nên «B22» và «Ghế B2 còn trống» được giữ). Lượt đi qua liên kết vẫn tiếp tục
qua những lượt được giữ lại, và bản sao được so với từ của từng phiên bản
riêng, nên bản sao của «tím» mới vẫn bị xóa dù không có «xanh mòng két» cũ.
Voice chỉ yêu cầu quy tắc này bằng `selective:true` khi không bật
`LAMP_MEMORY_LINK_USED`; khi bật (liên kết nghĩa là đã dùng), lệnh quên xóa
mọi thứ có liên kết như trước. Lệnh quên không có trường này (voice cũ) cũng
như trước; memory cũ từ chối trường này và voice gửi lại một lần không kèm nó.
Lượt gốc có nhiều chủ đề vẫn bị xóa nguyên lượt.

Đây là quy tắc theo từ, không bảo đảm xóa mọi cách diễn đạt: liên kết cũ chỉ
cho biết lamp đã xem ghi chú, không cho biết lamp đã dùng nó, nên câu gián tiếp
như «tránh món đó» có thể còn lại. `forget_all` và `forget_person` vẫn xóa
toàn bộ phạm vi được yêu cầu; mọi lệnh quên vẫn vô hiệu hóa ghi chú phiên và
lượt ghi đang chờ.

## Nhật ký dừng sau một lệnh quên, rồi chạy lại

Một lệnh quên đã xác nhận dừng việc ghi nhật ký của voice. Đây là chốt chặn
riêng tư có chủ đích: lời và id của điều bị quên có thể vẫn nằm trong ngữ cảnh
của provider, nên không lượt nào ghi sau đó được liên kết tới chúng. Nhưng không
có nhật ký thì không có `conversation_end` và không có lượt quan sát, nên khi
không bật `LAMP_MEMORY_JOURNAL_RESUMES`, một lệnh quên làm lượt quan sát ngừng
cho tới khi voice khởi động lại (lamp-06c1, 27/10, N13 của Friday, #3504). Unit
sản phẩm nay bật công tắc này cạnh `LAMP_MEMORY_OBSERVE`: phiên kế tiếp được mở
mới, không tiếp nối phiên cũ, ghi nhật ký trở lại, và cuộc trò chuyện kế tiếp có
lượt quan sát của nó. Vẫn còn nợ một lần kiểm tra trên lamp.
