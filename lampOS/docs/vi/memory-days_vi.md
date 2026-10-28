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
Quy tắc chỉ đọc những dòng nó có thể xóa (#3871): các dòng có liên kết đến từ
một lượt đi đệ quy trên `sources_parent`, mỗi dòng một lần, và chỉ mục tìm
kiếm chọn ra những dòng có một từ của ghi chú (mỗi từ là một tiền tố), cùng
các dòng của người đó có đủ mọi từ của một phiên bản hoặc của một văn bản
nguyên vẹn; chỉ những dòng này được so từng từ như trên. Chỉ mục tách từ theo
cách riêng: nó đọc «İstanbul» (viết thường là «i» kèm một dấu chấm tổ hợp)
thành «istanbul», còn phần tách từ lại ra «stanbul» (#3902). Vì vậy dòng
ngoài ASCII có dấu tổ hợp không phải chữ cái, hoặc ký tự dùng riêng, cũng được
đọc và so (`search::splits_unlike_index`), và ghi chú có ký tự như vậy thì
đọc mọi dòng như trước. Một bài kiểm tra bảo đảm hai cách xóa đúng cùng những
dòng. Trên một kho 10 MB được sinh ra (7,1 nghìn dòng, nửa số từ là tiếng
Việt), một lệnh quên có mục tiêu giảm từ khoảng 11,3 s xuống khoảng 260 ms
(bản release, trên Mac; 440 ms bản debug); phần còn lại là xóa và ghi bền
vững. Trước đây nó so mọi dòng của người đó.
Lượt gốc có nhiều chủ đề vẫn bị xóa nguyên lượt.

Đây là quy tắc theo từ, không bảo đảm xóa mọi cách diễn đạt: liên kết cũ chỉ
cho biết lamp đã xem ghi chú, không cho biết lamp đã dùng nó, nên câu gián tiếp
như «tránh món đó» có thể còn lại. `forget_all` và `forget_person` vẫn xóa
toàn bộ phạm vi được yêu cầu; mọi lệnh quên vẫn vô hiệu hóa ghi chú phiên và
lượt ghi đang chờ.

Khi chưa nhận diện được mặt người nói, lệnh quên một thông tin cụ thể tìm
trong ký ức chung của phòng. Quan sát cũ có thể đã lưu lời của khách tại đó;
nếu tìm thấy, lamp xóa mục ấy và nói rõ rằng chỉ ký ức của phòng được thay đổi.
Nếu không tìm thấy, lamp báo không có mục phù hợp, không nhận là đã xóa ký ức
riêng của ai. Tuyến hội thoại thứ hai chờ tối đa 300 ms để camera nhận diện
mặt đến muộn trước khi chọn phạm vi của người đó hay của phòng. Lệnh quên
toàn bộ ký ức riêng của một người vẫn cần nhận diện được mặt.
Khi mệnh đề có lệnh quên đã nêu một thứ, chỉ thứ đó được quên: «Quên địa
chỉ của tôi, cái cũ» chỉ quên địa chỉ, không bao giờ quên những thứ «cũ».
Khi nó không nêu gì, mệnh đề đầu tiên sau dấu phẩy không chỉ có từ đệm
(«ờ», «à», «um», «you know») nêu thứ cần quên, trừ khi nó mở đầu bằng một
đại từ làm chủ ngữ: «Quên điều đó, tôi nói sai rồi» vẫn là lệnh quên không
nêu tên.
Một danh sách («Quên địa chỉ, số điện thoại và email của tôi») được để cho
mô hình: nó gọi `forget` một lần cho mỗi thứ, và `about` của mỗi lần gọi có
mọi từ đã được nghe trong phần yêu cầu quên của câu thì là thứ của lần gọi
đó. Câu được đọc theo từng mệnh đề, một mệnh đề bắt đầu sau dấu phẩy, dấu
hai chấm, gạch ngang hoặc ở một từ nối («nhưng», «trừ», «và», «but», «and»,
«except» …). Một mệnh đề là phần giữ lại khi nó mở đầu bằng một từ giữ lại
(«giữ», «trừ», «keep», «not», «except» …, sau «nhưng», «hãy», «but»,
«please» …) hoặc khi có một từ phủ định trước từ quên của nó («đừng quên»,
«don't forget»), ở bất kỳ vị trí nào; mệnh đề không có cả hai đi theo mệnh
đề trước nó. Với «Quên địa chỉ của tôi, nhưng đừng quên số điện thoại», lần
gọi quên số điện thoại bị từ chối, không xóa gì. Một từ phủ định nằm trong
chính thứ cần quên là một phần của thứ đó: «Quên rằng tôi không ăn thịt heo»
vẫn quên điều đó. Các câu cũng tách mệnh đề: một câu mà mọi từ quên đều bị
phủ định («Đừng quên số điện thoại.», «Don't forget my phone.») chỉ giữ lại,
ở bất kỳ vị trí nào, và thứ cần quên chỉ đến từ câu cuối cùng yêu cầu quên
mà không có từ phủ định: «Quên địa chỉ của tôi. Đừng quên số điện thoại.»
quên địa chỉ và giữ số điện thoại; mỗi câu yêu cầu quên không có từ phủ
định đều thêm thứ nó quên, nên với «Quên địa chỉ của tôi. Quên số điện
thoại.» mỗi lần gọi đều chạy; một câu chỉ giữ lại thì không xóa gì. Một từ
phủ định trong lời nhờ lịch sự vẫn là yêu cầu quên («Sao không quên địa chỉ
của tôi đi?», «Sao bạn không quên …?», «Why don't you forget …?»; không bao giờ «Can / Could / Would / Will you
not forget …?», vốn là lời nhờ giữ lại), còn «I»
làm chủ ngữ của từ quên («Why would I forget my phone?») không yêu cầu gì,
nên không xóa gì. Một câu đọc ra không nêu tên nhưng các
từ khác có thể nêu một thứ («Quên nó đi, ý tôi là địa chỉ») không bao giờ
xóa thứ được giữ gần nhất theo phỏng đoán: không xóa gì, và đèn hỏi lại một
lần là thứ nào. Chỉ câu không nêu tên rõ ràng («quên cái đó đi») mới xóa thứ
được giữ gần nhất.

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

## Một điều đã giữ, nói lại, vẫn là một điều

Khi bật `LAMP_MEMORY_REMEMBER_ONCE` (#2176), `remember` gửi `once` cho bộ nhớ:
một câu mà chính người đó đã giữ, còn hiệu lực và cùng chữ (không phân biệt
hoa thường, khoảng trắng và dấu câu cuối), là chính điều đã giữ đó, không ghi
thêm dòng nào. Lời sửa (`replaces`) luôn ghi bản mới; câu của người khác, hoặc
điều đã quên hay đã bị thay, không tính là cùng một điều. Unit sản phẩm bật công
tắc này từ 28/10: trên lamp-06c1, khi không có nó, một câu nói lại đã được giữ
lần thứ hai (lượt chạy của Friday, #3504). Vẫn còn nợ một lần kiểm tra trên
lamp: cùng một câu nói hai lần chỉ giữ một điều.

## Kết thúc cuộc trò chuyện sau một lệnh quên

Khi một cuộc trò chuyện còn mở mà có lệnh quên hoặc `forget_everything`,
`conversation_end` của nó có thể nêu những lượt đã bị xóa. Memory bỏ qua các id
đó và kết thúc với những lượt còn lại (#2176 hàng 13): ba lượt, quên một, thì
lượt quan sát chạy trên hai lượt. Khi mọi lượt đều đã bị quên, memory không bao
giờ từ chối (`bad_request`) nữa mà đóng cuộc trò chuyện, không có gì để quan
sát và không giữ job nào: `{ok:true,reason:"forgotten"}`, hoặc với `submit`
là `{ok:true,submitted:false,reason:"forgotten"}`; voice ghi
`memory conversation end had nothing left to observe` (N12 của Friday, #3504).
Một lệnh kết thúc không nêu nguồn nào vẫn bị từ chối như trước.
Id được đánh số riêng cho từng người, nên memory chỉ hỏi phạm vi của chính người
đó, không bao giờ hỏi những số trùng của phòng (phần tiếp theo của N12,
lamp-06c1 ngày 28/10: các id của phòng làm những lượt đã xóa của một người trông
như vẫn còn, và lệnh kết thúc bị từ chối). Một id người đó đã được cấp (nhỏ hơn
`next_id` của họ, lệnh quên không bao giờ hạ nó) mà nay không còn là đã bị quên;
một id chưa từng được cấp cho họ, kể cả id của người khác, vẫn bị từ chối
(`bad_request`).

## Truy xuất theo khoảng ngày

Khi `recall_v2` hỏi một ngày hay một khoảng ngày mà không có từ khóa, các ghi
chú đang giữ được ưu tiên chiếm chỗ trong số kết quả có hạn trước các lượt hội
thoại mới hơn, để một ngày bận không che hết các ghi chú cũ (lamp-06c1, 26/10:
một dị ứng ghi ngày 21/10 bị sáu lượt mới che mất). Kết quả vẫn trả theo thứ tự
thời gian.

Với một khoảng ngày (`day_from`), câu trả lời có thêm `more` (#3627); các câu trả lời không có khoảng ngày không có trường `more`: `true` khi còn bản ghi khớp vượt quá
`limit`, hoặc `max_bytes` đã bỏ bớt một bản ghi; trình đọc cũ bỏ qua trường này.
Voice báo câu trả lời là `partial` (chưa đủ) khi phần của người đó hoặc của căn
phòng có `more`; với memory cũ không có `more`, khi hai phần cộng lại đủ sáu kết
quả; và khi phần của căn phòng không được hỏi vì phần của người đó đã dùng hết
hạn mức. Khi đó mô hình không được nói những ngày còn lại không có gì.

## Ngày của một điều đã ghi nhớ trong ghi chú phiên

Một ghi chú hay quan sát có ngày sự kiện (`event_days`) nay mang `on`
(`YYYY-MM-DD`) trong ghi chú phiên (`session_open`) và trong câu trả lời của
`recall`, như `kept` và `search` đã có (#3987). Voice đọc ngày đó theo cách
người ta nói («Friday 30 October») và thêm một câu: đó là ngày sự việc diễn
ra, không phải ngày nó được nói, hãy tính từ ngày hôm nay và chỉ nói «hôm nay»
khi đúng là hôm nay. Trước đây một phiên mở sau khi khởi động lại chỉ thấy
«nha sĩ vào thứ Sáu» cạnh ngày mới, nên một cuộc hẹn thứ Sáu từng bị nói như
là hôm nay. Ghi chú cũ (legacy) ghi cùng ngày ở cuối dòng, «(on Friday 30
October)». Không có điều nào có ngày thì ghi chú giữ nguyên như trước. Giới hạn
1.200 byte của ghi chú không đổi và tính cả `on`; mỗi điều có ngày tốn khoảng
18–23 byte, câu giải thích khoảng 130 byte, chỉ khi có điều có ngày.

## Người hỏi chưa nhận ra được, hỏi về chính mình

Khi đèn không nhận ra người hỏi (không thấy mặt quen, không biết tên),
`recall` không trả điều gì đã ghi nhớ khi câu họ nói có một từ về chính họ
(`remember::names_the_asker`: «my», «mine», «myself», «me», hoặc «I», «I'm»,
«I'd», «I've» chỉ người hỏi nhưng không phải trong «can/could/may I ask»;
«của tôi», «tôi», «mình» hay «em» chỉ chính mình), dù câu còn nói gì khác. Một
câu không có từ như vậy cũng không được trả gì, trừ khi nó hỏi về căn phòng
hay về người khác (`remember::asks_about_room_or_other`): một từ của căn
phòng («room», «office», «here», «we», «our», «us», «team», «meeting»,
«wifi»/«wi-fi»; «phòng», «ở đây», «chúng ta», «nhóm»), một đại từ chỉ người
khác («he», «she», «they», «his», «her», «their», «him», «them»; «anh ấy»,
«chị ấy», «cô ấy», «ông ấy», «bà ấy», «họ»), hoặc một cái tên (một từ viết
hoa không đứng đầu câu, trừ «I», «Lamp», «Hey»). Câu được đọc là câu voice
giữ cho lượt nói đó; một lượt nói mà không biết câu (luồng đang bận, hoặc
không có câu nào được giữ) thì coi như hỏi về chính họ (an toàn khi không
chắc). Một lệnh gọi không có lượt nói thì không có giọng nào đang hỏi (một tin
nhắn gõ từ ứng dụng, vốn dùng trí nhớ của căn phòng, hoặc một nơi gọi trực
tiếp), nên quy tắc này không áp dụng. Khi đó memory không được hỏi. Ghi chú trả
về (`remember::UNPLACED_SELF_NOTE`, 215 byte, chỉ trong câu trả lời đó) bảo
nói rằng đèn chưa biết ai đang hỏi, xin tên hoặc cho nhìn mặt, và không bao
giờ kể điều căn phòng hay người khác đã nói như là của người hỏi. Nhật ký ghi
một dòng không có chữ nào của câu hỏi. Câu hỏi về căn phòng từ họ («what's the
meeting room called?») vẫn được trả lời từ trí nhớ của căn phòng như trước.
Ghi chú phiên khi không nhận ra ai thêm `remember::UNPLACED_ROOM_RULE` (99
byte) sau quy tắc của căn phòng: không kể những điều này như là của người hỏi,
và nếu họ hỏi về chính mình thì nói đèn chưa biết ai đang hỏi. Trước đây, ở
vòng kiểm tra bản 8327 trên lamp-4ace, một giọng chưa nhận ra hỏi «how do I
take my coffee?» và được trả lời «someone here said you take it green tea»,
một ghi chú của căn phòng từ cảnh khác (#4196). Trong ghi chú đó là lời dặn
cho mô hình; trong `recall` là một chặn cứng.
