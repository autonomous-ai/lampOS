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

Từ #4753, bộ nhớ còn ghi lại lúc điều đó được nói lại (bảng `said_again`, tạo
`IF NOT EXISTS`, `user_version` không đổi), khi lúc ấy muộn hơn lúc giữ và lần nói lại gần nhất. `kept`,
`recall` và `search` khi đó trả thêm `last_said_unix_ms`, lần nói gần nhất,
bên cạnh `at_unix_ms` vẫn là lúc giữ; voice nói «khi nào» theo lần muộn hơn.
Trước đây, «lần cuối tôi nói với bạn về cà phê là khi nào?» chỉ nhận được ngày
giữ đầu tiên, dù hôm qua vừa nói lại. Thời điểm được giữ tạm trong bộ nhớ của
store (`said_pending`, tối đa 64) và chỉ được ghi khi chính dòng của lượt đó
được ghi (`append_linked`, sau lượt, có liên kết tới điều đã giữ), với dòng đó
là `origin` duy nhất; không bao giờ có hàng `origin` 0, và `source_ids` của
`remember` (các dòng ghi trước nó) không bao giờ là origin. Quên dòng có câu
nói lại («quên điều tôi vừa nói», `forget_last`, kể cả ở lượt đầu của cuộc trò
chuyện), hoặc quên điều đã giữ hay lượt quan sát mà dòng đó liên kết tới, thì
mất luôn thời điểm đó; quên một dòng trước đó của cuộc trò chuyện thì không.
Lượt ấy hết hạn sau 30 ngày thì thời điểm vẫn còn. Kho
tạo trước bảng này mở ra vẫn chạy, và mọi điều đã giữ của nó được coi là nói
lần cuối lúc được giữ.

Một điều đã giữ có ngày sự kiện, nói lại với một ngày khác, nhận ngày mới dù
sớm hơn hay muộn hơn («Lily đến thứ Bảy», «giờ là Chủ nhật», «lại thứ Bảy»), và
được hỏi thăm lại một lần sau ngày đó; nói lại mà không kèm ngày thì giữ ngày
cũ. Trước đây ngày chỉ đổi khi muộn hơn.

## Ngày được nói, chỉ khi được hỏi «khi nào» (#4753)

Mỗi kết quả của `recall` (và mỗi dòng «said …» của gợi ý trong lượt) nay mang
cả khoảng thời gian lẫn chính ngày đó, theo múi giờ của lamp
(`remember::when_said`): «3 days ago, Saturday 31 October», quá sáu ngày cũng
vậy («2 weeks ago, Tuesday 20 October»), thêm năm khi là năm khác; không bao
giờ có giờ. Mô tả của `recall` (`remember::RECALL_WHEN`) dặn mô hình trả lời
không kèm giờ và chỉ nói ngày khi người ta hỏi «khi nào»; câu trả lời thường
vẫn nói đại khái («ba hôm trước»). Trước đây mô tả bảo «không ngày, không giờ»
và chỉ đưa «last week», nên câu hỏi «khi nào» không có ngày (bench #4753:
0,10–0,25). Ghi chú tóm tắt cuộc trò chuyện và lượt quan sát nay được biết
ngày hôm nay và được dặn viết ngày đầy đủ («Friday 6 November»), không viết
thứ trơn. `probe-tools ... --when` đo cách nói mới so với cũ trên mô hình thật.

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

## Điều được giữ là điều đã nói (`LAMP_MEMORY_KEEP_HEARD`)

`LAMP_MEMORY_KEEP_HEARD=on` (#2176; tắt nếu không đặt, cả hai đường voice)
bật trong bản sản phẩm từ 5/11, theo quyết định của người phụ trách trí nhớ.
Khi câu `remember` của mô hình có một từ khóa mà câu nghe được không có, lamp
giữ chính câu nghe được, dạng "They said: …" (tối đa 300 byte). Một câu chỉ là
"có", "không" hay "okay" không bao giờ được giữ như vậy; một lời "có" sau lời
đề nghị giữ lần hai giữ câu mà lời đề nghị đầu nói đến (#5276, N6). Chi tiết:
[memory.md](../memory.md), "What is kept is what was said".

## Truy xuất theo khoảng ngày

`LAMP_MEMORY_RECALL_V2=on` (#2176; tắt nếu không đặt, chỉ với client Days, cả
hai đường voice) bật trong bản sản phẩm từ 4/11, sau lần kiểm tra trên
lamp-06c1 (build 10012): chỉ "my" thì không tìm thấy gì, "last week" có câu
trả lời theo khoảng ngày. Cùng ngày, `LAMP_MEMORY_REPLACES` (#2510, lời sửa
thay thế điều đang giữ) và `LAMP_MEMORY_FORGET_ABOUT` (#2176, quên những gì đã
nói về một thứ) cũng bật trong bản sản phẩm sau lần kiểm tra của chúng.

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

## Lời sửa mới hơn xếp trước (`LAMP_MEMORY_NEWER_FIRST`)

Khi bật `LAMP_MEMORY_NEWER_FIRST=on` (kế hoạch đưa ý tưởng của Tiers vào Days
của người phụ trách trí nhớ, 3/11, phần P1; tắt nếu không đặt; ý tưởng phỏng
theo cách memory-tiers thay thế bằng "bằng chứng mới hơn của chính chủ",
[memory-tiers.md](../memory-tiers.md)), `search` hay `recall` của một lần
truy xuất sẽ có thêm `"newer_first":true` (P4, voice gửi trường này; một
`recall` không có truy vấn thì không, dù kho cũng trả lời nó ở đó).
Trong các bản ghi truy vấn đã tìm được cho một người đã nhận ra, một câu mới
hơn sửa một câu cũ hơn được xếp trước trong `search`, và câu cũ có
`"earlier":true`; `recall` vẫn giữ thứ tự thời gian và chỉ thêm `earlier`.
Với phạm vi của căn phòng (không có người), trường này bị bỏ qua và câu trả
lời như khi không có nó: câu của căn phòng là của nhiều người nói, và câu của
người này không bao giờ được đánh dấu câu của người khác là `earlier`
(`days_recall::newer_first::room_scope_is_never_reordered`). Một câu sửa câu
khác khi nó mới hơn, có một cụm báo hiệu sửa nguyên từ (`actually`, `no
longer`, `not … anymore`, `changed to`, `moved to`, `now it's`/`now it is`,
`instead`, `thật ra`, `không còn`, `đổi sang`, `chuyển sang`, `bây giờ là`,
`thay vì`; chỉ tiếng Anh và tiếng Việt; `now` hay `giờ` đứng một mình, cũng
là "giờ" đồng hồ, không phải báo hiệu) và nói về cùng một điều: có chung ít
nhất hai từ đặc trưng của câu cũ (không tính từ báo hiệu) và chung tất cả
trừ nhiều nhất một, giá trị đã đổi. Vì vậy "now I'm brewing jasmine tea for
mom" không sửa "I drink jasmine tea", "7 giờ tôi uống trà nhài với mẹ" cũng
không sửa "Tôi uống trà nhài mỗi sáng"
(`search::newer_first_tests::bare_now_or_gio_marks_nothing`). Chỉ tính lời
của chính người đó, không bao giờ tính lời đáp của đèn. Không bản ghi nào bị
bỏ, vô hiệu hay thêm vào: vẫn các bản ghi đó, vẫn `more` đó, chỉ đổi thứ tự
và cờ này. Khi tắt, trường không được gửi và mọi câu trả lời giống hệt từng
byte như trước (`off_is_byte_identical`). Cần lamp-memory cùng bản build:
memory cũ từ chối trường này.

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

## Dải sở thích trong ghi chú phiên (`profile`, `LAMP_MEMORY_PROFILE`)

Với `profile:true` trong `session_open` (công tắc `LAMP_MEMORY_PROFILE` của
voice, mặc định tắt; kế hoạch ý tưởng Tiers của người phụ trách bộ nhớ, 3/11,
ý tưởng hồ sơ chủ nhân của [memory-tiers](../memory-tiers.md) (Tiers), đã điều
chỉnh), ghi chú được điền hai lượt. Trước hết là sở thích và các điều đã giữ,
mới nhất trước, trong tối đa 600 byte và 30 điều (`PROFILE_BYTES`,
`PROFILE_FACTS`). Sau đó là lời hứa, chuyện đang dở và phần còn lại, như khi
không có nó, trong phần còn lại của cùng giới hạn 1.200 byte. Trước đây một
người có nhiều chuyện đang dở bị đẩy hết sở thích và thói quen ra khỏi ghi
chú. Cùng ứng viên, cùng người, cùng giới hạn, các điều được viết theo cùng
thứ tự: không giữ, nói hay đọc thêm gì, không gọi mô hình, không di chuyển dữ
liệu. Không có trường này, ghi chú giống hệt trước, từng byte. Memory cũ từ
chối trường này (`deny_unknown_fields`).

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
- **Lượt quan sát chỉ giữ một cái tên khi chính người đó nói ra**
  (lamp-06c1, 4/11: mặt chủ ở trước đèn, im lặng, một đồng nghiệp nói
  «... Connie Long ...», và đèn giữ «Their name is Connie Long.» vào phạm vi
  của chủ). Luôn bật, chỉ siết lại, không công tắc: một đề xuất nêu tên của
  chính người đó («Their name is X.», «Name is X.», «They go by X.», «Tên
  của họ là X.»; `remember::named::name_in`) chỉ được giữ khi một lượt nó
  dẫn được thấy chính người đó nói (một khuôn mặt, của họ, như người nói của
  một dòng phòng cần, `remember::seen_by`) và trong lượt đó họ tự giới thiệu
  bằng tên ấy («My name is X», «I'm X», «Call me X», «Tên tôi là X», không
  phải câu hỏi), đánh vần nó, hoặc đèn đọc lại và câu kế tiếp của họ, cũng
  được thấy họ nói, là một tiếng «có» rõ ràng (đường tên đã kiểm, #1057).
  Lượt mà câu trả lời của đèn nghi mình nghe nhầm (#3504 N5), hay tên là
  tên của đèn hoặc từ bị viết thay cho nó (#3504 N4), không cho tên nào.
  Câu về tên người khác («Their friend's name is Connie.», «Talked to Connie
  Long today.») được xét như trước. Nhật ký chỉ đếm, không có lời nào. Test:
  `remember::named::tests`.

## Ý tưởng của Tiers trong Days, tắt cho bản ứng viên

`LAMP_MEMORY_NEWER_FIRST`, `LAMP_MEMORY_STRENGTH` và `LAMP_MEMORY_PROFILE`
(kế hoạch ý tưởng Tiers của người phụ trách trí nhớ, 3/11; mỗi công tắc chỉ
nhận `on`, chỉ với Days) được bật trong unit voice của bản sản phẩm từ 5/11
theo quyết định của người phụ trách trí nhớ, và tắt cho bản ứng viên từ 6/11
cho đến khi một bài kiểm tra phát thử đạt (#5331, #5464). Khi bật, một lời
sửa nói trong lúc trò chuyện
("thật ra bây giờ là trà xanh") được trả về trước câu cũ mà nó sửa, và câu cũ
mang `earlier`; một điều nói như "có lẽ" hay một dự định được đánh dấu
`maybe` hay `plan`, nên đèn không nói như thể nó đã xong hay đã đặt; và ghi chú
phiên dành 600 byte đầu cho sở thích và các điều đã giữ của người đó. Cần
lamp-memory cùng bản build, điều mà bản cài sản phẩm luôn mang theo
([memory-days.md](../memory-days.md)).

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

## Lời «không biết» tìm trước (`LAMP_MEMORY_DONT_KNOW_RECALLS`)

`LAMP_MEMORY_DONT_KNOW_RECALLS=on` (nhiệm vụ C57; tắt nếu không đặt, đường
voice thứ hai với Days) bật trong bản sản phẩm từ 5/11, theo quyết định của
người phụ trách trí nhớ, sau khi sáng hôm đó lamp-06c1 nói «tôi không nhớ kế
hoạch kỳ nghỉ nào» mà không gọi `recall` (C56). Khi câu đầu của câu trả lời
có một lời không biết ở bất kỳ chỗ nào («Cá vàng của bạn — mình không nhớ.»,
«mình không có…», «bạn chưa nói…», «chưa có gì về…») về điều của chính người
hỏi (không bao giờ với một câu được mang vào cuộc trò chuyện đang diễn ra,
`LAMP_CONVERSATION_CARRIES`, vì một vị khách ngoài khung hình trông cũng như
vậy; một số cụm như «I can't tell you», «nothing about» chỉ tính khi có từ phù
hợp theo sau), những điều đã giữ của người đó
được tìm ngay (tối đa 60 ms); có tìm thấy thì lời không biết không được nói và
mô hình được hỏi lại một lần với chúng. Chỉ người của chính yêu cầu, không bao
giờ của căn phòng hay của người khác. Chi tiết: [memory.md](../memory.md),
"A don't-know recalls first".

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

Với `LAMP_CARRIED_LINES_KEEP_TALK=on` (bật trong bản sản phẩm), một câu được
giữ trong cuộc trò chuyện như vậy, dù không bao giờ được ghi vào nhật ký hay lưu
là của họ, vẫn giữ cuộc trò chuyện trong bộ nhớ của người đó tiếp diễn: khoảng
im lặng 45 giây của bộ ghi được tính từ câu ấy (`DaysJob::Touch`). Trước đây,
khoảng 97 giây các câu như vậy làm cuộc trò chuyện của người đó hết hạn giữa
chừng và phần trước bị bỏ (lamp-06c1, 5/11). Câu của người khác, hoặc sau
khoảng im lặng, không thay đổi gì.

Một nguồn bị thiếu hoặc đã xóa làm dòng bị từ chối, và dòng đó không bao giờ
được gửi lại dưới dạng không liên kết. Khi một điều bị quên từ bên ngoài voice
(ứng dụng, `forget_person` của mắt, việc dọn dẹp theo thời hạn, bước dọn của bài
thử), ngữ cảnh vẫn giữ id đã mất, nên trước đây mọi dòng sau dẫn tới nó đều bị từ
chối (lamp-06c1, 5/11). Nay lời từ chối đúng nguyên văn «a source ID does not
exist for this person», cho một nguồn hay một quy tắc (`remember::source_gone`),
được xử lý như một lần quên đã xác nhận: dừng ghi nhật ký
(`why=source_gone`), cuộc trò chuyện đến lúc đó kết thúc trọn vẹn và vẫn được
quan sát, các dòng tạo trước lúc dừng không được ghi sau đó, và dòng bị từ chối
không được giữ.

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
(`remember::asks_about_keeping`: «Should I keep your lucky number as
fifty-three?» và «Want me to keep Thursday book club for you?» đều tính; trên
lamp-06c1 ngày 5/11, S14, cả hai bị bỏ sót và việc giữ bị bỏ). Lời «có» đơn giản của họ ở câu nói kế tiếp,
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
## Một câu của họ không thấy mặt nói, khi chỉ có mặt họ trong khung hình

**Chỉ mặt của người ngồi tại bàn mới là mặt khác** (`LAMP_FACES_AT_DESK_ONLY=on`,
bật trong bản sản phẩm; [eyes.md](../eyes.md), "Who counts as someone at the
lamp"): với «một mặt trong khung hình», «không mặt nào khác» và «một mình trong
khung hình» bên dưới, một mặt cao chưa tới 0,6 chiều cao của mặt tham chiếu trong
cùng lần nhìn (mặt cao nhất có độ chắc ít nhất 0,70 và cao không quá 0,6 khung
hình, nên một khung giả khổng lồ không loại được chủ đèn), hoặc độ chắc chắn dưới
0,70, không được tính là người ở bên đèn. Một lần nhìn có mặt không bao giờ đếm
là không có ai.
Trên lamp-a0ae ngày 2/11, ngay sau cài đặt, tên chủ đèn bị giữ lại ở 51 trên 79
lượt vì «nhiều hơn một mặt trong khung hình»: bên cạnh mặt chủ đèn (rộng 0,09
đến 0,19 khung hình, chắc 0,80 đến 0,92), mắt liên tục báo những mặt rộng 0,02
đến 0,06 với độ chắc 0,61 đến 0,78, gần như mỗi lần nhìn một mã mới. Mặt như vậy
vẫn được theo dõi và vẫn được đọc là đang nói; danh sách của mắt không bị đổi.
Một mặt không được tính mà đọc là đang nói trong 10 giây qua vẫn giữ tên chủ đèn
lại («a face not at the desk seen speaking»): một vị khách đứng xa phía sau, là
giọng duy nhất, không được nghe tên chủ đèn. Mặt có độ chắc dưới 0,70 bị bỏ dù
lớn cỡ nào, nên một vị khách tại bàn quay nghiêng (khoảng 0,65) được tính 0: nếu
họ nói, tên vẫn bị giữ lại; nếu họ im lặng thì không.
Dòng nhật ký quyết định tên mặc định ghi kích thước và độ chắc của các mặt được
tính và bị bỏ, không mã, không vị trí, chỉ khi bật công tắc hoặc có mặt bị bỏ. Tắt công tắc thì mọi mặt đều được tính,
như trước. Các ngưỡng 0,6 và 0,70 lấy từ bản ghi trên lamp-06c1 (3/11); một người thật đứng
sau chủ đèn 1 m vẫn chưa được thử.

Với `LAMP_PERSON_SCOPE_ONE_FACE=on` (bật trong bản sản phẩm; mục 1 của nhóm
thử nghiệm ra mắt trên #3997): trên lamp-220c ngày 30/10, mặt chủ đèn có trong
17 lần nhìn của một câu nhưng không lần nào đọc là đang nói, nên với
`LAMP_PERSON_SCOPE_SEEN` mọi câu đều thuộc về căn phòng và ký ức riêng của họ
gần như trống. Giờ đây, với một câu được nhận là của người quen, không thấy mặt
ai nói, khi các lần nhìn qua câu ấy thấy khuôn mặt quen của họ trong ít nhất 3
lần và không thấy mặt nào khác trong bất kỳ lần nào
(`SpeakingFaces::one_face_in_view`; một lần nhìn không có ai không tính là mặt
khác), và cuộc trò chuyện **chưa có giọng nào khác**, thì **việc ghi nhớ**
(`remember`) **và việc quên điều đã ghi** là của họ: điều đó được lưu vào phạm
vi của họ (`ScopeLease::take_by_one_face`, `ScopeLease::kept_by_one_face`), và
một `forget` một mục tìm trong phạm vi của họ rồi đến của căn phòng
(`ScopeLease::reached_by_one_face`, quyết định duy nhất mà tuyến ký ức trong
`tools.rs` đọc cho cả hai, nên việc quên không bao giờ hẹp hơn việc ghi). **Mọi việc
đọc vẫn là của căn phòng**: không `recall` hay gợi ý từ ký ức của họ, không
ghi chú ký ức của họ, không ngữ cảnh của người ấy, không chuỗi hội thoại hay
bản tóm tắt của họ; câu được nói và đọc như của căn phòng, như khi không có
công tắc. Lý do (P1 của Reviewer-three trên #4584, phương án 2): «không có
giọng khác» được so với giọng của cuộc trò chuyện, không phải giọng của họ,
nên một vị khách ngoài khung hình mở đầu cuộc trò chuyện trong khi họ ngồi im
là mặt duy nhất trông y hệt; một lần ghi nhầm nằm trong ký ức của chính họ, nơi
họ có thể quên nó, còn một lần đọc nhầm đưa ký ức của họ cho người khác. Việc
đọc ký ức của họ cần một dấu hiệu nói (thấy mặt họ nói) hoặc một cuộc trò
chuyện được giữ (#4479); khi đó cách ấy thắng, như hôm nay. Bằng chứng «giọng
khác» là ghi nhận của chính quy tắc trên tuyến thứ hai
(`worker::Heeding::another_person`): một câu có dấu giọng khác với giọng của
cuộc trò chuyện (`same_voice::DIFFERENT_BELOW`) hoặc thấy hai mặt cùng nói một
câu, kể từ khi cuộc trò chuyện bắt đầu. Nó được đọc khi câu được nhận và đọc
lại khi câu đã được xét, nên giọng khác nghe thấy ngay trong câu ấy khiến cả
việc ghi nhớ cũng thuộc về căn phòng. Một câu được nhận là của căn phòng mà mắt
gọi tên họ muộn (#2807) cũng vậy: chỉ việc ghi nhớ là của họ. Chỉ có tác dụng
cùng `LAMP_PERSON_SCOPE_SEEN` và khi quy tắc giữ phòng của từng câu
(`LAMP_JOIN_CUT_WORDS` hoặc `LAMP_SECOND_NAME_CARRIES`)
(`worker::one_face_not_acted`); thiếu một trong hai thì lý do được ghi lúc khởi
động và công tắc không làm gì. Công tắc không chờ việc kiểm tra cùng giọng
(phương án 1 của người xây dựng đầu tiên trên #3997): không khởi động được bộ
lấy dấu giọng, hoặc là tuyến dự phòng bên cạnh Live, thì không câu nào được nhận
là giọng khác, chỉ thấy mặt thứ hai nói mới cho biết có người khác; vì chỉ việc
ghi nhớ là của họ, cái giá là lời được lưu, không bao giờ bị đọc. Có mặt thứ hai, mặt lạ,
không có mặt nào, hoặc có giọng khác, thì như trước. Một câu được nhận là của
căn phòng vì không nhận ra ai thì không đổi, nên giọng chưa nhận ra vẫn không
chạm được ký ức của ai (#4196, #4207). **Việc quên theo việc ghi:** một
`forget` một mục trên câu như vậy xoá khỏi phạm vi của họ điều mà cùng quy tắc
đã lưu ở đó, và chỉ tìm thêm trong phạm vi của căn phòng khi của họ không có gì
về điều ấy (lần kiểm tra của Fourteen trên lamp-9adf, bản 9699: «forget my
coffee» chỉ tìm trong phạm vi căn phòng trong khi cà phê được lưu là của họ,
lỗi đã có từ bản 9610). Việc «không có gì» ấy là một kết quả do chính lệnh
quên đặt ra (`remember::run_one_face_forget`), không bao giờ đọc từ câu trả
lời: khi của họ có điều khớp nhưng không quên gì (hơn tám điều, nên hỏi là
điều nào; danh sách bị cắt; ký ức cũ từ chối danh sách), phạm vi của căn phòng
không bị chạm tới (P2 của Reviewer-lead trên #5022). Một `forget {about}`
không nêu tên đã tự tìm trong phạm vi căn phòng khi họ không có gì của riêng
mình, nên phạm vi căn phòng chỉ được chạy lại cho lệnh quên có nêu tên trong
lời nghe được. Nó chỉ chạm tới khuôn mặt quen duy nhất được nhận cho câu này,
được kiểm tra lại ngay trước mỗi lần chạy trong phạm vi của họ, cả cho việc
ghi (`remember::run_one_face_keep`): có mặt thứ hai, mặt lạ, không có mặt,
giọng khác hoặc bị thu hẹp sau đó, kể cả giữa lần đọc ấy và lần chạy, thì chỉ
là của căn phòng. `forget_everything` trên câu như vậy vẫn chạy trong phạm vi của
căn phòng. Chỉ lệnh quên theo điều nó nói tới hoặc theo id mà `recall` đã đưa mới chạm
tới phạm vi của họ: một câu «forget that» trống, hoặc bất kỳ lệnh quên nào mà
ký ức sẽ chạy như «điều được lưu gần nhất» (ký ức cũ, công tắc tắt), chỉ là của
căn phòng, vì sau lần `recall` của căn phòng trên câu như vậy nó sẽ lấy mất
điều họ lưu gần nhất (P2 của Thirteen trên #5022). Id được đếm theo từng
người, nên số 3 của họ và số 3 của căn phòng là hai điều khác nhau: một lệnh
quên theo id chỉ đi tới phạm vi mà danh sách của nó đã cho thấy id ấy trong
phiên này, và một id đã hiện ở cả hai thì không quên gì và mô hình hỏi là
cái nào (`remember::by_where_shown`; P2 thứ hai của Thirteen trên #5022). Và **một
lời «có» nói trên câu như vậy không bao giờ chạy một lệnh quên đang giữ**
(`ScopeLease::by_one_face`): lời «có» vẫn cần thấy mặt họ nói. Rủi ro còn lại:
một giọng ngoài khung hình khi họ ngồi một mình trong khung, không có dấu giọng
để phân biệt, có thể được lưu vào ký ức của họ, và lệnh `forget` một mục của
giọng ấy có thể xoá trong đó tối đa tám điều khớp (một `forget {about}` quên
nhiều nhất tám điều, và không quên gì khi có nhiều hơn), không bao giờ tất cả
(không gì được đọc lại cho giọng ấy). Một câu được lưu là của bạn khi bạn là khuôn mặt duy nhất trong khung
hình có thể chứa lời người khác nói ngoài khung hình.

**Lệnh quên tới cả dòng của căn phòng nói lại điều họ giữ** (GATE 23 FAIL
của Core-verifier trên lamp-52e6, bản ổn định 9980, ngày 4/11): câu «remember
my favourite drink is jasmine cheese» của Henry được giữ là của Henry, câu trả
lời ở lượt sau «Jasmine cheese, that's the favorite I have for you, Henry.» vào
nhật ký của căn phòng, và «forget my favourite drink» xoá điều đã giữ nhưng dòng
của căn phòng vẫn còn «jasmine»: không gì nối dòng đó với điều Henry giữ, vì
nguồn của một dòng chỉ thuộc phạm vi của chính nó. Nay một dòng của căn phòng
mà việc ghi nhớ thuộc về một người quen theo quy tắc này
(`memory_scope::one_face_line`, đánh dấu bởi `take_by_one_face` và
`keep_for_one_face`, bỏ dấu khi bị thu hẹp) trích dẫn những điều người ấy giữ
trong phiên này mà dòng đó mang từ khoá của chúng (`remember::keeper_cited`),
gửi bằng `keeper` và `keeper_ids` trong `append_linked`
(`Days::append_keeper`). Lệnh quên một điều của họ khi đó xoá luôn dòng ấy nếu
dòng mang từ của điều đó (quy tắc chọn lọc, như với dòng của chính họ), và
«quên hết» của họ xoá mọi dòng của căn phòng trích dẫn điều họ giữ, cùng những
gì căn phòng ghi nhận từ các dòng ấy (một sở thích «Henry's favourite is jasmine
cheese») khi nó mang một từ của một điều họ giữ, đọc như quy tắc chọn lọc đọc
(P2 của Thirteen trên #5291). Không dòng
nào khác của căn phòng bị chạm tới: câu của một người khách, có giọng khác, không
thuộc về ai và không trích dẫn gì của họ; các lần đọc của căn phòng không bao giờ
cho thấy liên kết này. Chính câu ghi nhớ không vào nhật ký, như mọi câu ghi nhớ,
nên dòng mang từ ấy là câu trả lời sau nó. Ký ức từ chối các trường này (một
bản cũ, hoặc bản hiện tại từ chối người giữ ấy) nhận lại dòng một lần nữa không
có chúng, được ghi lại như trước.

**Lời nhắc của họ trên câu như vậy** (`LAMP_ONE_FACE_REMINDERS=on`, bật trong
bản sản phẩm, quyết định của người phụ trách ký ức ngày 4/11): việc ghi nhớ
của câu cũng quyết định việc đọc lời nhắc. `list_reminders`, `cancel_reminder`
và `snooze_reminder` hỏi Schedule với tư cách người mà việc ghi nhớ thuộc về
(`ScopeLease::kept_by_one_face`, `tools::schedule_whose`), nên họ nghe được lời
nhắc của chính mình và của căn phòng (`Asker::may_see`), không bao giờ của người
khác. Trên lamp-29e7 ngày 3/11, mặt người ấy là mặt duy nhất trong 17/17 lần
nhìn, đọc là đang nói 0 lần, và «What reminders do I have?» bỏ sót lời nhắc của
chính họ. Không gì khác được mở rộng: `recall`, gợi ý, ghi chú và ngữ cảnh vẫn
là của căn phòng như trên; có mặt thứ hai, mặt lạ, giọng khác, bị thu hẹp sau
đó, hoặc tắt một trong hai công tắc thì câu vẫn là của căn phòng. Rủi ro đã
biết, được người phụ trách ký ức chấp nhận ngày 4/11: một giọng không được thấy
nói, khi chỉ có mặt người ấy trong khung hình, nghe được lời nhắc của họ và có
thể huỷ hoặc hoãn chúng.

**Gợi ý ký ức ở mỗi lượt** (`LAMP_SECOND_MEMORY_HITS=on`, bật trong bản sản
phẩm, quyết định của người phụ trách ký ức ngày 4/11: «Build it that way, on by
default … and accept the privacy leak»). Mỗi câu nói được đối chiếu với những
điều đã lưu, tối đa ba điều được kể như ghi chú. Phạm vi theo thứ tự
(`worker::hits_whose_by`, `worker::hits_of_lease`):

1. **Thấy đang nói** (`hits_path=speaking`): người mà chính mặt họ được thấy nói
   câu ấy (cùng cách đọc mà việc lưu, `recall` và ngữ cảnh dùng, #3437).
2. **Mặt trong khung hình** (`hits_path=face`), phương án dự phòng đã chấp nhận:
   không mặt nào được thấy nói câu ấy, và mặt quen của người được nhận cho câu
   ấy là mặt duy nhất trong khung hình suốt câu, theo cách đếm một mặt của việc
   lưu (`SpeakingFaces::one_face_in_view`: có trong ít nhất ba lần nhìn, không lần
   nào có mặt khác; không có lần nhìn nào thì không tính là một mình)
   (`ScopeLease::hits_by_face`). Thêm một mặt thứ hai, dù im lặng, thì gợi ý vẫn
   là của căn phòng: người ngồi đó sẽ nghe ghi chú của người kia (first builder
   trên #5272). Chỉ gợi ý đọc
   phạm vi này; mọi thứ khác của câu vẫn là của căn phòng. Một lần thu hẹp sau đó
   (giọng khác, lời người khác được nối vào) huỷ nó.
3. **Của căn phòng** (`hits_path=room`): không ai được nêu, hoặc một mặt khác
   (mặt lạ hay mặt quen khác) được thấy nói câu ấy: không bao giờ đọc phạm vi của
   mặt đang im lặng.
4. **Không gì cả**: yêu cầu đã bị thu hồi.

Rủi ro đã biết, được người phụ trách ký ức chấp nhận ngày 4/11 (reviewer-two
trên #3371): khi mặt một người quen là mặt duy nhất trong khung hình và im lặng, không mặt nào
được thấy nói, câu trả lời cho một người gọi ngoài khung hình được kể những điều
người ấy đã lưu. Câu đang được mang theo như cuộc trò chuyện của một người
(`LAMP_CONVERSATION_CARRIES`) vẫn không có gợi ý. Nhật ký mỗi lượt ghi
`hits_path=` và số lượt theo từng đường (`hits_turns_speaking=`,
`hits_turns_face=`, `hits_turns_room=`), rồi `hits_whose=` (`room`, hoặc
số thứ tự của người trong ngày, `person-1`, `person-2`, … theo thứ tự mỗi người
xuất hiện lần đầu trong ngày; không bao giờ là tên hay id của người, vì id là
hash không muối của tên; bảng id sang số thứ tự chỉ ở trong bộ nhớ, không bao
giờ được ghi ra) cùng tổng trong ngày của ký ức ấy: số điều đã lưu
(`hits_today_told=`) và số byte (`bytes_today_told=`) đã kể qua gợi ý hôm nay,
chỉ giữ trong bộ nhớ của voice, số thứ tự và tổng cùng đặt lại lúc nửa đêm giờ địa phương và khi khởi
động lại (`remember::HitsDay`, C4), để đo ngân sách tiết lộ (C9) từ nhật ký.
Chỉ là số đếm và số thứ tự, không tên, id hay lời.

## Lượt ban đêm B (kế hoạch trích xuất C, bước B, của người phụ trách trí nhớ, 3/11)

Một lượt mô hình mà voice chạy mỗi đêm cho một người đã biết, sau công tắc
`LAMP_MEMORY_NIGHTLY` (phía voice). Nó ghi vào kho này những gì Tiers đề
xuất: các đoạn (episode), sự thật (fact), dòng hồ sơ (profile), thói quen
lặp lại (pattern), việc sắp tới (ahead) và cảm xúc (feeling)
(`crates/memory/src/nightly.rs`). Các loại và quy tắc kiểm tra là của Tiers
(`docs/memory-design.md` và `docs/memory-tiers.md` trên `tiers/recall-adapter`):
mô hình chỉ trỏ tới id bản ghi, mã chép lời trích; chỉ bản ghi của chính
người đó; một pattern cần bản ghi trên hai ngày; thay thế giữ lịch sử; một
kế hoạch hết hạn 14 ngày sau nguồn mới nhất; một cuộc hẹn đóng sau ngày của
nó; hồ sơ tối đa 60 dòng và 2 KiB, và một đêm đổi tối đa một phần tư. Khác
biệt là đơn vị ghi: Tiers ghi cả đêm hoặc không gì, nên một mục hỏng làm mất
cả đêm (34 lần; Tiers đạt 0,35). Ở đây từng mục được kiểm tra riêng: mục hỏng
bị bỏ và đếm theo lý do, mục tốt được ghi.

- `nightly_prepare {person, day, after_id?}`: các bản ghi đủ điều kiện của
  người đó trong ngày địa phương `day` (lượt trao đổi có `heard` không rỗng,
  trừ lượt nói trước một lần quên của người đó trong ngày hoặc trong một
  phút sau nó, và điều đã giữ trong ngày), theo id sau `after_id`, tối đa 128 bản ghi và
  64 KiB; kèm `existing` (mọi dòng hồ sơ hiện tại, rồi 32 fact, episode,
  ahead mới nhất). Chỉ người đã biết (`p-` và 16 chữ số hex); phòng hay phạm
  vi quy tắc là `bad_person`. Công việc chỉ giữ id, không giữ chữ.
- `nightly_apply {person, job, proposal, patterns?, feelings?}`: từng mục
  được kiểm tra riêng; câu trả lời đếm `accepted`, `dropped` theo loại và
  `reasons` theo lý do, không bao giờ kèm chữ. `patterns` và `feelings` mặc
  định tắt: mục của chúng bị bỏ `off`.
- `search`/`recall` với `nightly:true` thêm `nightly:[…]`, các mục hiện tại
  khớp truy vấn, trong ngày hay khoảng ngày theo ngày của mục, tối đa 8 và
  4.096 byte. Không có trường này, câu trả lời giữ nguyên từng byte.

Quyền riêng tư (`private_kind`): không giữ sức khoẻ, thuốc men, tiền bạc hay
chuyện riêng của người khác, trừ khi mọi nguồn mục đó dẫn đều là điều người
đó đã nhờ đèn ghi nhớ (hoặc mục dựa trên điều đó); một điều đã giữ lẫn giữa các
trao đổi là chưa đủ. Kiểm tra bằng danh sách từ tiếng Anh và tiếng Việt
(`nightly::private`), là lớp chặn dự phòng sau lời nhắc; một cách diễn đạt
khác có thể lọt. Dòng `relationship` chỉ gồm tên và quan hệ, tối đa 8 từ;
không có loại `worry`.

Lưu trữ: mỗi mục là một hàng `entries` (`kind='observation'`, `category` là
loại của nó), nên tìm kiếm, nguồn, thay thế, quên và xoá an toàn như các
quan sát; trường riêng nằm trong `nightly_items`, công việc trong
`nightly_jobs`, hạn mức của đêm trong `nightly_nights`, tạo `IF NOT EXISTS`,
`user_version` vẫn là 1. Mục không hết hạn vì không dùng hay theo 30 ngày của
lượt trao đổi; chỉ theo ngày kết thúc của nó (ahead, plan, `valid_until`).
Mỗi người giữ tối đa 512 mục còn hiệu lực (`full`); mục đã bị thay thế hay đã
hết hạn là lịch sử và không được tính.
Một lệnh quên xoá mọi mục dẫn tới thứ bị quên, lan tiếp (dòng hồ sơ dẫn một
fact bị quên); quên trong lúc lượt đang chạy thì khi áp dụng, các mục dẫn tới
thứ đã mất bị bỏ `forgotten`, phần còn lại vẫn ghi.

Dấu quên (phương án 2 của El trên #5333): quên theo chủ đề (`forget
{about}`) chỉ khớp điều đã giữ và quan sát, nên một lượt nói chưa từng được
giữ không có gì để khớp và vẫn còn 30 ngày; lượt đêm sẽ đọc và giữ nó kèm lời
trích. Vì vậy mọi lần quên của một người đã biết đánh dấu ngày đó: `forget`,
`forget_last` (thấy hay không), và `forget_mark {person}` voice gửi sau một
`forget {about}`, thấy hay không (`{ok:true}`; phòng hay phạm vi quy tắc
không đánh dấu; bộ nhớ cũ trả `unknown_op` và voice đi tiếp). Mỗi người, mỗi
ngày địa phương một hàng `nightly_forgets(person, day, at_unix_ms)`, giờ của
lần quên mới nhất, tạo `IF NOT EXISTS`, `user_version` vẫn là 1. Mỗi lần
quên đánh dấu hôm nay và `DAYS_BACK` (3) ngày trước đó, cùng một thời điểm:
voice chỉ chạy lượt cho một ngày vào đêm sau và giữ ngày đó chờ tối đa 3
ngày, nên lần quên lúc 00:10 vẫn tới được câu nói lúc 23:40, và lần quên
trước một lượt chạy muộn vẫn tới ngày của nó (P2 của Thirteen trên #5345).
`nightly::DAYS_BACK` của bộ nhớ sao lại hằng số cùng tên của voice;
`tests/nightly_days_back.rs` hỏng khi hai số khác nhau. Ngày xa hơn không
được đánh dấu: lượt của nó đã chạy hoặc sẽ không chạy.
`nightly_prepare` bỏ qua các lượt trao đổi của người đó trong ngày đã đánh dấu nói trước
dấu cộng `FORGET_TURN_MS` (60 giây): chính câu yêu cầu quên cũng là lượt
trao đổi của ngày ("quên điều tôi nói về chị tôi" nhắc lại chủ đề), nên
cũng bị bỏ qua (ý của Heron trên #5333). Lượt nói sau đó vẫn được đọc. Điều
đã giữ không bị bỏ qua. Quên trong lúc lượt đang chạy cũng vậy: khi áp
dụng, lượt trao đổi đó bị bỏ `forgotten` cùng mọi mục dẫn tới nó. Lượt nói
của người khác, và của ngày ngoài khoảng đó, giữ nguyên. Độ trễ: voice gửi
`forget_mark` sau `forget {about}` trong 1,5 giây của lệnh, nhưng ít nhất
250 ms (`FORGET_MARK_WITHIN`) khi lệnh quên đã dùng hết, nên trường hợp xấu
nhất của lượt khoảng 1,75 giây trên socket cục bộ. `forget_all` và `forget_person`
xoá dấu của người đó; một dấu mất khi ngày của nó quá `keep_days`.

Dòng hồ sơ vào dải sở
thích của ghi chú phiên (P3, #5043); các loại khác không bao giờ vào ghi chú
phiên. Không có gì được nói ra hay hiển thị.

## Đầu dò của bộ đo (#4748)

Bộ đo bộ nhớ (`docs/memory-bench.md`) chạy một tháng qua chính client Days của
Voice và kho thật. `lamp_voice::remember::probe` cho nó những gì cần. Nó chỉ có
trong bản dựng kiểm thử hoặc bản dựng với `RUSTFLAGS="--cfg lamp_memory_probe"`;
không bản dựng sản phẩm nào đặt cờ đó, không feature nào bật nó, và `lamp-voice`
từ chối dựng nó ở chế độ tối ưu (`compile_error!` trong `lib.rs`). Không có nó,
mọi dòng đã đổi biên dịch như cũ: phần điều phối đọc `unix_ms()` và
`local_time::system()`, và không hàng đợi nào có biến thể rào chắn. Nó không có
socket, cờ hay công tắc riêng.

- **Đồng hồ.** `Clock::install(start_ms, zone)`, một lần mỗi tiến trình, chỉ đi
  tới (`set`). Khi đã cài, phần điều phối công cụ (`run_for_request`), ngày sau
  lệnh quên của phòng, các lệnh quy tắc, giờ của lượt chuyển, `today` của lượt
  quan sát và tuổi của ghi chú đang chạy đều đọc nó.
  `DaysClient::run_probe_at(call, deadline, person, request, now_ms, zone)` nhận
  thời điểm cho từng lệnh, giữ kiểm tra phạm vi, định danh yêu cầu và câu trả
  lời của lệnh quên ngoài luồng, như `run_in`.
- **Xả hàng đợi.** `drain(client, within, cancel)` đặt rào chắn vào hàng đợi của
  bộ ghi, rồi của lượt quan sát, rồi của luồng tóm tắt, mỗi cái sau khi cái
  trước đã trả lời. Rào chắn chỉ được trả lời khi mọi việc trước nó xong, kể cả
  câu trả lời của kho. Hết giờ, bị huỷ, một câu trả lời đang giữ hay một lượt bị
  mất thì không có `Drained`.
- **Điểm lưu và khôi phục.** `checkpoint` nhận một `Drained` (không sao chép
  được), chép các tệp của kho đã dừng và trạng thái của client cùng các ghi chú
  đang chạy. Nó từ chối khi cuộc trò chuyện còn mở, khi tệp đổi trong lúc chép,
  hay khi có lệnh quên hoặc lượt mới sau lần xả, và không để lại gì trừ khi mọi
  bước xong. `restore` đặt lại trên cùng client và kho; `copy_store` cho một
  tiến trình con dùng một lần.

Giới hạn (reviewer-three): khôi phục bị từ chối sau bất kỳ lệnh quên nào kể từ
điểm lưu, nên không gì đã quên quay lại; hết giờ hay huỷ không bao giờ cho một
điểm lưu. Một đầu dò là một góc nhìn cho mỗi tiến trình.

## Đã nói, dự định hay có lẽ (`LAMP_MEMORY_STRENGTH`)

Theo kế hoạch đưa ý tưởng của Tiers vào Days của người phụ trách trí nhớ
(3/11, P2; phỏng theo `Strength`, `combine` và `constrained_by_words` của
memory-tiers, xem [memory-tiers.md](../memory-tiers.md)). Khi bật (chỉ giá trị
`on`, chỉ với Days; mặc định tắt), `search`, `recall` và `session_open` của
giọng nói mang `"strength":true`, và mỗi dòng trong câu trả lời mà chính lời
của nó là một dự định hay một điều chưa chắc sẽ có `"strength":"plan"` hoặc
`"strength":"maybe"`; một dòng đã nói như sự thật thì không có trường này.
Nhãn được tính khi đọc, từ lời của dòng (lời đã nghe của một lượt hội thoại,
không bao giờ lời của đèn), và không lưu ở đâu: không đổi schema, không
migration, không hết hạn, không gọi model. Chưa chắc: "maybe", "might",
"not decided", "not booked", "có lẽ", "chưa quyết", "chưa chắc", "không chắc",
"chưa đặt". Dự định: "will", "going to", "plan", "định" (trừ "quyết định",
"ổn định"…), "dự định", "kế hoạch". Chỉ là danh sách từ tiếng Anh và tiếng
Việt: nhãn chỉ làm một dòng yếu hơn "đã nói", không bao giờ bỏ dòng nào, và
một câu hỏi hay một phủ định không bao giờ làm nó mạnh hơn. Ghi chú phiên có
nhãn vẫn trong giới hạn 1.200 byte: nếu vượt, các điều cuối cùng bị bỏ cùng
id của chúng. Giọng nói nói với model dòng đó đã được nói thế nào, không phải
bây giờ ra sao: một điều chưa chắc là "maybe: said as a maybe; not known to
be decided or booked" và một dự định là "plan: said as a plan; not known to
be done". Với dòng có `event_status` là past, nó nói "said as a plan (hoặc a
maybe) for that day; whether it happened is not known". Khi tắt, mọi
yêu cầu và câu trả lời giống hệt từng byte như trước
(`days_strength::off_is_byte_identical`). Memory cũ từ chối trường này, nên
cập nhật cả hai cùng lúc.

## Sở thích giữ theo quy tắc (`--taste-rules`, kế hoạch C bước A)

Kế hoạch C của người phụ trách trí nhớ (3/11) học sở thích, thói quen và tóm
tắt qua hai bước: A, quy tắc bằng mã không dùng mô hình; rồi B, một mô hình
chạy hằng đêm với kiểm tra từng mục. Đây là A. Chạy `lamp-memory
--memory-days --taste-rules` để bật; không có cờ thì không gì thay đổi (cờ cần
`--memory-days`, và dòng khởi động khi đó ghi `taste_rules=on`).

Khi bật, mỗi lượt ghi vào phạm vi riêng của một người đã biết (id `p-…`) được
`crates/memory/src/taste_rules.rs` đọc ngay trong lần ghi, trước khi commit.
Chỉ đọc `heard`, lời người đó nói; câu `said` của lamp không bao giờ được đọc.
Phạm vi của phòng (`everyone`: khách, người lạ, cả câu của phòng mà mặt một
người quen được thấy nói) và phạm vi quy tắc không giữ gì. Câu thuộc phạm vi
nào vẫn do quy tắc của voice quyết định (`memory_scope.rs`), không đổi. Mỗi
câu (đến `.`, `!`, `?`, `;` hoặc xuống dòng, tối đa bốn câu một lượt, 300 byte
một câu) nói sở thích của chính người đó về một thứ thường ngày thì được giữ:

| Loại | Tiếng Anh | Tiếng Việt |
|---|---|---|
| `preference` | I love / like / really like / enjoy / prefer / adore …; my favourite X is Y | tôi / em / mình (rất, cũng …) thích …; X yêu thích (nhất) của tôi / em / mình là … |
| `dislike` | I hate / dislike / can't stand …; I don't / do not / no longer / never like … | … ghét …; … không / chẳng (còn) thích … |

**Sức khỏe, tiền bạc và người khác không bao giờ được giữ theo quy tắc.** Sản
phẩm không giữ những điều đó trừ khi người dùng nói rõ muốn lamp ghi nhớ
(quy tắc của lượt quan sát, `remember::OBSERVE_PROMPT`, và
[memory.md](../memory.md)), nên không có quy tắc dị ứng hay ăn chay («tôi dị
ứng tôm», «tôi ăn chay», «I'm allergic to peanuts» không giữ gì), không có quy
tắc cho điều về người khác («my wife likes coffee», «tên của mẹ tôi là Lan»),
và một câu sở thích có bất kỳ từ nào về sức khỏe, thuốc, ăn kiêng, tiền hay
người khác (`taste_rules::SENSITIVE`: «tôi ghét uống thuốc», «tôi thích
tiền», «tôi thích nấu ăn cho vợ») bị bỏ qua cả câu. Từ chỉ người có thêm
«'s» cũng tính như chính từ đó («I hate my boss's jokes», «I love my sister's
cooking»). Câu cũng bị bỏ khi đối tượng, như đã nghe, có một từ viết hoa khác
«I» («I can't stand Tom», «tôi ghét Lan»; tên hãng viết hoa cũng bị bỏ, vì quy
tắc không phân biệt được với tên người), hoặc khi có lời thuật lại sau động
từ (`says`, `said`, `told`, `tells`, `nói`, `bảo`, `kể`: «I hate broccoli,
Minh says»). Nói «hãy nhớ …» vẫn giữ
những điều đó như trước, theo chính yêu cầu của người dùng.

Quan sát là đúng câu đó, mã chép nguyên văn như đã nghe, không diễn đạt lại;
nhóm `preference` (thứ ghi chú phiên và lượt tidy đã đọc), loại và điều nó nói
về nằm trong bảng phụ `tastes(rid, kind, about)` chỉ được tạo khi bật cờ
(`Days::taste_kind`), nguồn là lượt hội thoại, thời điểm là của lượt đó. Nó
được tìm kiếm, đưa vào ghi chú của phiên sau và hết hạn như mọi quan sát
(30 ngày không dùng).

Cũng bỏ qua: câu kết thúc bằng `?`, hay câu tiếng Việt kết thúc bằng `không`,
`à`, `nhỉ`, `chưa`… («em thích cà phê không»); câu do dự (`maybe`, `might`,
`think`, `có lẽ`, `chắc`, `hình như`, `có thể`): điều «có lẽ» không bao giờ
được giữ như sự thật; câu không mở đầu bằng chính người nói; đối tượng không
tự nói gì («I love it», «em thích anh»); đối tượng bắt đầu bằng «to» và một
động từ («I hate to bother you»), nói về người nghe (`you`, `your`, `bạn`: «I
like the way you said that») hay chỉ là «nothing», «none», «everything»,
«anything»; mong muốn hay quá khứ («I'd like a
coffee», «I used to love coffee»). Độ chắc (P2, #5052) không được dùng.

Một sở thích mới hơn về cùng một thứ (cùng đối tượng, cùng món yêu thích)
thay thế cái cũ do quy tắc giữ bằng một cạnh `replacements`: «Tôi không thích
trà sữa nữa» sau «Tôi thích trà sữa» giữ câu cũ làm lịch sử không còn hiệu
lực (`history:true`). Cùng lời nói lại thì không giữ thêm, cũng như khi phạm
vi đã giữ điều đó cùng lời (`tidy::restates`). Quy tắc không bao giờ thay thế
quan sát của mô hình.

Quên dùng các liên kết sẵn có: `forget {id}` của lượt hội thoại xóa quan sát
và các phiên bản của nó; `forget {id}` của quan sát xóa lượt gốc của nó;
`forget_last`, `forget_all` và `forget_person` xóa nó cùng phần còn lại, và
dòng `tastes` đi theo khóa ngoại. Lỗi trong quy tắc chỉ hoàn tác savepoint của
chúng, không bao giờ hoàn tác lượt hội thoại. Không gọi mô hình hay mạng, chỉ
tra theo chỉ mục. Trên Mac ở bản debug, đọc một câu mất vài micro giây; trung
vị một lần ghi với 200 quan sát sẵn có là 67 ms khi tắt và 104 ms khi bật,
với mỗi câu giữ hai quan sát, phần chênh chủ yếu là các dòng đó trong lần
commit bền vững (`it::days_tastes::the_rules_add_little_to_the_write`). Khi
tắt, kho giữ đúng những dòng như trước
(`it::days_tastes::off_the_store_is_unchanged`). Kiểm thử:
`taste_rules::tests` và `it::days_tastes`. Chưa đo bằng bộ đo, chưa chạy trên
lamp.

Ý tưởng quan sát có liên kết nguồn, chép đúng lời của người nói, là sự thật
kèm trích dẫn nguyên văn của thiết kế Tiers
([memory-tiers.md](../memory-tiers.md)), ở đây không dùng mô hình.
