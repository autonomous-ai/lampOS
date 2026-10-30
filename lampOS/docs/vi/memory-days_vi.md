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

## Cuộc trò chuyện dài quá 64 lượt

Một cuộc trò chuyện không có quãng lặng nào mà đạt 64 lượt (`SOURCE_IDS`,
giới hạn nguồn của một lệnh kết thúc) không còn bị bỏ qua lượt quan sát nữa
(#1693, bước 2). Khi lượt thứ 65 được ghi, 64 lượt trước đó được đóng thành
một phần riêng và kết thúc như một cuộc trò chuyện kết thúc, rồi cuộc trò
chuyện đi tiếp, vẫn của người đó, trong một phần mới. Không lượt nào được nêu
trong hai phần. Mọi phần giữ số lệnh quên của lượt đầu tiên của cuộc trò
chuyện, nên một lệnh quên nói ở bất kỳ đâu trong đó vẫn đóng job quan sát của
từng phần mà không gửi; một lượt ghi bị mất chỉ làm bỏ qua phần chứa nó
(`crates/memory/tests/voice_observe_overflow.rs`: 70 lượt cho hai lượt quan
sát 64 và 6, và một lệnh quên theo tên trong phần đầu dừng cả hai). Nhật ký
có ba bộ đếm không chứa nội dung, tính từ lúc khởi động: phần bị đóng ở giới
hạn (`closed=N`), lệnh kết thúc bị memory từ chối vì vượt giới hạn đầu vào
32 KiB của lượt quan sát (`why=too_large too_large=N`), và job bị đóng không
gửi vì hàng đợi quan sát đã đầy (`busy=N`).

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

## Ghi chú cuộc trò chuyện (`talk_note`, `LAMP_CONVERSATION_SUMMARY`)

Khi bật `LAMP_CONVERSATION_SUMMARY` (chỉ tuyến thứ hai, bật trong unit sản
phẩm), lúc một cuộc trò chuyện kết thúc, ghi chú chạy của nó (bản tóm tắt
mà voice giữ trong RAM trong lúc nói chuyện) được lưu vào bộ nhớ thành
**ghi chú cuộc trò chuyện** của chính phạm vi đó, kể cả phạm vi của căn
phòng:

- **Lưu thế nào.** Lệnh mới `talk_note {text, source_ids}` trả
  `{ok:true,id}`. `text` không rỗng, tối đa 6.400 byte UTF-8, được xuống
  dòng nhưng không có ký tự điều khiển khác; `source_ids` là 1 đến 64 lượt
  trao đổi khác nhau của phạm vi này, đã xảy ra. Lưu như một observation
  loại `talk`, **không hoạt động** (`active=0`), liên kết với các lượt đó và
  mang ngày của lượt mới nhất. Vì không hoạt động, ghi chú phiên, `kept`,
  `search`, `recall` và lượt quan sát không bao giờ thấy nó, kể cả một bản
  cũ hơn khi quay lại. `adopt` không chuyển nó: khi các lượt của căn phòng
  được chuyển cho một người quen, ghi chú cuộc trò chuyện của căn phòng làm
  từ bất kỳ lượt nào trong số đó bị xoá, để căn phòng không còn nhắc lại
  được những lời đó (nó có thể chứa cả lượt khác của căn phòng nên không
  được chuyển; P2 của reviewer-lead trên #4479).
- **Giữ bao lâu.** Đến khi bị quên: không hết hạn, kể cả khi các lượt trao
  đổi của nó đã hết hạn.
- **Không kể khi không được hỏi.** Không có trong ghi chú đầu phiên. Lệnh
  `talk_notes {query?, day_from?, day_to?, limit?, max_bytes?}` trả các ghi
  chú của phạm vi, mới nhất trước: theo khoảng ngày nếu có, nếu không thì
  theo từ của câu hỏi; câu hỏi chỉ về chính cuộc trò chuyện («lúc nãy chúng
  ta đã nói chuyện gì?», `TALK_WORDS`) hoặc không có câu hỏi thì trả các ghi
  chú mới nhất (trên lamp-06c1 ngày 30/10, hỏi như vậy không tìm thấy ghi
  chú nào trong 7 ghi chú đã lưu); nhật ký đếm các ghi chú này là `entries`;
  tối đa 4 ghi chú và 12.800 byte, ghi chú quá dài bị
  bỏ các dòng `- ` cũ nhất, không bao giờ cắt phần cuối. Khi `recall`, voice
  hỏi thêm ghi chú của người hỏi (đánh dấu là của họ, «bản tóm tắt của chính
  bạn, bằng lời của bạn, không phải lời họ») rồi của căn phòng (kể như của
  căn phòng, không bao giờ như của người hỏi). Người hỏi chưa nhận ra được,
  hỏi về chính mình, vẫn không được trả gì (#4196).
- **Quên.** «Quên mọi thứ về tôi» xoá ghi chú của người đó. Bất kỳ lệnh quên
  nào xoá một lượt trao đổi của nó cũng xoá nó (kể cả quên một điều đã ghi
  nhớ, vì bằng chứng của điều đó bị xoá); quên chính ghi chú thì xoá cả các
  lượt trao đổi của cuộc trò chuyện. Quên một điều đã ghi nhớ hay một
  observation cùng phạm vi, có chung một lượt nguồn với ghi chú, cũng xoá
  ghi chú, kể cả khi các lượt đó đã hết hạn sau 30 ngày: id các lượt nguồn
  của mỗi điều đã ghi nhớ và observation được chép vào bảng `origins` trước
  khi lượt nào hết hạn (`keep_origins`, trong `maintain`), tạo `IF NOT
  EXISTS`, `user_version` không đổi (P1 của reviewer-three trên #4479).
- **Lượt quan sát** được đọc ghi chú như `"summary"`, là lời của đèn, chỉ
  được dẫn các bản ghi; voice bỏ một đề xuất không có từ nào trong lời người
  đó nói ở các bản ghi được dẫn mà lại có trong ghi chú.

## Gọi `recall` trước khi nói không biết

Mô tả `recall` của voice bảo mô hình gọi nó trước khi nói không nhớ một sở
thích của chính người đó, như đồ uống yêu thích, trừ khi câu trả lời đã có
trong cuộc trò chuyện (`remember::RECALL_OWN_PREFERENCE`; lamp-06c1, 26/10:
P09 đáp «tôi không biết» mà không gọi, P40 gọi và nói cà phê đen). Từ #4339
(lần chạy dài thứ ba trên lamp-06c1, 30/10: 3 lần `recall` trong 96 lượt, bốn
câu «tôi không có gì» về đời sống của chính người đó trong khi điều ấy đã được
giữ, quan sát hay nằm trong một ghi chú cuộc trò chuyện), cùng câu đó phủ cả
đời sống của họ: kế hoạch, những người trong đời họ, công việc và những gì đã
nói với nhau. `recall` nay «không dành cho những gì bạn còn thấy của cuộc
trò chuyện này» (`remember::RECALL_NOT_FOR`); trước đây là «không dành cho
cuộc trò chuyện này», khiến mô hình không hỏi phần đầu của một cuộc trò
chuyện dài, nay đã nằm trong ghi chú cuộc trò chuyện. Lượt nói cũng nói điều
đó, sau các lệnh của bộ nhớ (`worker::RECALL_BEFORE_NOT_KNOWING`): trước khi
nói không biết hay không nhớ điều gì về đời sống của họ, gọi `recall` trước,
chỉ nói «tôi không nhớ» khi nó không tìm thấy gì, và chỉ dùng những gì tìm
được để trả lời điều họ hỏi (không bao giờ kể ra khi không được hỏi). Một câu
đùa, một sự thật, giờ giấc hay chuyện phiếm không hỏi gì trước, nên một lượt
bình thường không có vòng `recall` trước âm thanh đầu tiên. Đó là lời cho mô
hình, không phải một bảo đảm; memory vẫn chỉ tìm trong phạm vi của người hỏi.
Ghi chú cuộc trò chuyện vẫn chỉ đến qua `recall`: không ghi chú nào được hiện
khi một phiên bắt đầu. `probe-tools ... --recall` đo quy tắc này so với lời
trước nó trên mô hình thật ([second-voice.md](../second-voice.md)).

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

## Một câu của họ không thấy mặt nói, trong cuộc trò chuyện đang diễn ra

Với `LAMP_PERSON_SCOPE_SEEN`, một câu của người quen mà không thấy mặt ai nói
thì thuộc về căn phòng, nên một lượt trong cuộc trò chuyện của họ mất cả cuộc
trò chuyện (lamp-3e83, 29/10: cuộc chat thấy 16, rồi 4, rồi 16 tin nhắn, và đèn
hỏi lại câu đã hỏi). Với `LAMP_CONVERSATION_CARRIES=on` (#4339), câu ấy vẫn
thuộc cuộc trò chuyện đó khi lượt trao đổi mới nhất trước nó là của họ và bắt
đầu trong vòng 45 giây, và mắt có nhìn qua câu ấy mà không thấy mặt nào khác
ngoài mặt họ. Nó được đọc như một phạm vi được giữ qua khoảng hở của khuôn mặt
(`ScopeSnapshot::held`): luồng và các lượt trao đổi của họ như đang có, không
có ghi chú ký ức, gợi ý, `recall`, ghi nhớ, quên hay một lời «có», và không bao
giờ được ghi vào nhật ký. Vì vậy quy tắc ở trên vẫn giữ: người hỏi chưa nhận ra
không chạm được ký ức đã lưu của ai. Thấy mặt ai nói câu ấy, có mặt khác trong
khung hình, im lặng quá 45 giây, có lượt của người khác ở giữa, hoặc mắt không
nhìn qua câu ấy, thì nó thuộc về căn phòng như trước. Một câu được nhận là của
căn phòng vì không nhận ra ai (lamp-06c1, 29/10: 5 trong 68 lượt dù người đó ở
trong khung hình suốt) cũng được giữ trong cuộc trò chuyện đang diễn ra của
người quen theo cùng quy tắc. Rủi ro đã chấp nhận: một
người ngoài khung hình, không có dấu hiệu nào khác, được trả lời trong cuộc trò
chuyện đang diễn ra và nghe được những gì đã nói trong đó; họ không chạm được
ký ức đã lưu của người kia.

**Một khuôn mặt im lặng không tính** (`LAMP_CONVERSATION_CARRIES_PAST_SILENT=on`,
#4339, quyết định của người phụ trách trí nhớ ngày 30/10): trên lamp-3e83 ngày
30/10 (bản 8899), khuôn mặt quen ở trong khung hình ở các câu bị rơi nhưng hầu
như không bao giờ một mình (30 lần nhìn, không lần nào chỉ một mặt, mặt quen có
trong cả 30), nên quy tắc trên chỉ giữ được 4 câu và một cuộc trò chuyện bị chia
từng câu vào luồng của người đó hoặc của căn phòng. Bật công tắc này, «không có
mặt nào khác trong khung hình» trở thành **không thấy mặt nào khác nói câu ấy,
và không có giọng nào khác trong cuộc trò chuyện**: mắt vẫn phải nhìn qua câu
ấy (`SpeakingFaces::looked_between`), và giọng khác là dấu hiệu người thứ hai
của chính quy tắc nghe, dấu hiệu mà việc nối lời bị cắt giữ
(`Heeding::another_person`: một câu bằng giọng khác giọng cuộc trò chuyện, một
dấu giọng dưới `same_voice::DIFFERENT_BELOW`, hoặc hai mặt được thấy nói một
câu). Nó được đọc khi nghe câu và một lần nữa sau khi quy tắc đã xét giọng của
chính câu ấy; nếu đã có giọng khác, câu thuộc về căn phòng trước khi kể gì. Một câu
mà quy tắc bỏ qua (bị giữ lại vì là tiếng ngắn, rỗng, hoặc là tiếng vọng của
chính đèn) không được xét giọng, nên dấu hiệu là của câu trước: câu ấy không
được giữ, thuộc về căn phòng trước khi kể hay lưu gì. Chỉ có tác dụng khi quy
tắc giữ dấu hiệu đó (`LAMP_JOIN_CUT_WORDS` hoặc `LAMP_SECOND_NAME_CARRIES`, cả
hai đều bật trong sản phẩm) và việc kiểm tra cùng giọng đang chạy (bộ lấy dấu
giọng đã khởi động, đèn là giọng duy nhất, và một trong `LAMP_SPOKEN_TO`,
`LAMP_QUIET_GOING_ON_SAME_ONE`, `LAMP_GOING_ON_NOT_OWN`,
`LAMP_MEMORY_FOLLOWS_FACE` bật): thiếu một trong hai, không gì có thể cho thấy
giọng khác, nên công tắc không làm gì, mặt khác trong khung hình vẫn giữ câu
cho căn phòng như trước, và nhật ký ghi một lần lúc khởi động là thiếu gì
(`Engine::carrying_past_silent_where_heard`). Phần còn lại giữ nguyên. Rủi ro đã chấp nhận: **một vị khách im lặng ngồi bên nghe được luồng
của người đó ở một câu được giữ** (không bao giờ là ký ức đã lưu của họ).

**Các câu của căn phòng trong cùng cuộc trò chuyện**
(`LAMP_CONVERSATION_ROOM_LINES_TOLD=on`, #4339): một câu vẫn bị tách sang luồng
của căn phòng thì lượt sau của người đó không biết (lamp-3e83, 30/10: một tìm
kiếm trên mạng được trả lời trong luồng của căn phòng, rồi «I've lost the
thread» cho chính câu hỏi của họ về nó). Bật công tắc này, lượt của một người
quen được đưa cả các lượt trao đổi của căn phòng trong cùng cuộc trò chuyện,
câu của người nói được đánh dấu `(said in the room)`
(`Thread::recent_with_room_lines`): cùng cuộc trò chuyện là đi ngược từ bây giờ
qua các lượt của họ và của căn phòng, mỗi lượt cách lượt sau không quá 45 giây,
dừng ở lượt của người khác, và chỉ tính các lượt của căn phòng từ lượt đầu tiên
của họ trong đó; trong cửa sổ 12 lượt và 600 giây của cuộc chat. Chỉ một chiều:
lượt của căn phòng hay của một giọng chưa nhận ra (#4196/#4207) không bao giờ
được đưa câu của một người, và lượt của một người không bao giờ được đưa câu của
người khác. Bản tóm tắt chạy và ghi chú cuộc trò chuyện vẫn theo từng phạm vi.
Mọi lệnh quên đều bỏ chúng. Tất cả trong bộ nhớ, không thêm I/O trên đường trả
lời. Thuộc rủi ro «đã nói thành tiếng» được chấp nhận: một câu bị thu về căn
phòng vì một giọng khác đã nói nó (`Heeding::another_person`, hoặc lần xét lại
sau quy tắc) vẫn đến lượt của người đó, đánh dấu `(said in the room)`: lời của
một vị khách, nói thành tiếng bên cạnh họ, được kể cho lượt của họ; không bao
giờ vào bản tóm tắt chạy, ghi chú hay ký ức đã lưu của họ.

**Điều một câu được giữ yêu cầu lưu phải chờ lời «có» của họ**
(`LAMP_CONVERSATION_CARRIED_SAVES=on`, #4339, thay đổi quy tắc riêng tư được
duyệt ngày 30/10, cùng quyết định của người xây dựng đầu tiên trên #4508):
trước đây câu được giữ không lưu được gì, nên «We booked the Pine Hill Hotel»
nói lúc khuôn mặt lệch khỏi khung hình bị mất («I can't save that just now»).
Bật công tắc này, lệnh `remember` của một câu được giữ
(`ScopeLease::save_carried`) được khai báo nhưng không ghi ngay: nó được giữ
trong RAM, không ghi vào nhật ký, chờ lời «có» của chính người được giữ, như
một lệnh quên được giữ (`hold_carried_keep`, `take_held_forget`). Mô hình được
báo «Not kept yet» (`remember::held_keep_told`) và hỏi một lần, «Want me to
keep that for you?»; việc giữ chỉ còn nếu câu trả lời nói trọn có hỏi
(`remember::asks_about_keeping`). Lời «có» đơn giản của họ ở câu nói kế tiếp,
trong vòng `HELD_FORGET_FOR` (60 giây), với chính khuôn mặt họ được thấy đang
nói và không mặt nào khác (`LAMP_EYES_SPEAKING`,
`speaking_faces::seen_saying`), mới ghi nó vào phạm vi của họ, và câu trả lời
nói đã lưu. Khuôn mặt họ chỉ ở trong khung hình là chưa đủ: nó vẫn ở đó khi một
vị khách nói từ ngoài khung hình (Reviewer-three, 14:46). Mọi trường hợp khác
đều bỏ nó, không ghi gì ở đâu cả: không có «có», «có» không được thấy (mô hình
được báo là không tính), một mặt khác được thấy nói, một lệnh quên, cuộc trò
chuyện kết thúc, hay quá một phút. Khi mắt không đọc được ai đang nói thì không
giữ gì, và việc lưu bị từ chối («Not kept»). Lượt trao đổi của câu được giữ vẫn
không vào nhật ký, nên không lời nào trong đó thành điều quan sát về họ. Mọi thứ
khác vẫn giữ như phạm vi được giữ: không `recall`, ghi chú hay gợi ý, không
`forget`, `forget_everything`, quy tắc, khuôn mặt hay lời «có». Một câu được
nhận chỉ nhờ khoảng hở của khuôn mặt (`LAMP_FACE_GAP_HOLD`) không giữ gì, và câu
đã thu hẹp về căn phòng (thấy mặt khác nói) là của căn phòng. Bản tóm tắt chạy
và ghi chú cuộc trò chuyện vẫn bỏ qua câu được giữ. Rủi ro còn lại: lời «có»
được thấy của chính họ xác nhận một điều mà người khác có thể đã nói; họ được
hỏi trước. Việc kiểm tra cùng giọng (`same_voice_releases`, một điểm móc, hiện
chưa bao giờ đúng) là phần làm sau.
