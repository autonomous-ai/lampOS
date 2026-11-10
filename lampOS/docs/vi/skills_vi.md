# Kỹ năng vật lý: tạo bằng giọng nói

Trang này chỉ dịch phần mới về kỹ năng tạo bằng giọng nói. Toàn bộ thiết
kế kỹ năng là [skills.md](../skills.md), bằng tiếng Anh; nếu hai trang khác
nhau, bản tiếng Anh đúng.

## Tạo bằng giọng nói (#1685, `LAMP_MAKE_SKILL=on`, bật trong sản phẩm)

Từ fourteen-P15 (first builder đồng ý, 30/10 15:5x), `LAMP_MAKE_SKILL` và
`LAMP_APP_MAKE_SKILL` bật trong sản phẩm. Kỹ năng do chính mô hình của đèn
viết, không cần máy tính và không cần phần tasks (`crate::make_skill_local`,
đề xuất của Fourteen trên #4710, 2/11). Đó là cách tạo duy nhất: tác tử trên
máy tính không còn tạo kỹ năng nữa (first builder, 3/11, trên Queue), và
`LAMP_MAKE_SKILL_LOCAL`, công tắc từng chọn giữa hai cách, đã bị bỏ
([switches.md](../switches.md)). Kỹ năng tạo ra vẫn chỉ được bật bằng một câu
"có" nói ở đèn.

"Lamp, tạo cho tôi một kỹ năng làm đèn dịu đi lúc 10 giờ tối." Diễn ra như
sau:

1. **Hàm của riêng giọng nói.** `make_skill(request)` là của giọng nói,
   khai báo một lần và đi tới `Route::MakeLocal`, có hay không có phần
   tasks; phần tasks không bao giờ khai báo nó, và kỹ năng không bao giờ
   được giao cho máy tính. Không còn `put_on_made_skill` từ #5977: kỹ năng
   đã tạo được bật ngay khi được giữ (bước 8). Vì vậy nó chỉ được tạo
   trong một lượt có người nói (`heard`): trong lượt sự kiện của chính
   đèn, một lần giao việc hay theo ý riêng của mô hình, `make_skill` không
   tạo, không giữ gì và được báo `make_skill_local::NOT_ASKED_ALOUD` (P2
   của Thirteen trên #5985).
   `make_skill` tạo ngay, không đọc lại: mô hình nói vài chữ rằng nó đang
   tạo, và người dùng chờ. Công tắc chỉ được đưa cho worker của tuyến thứ
   hai (`process.rs`), worker này giữ cấu hình chat như khi nó khởi động,
   kể cả khóa riêng của người dùng (`make_skill_local::set_config`); tuyến
   Live không bao giờ có nó.
2. **Bản hướng dẫn** (`made_skill::brief`): chữ cố định, gồm định dạng kỹ
   năng và những gì một kỹ năng tạo ra được phép dùng, rồi đến yêu cầu của
   người dùng.
   - Yêu cầu tối đa 480 byte; dài hơn thì bị từ chối, không bao giờ bị cắt,
     và khi đó không hỏi mô hình gì cả.
   - Không có gì khác của cuộc trò chuyện đi kèm.
3. **Mô hình của đèn viết.** Bản hướng dẫn được gửi tới chat của tuyến thứ
   hai (`llm_*`, relay hoặc DeepSeek riêng của người dùng), không có hàm,
   dạng stream, trên luồng và runtime riêng, tối đa 30 giây
   (`WRITE_WITHIN`) và 20 KiB. Lỗi 404 (của relay, khoảng một phần mười
   ngày 2/11), 5xx, quá giờ, mất kết nối hoặc câu trả lời bị cắt trước khi
   khối cuối đóng thì được hỏi lại một lần (`make_skill_local::again`); lỗi
   khác thì không. Mô hình được xem một kỹ năng hoàn chỉnh mà bước kiểm tra
   chấp nhận (`make_skill_local::EXAMPLE`, `example-only`: màu cyan mức 2
   trong 25 giây khi có người ở đó, được gọi rõ là ví dụ về hình thức,
   "never answer with it") và được dặn rằng kỹ năng không
   nói được và không tự giữ giờ được. Câu trả lời phải đúng hai khối có rào,
   `skill.toml` và `SKILL.md`, mỗi khối tối đa 8 KiB.
4. **Giới hạn những gì kỹ năng tạo ra được dùng** (giới hạn của điều phối
   viên, 19/10):
   - `kind` là installable, có phiên bản;
   - `[permissions]` chỉ có `light`, `body`, và `topics` trong số
     `voice/state` và `room/presence`;
   - không có hàm cho mô hình (`tool`);
   - `[ask] policy` chỉ là `nudge`, không bao giờ là lời chào, tin tức hay lời nhắc;
   - nói, tin nhắn, cái nhìn và mọi chủ đề khác đều bị từ chối.
   Mở rộng giới hạn này là quyết định của trưởng nhóm. Lần mở rộng duy nhất
   (nửa "đọc" của đề xuất #5288, `LAMP_SKILL_READS=on` trong `lamp-voice` và
   `lamp-skills-manage@`, bật trong các unit sản phẩm): `services` nêu một dịch vụ
   liên kết có việc đọc cố định là `"read"`, cùng `speak` để nói nó. Một yêu cầu nêu
   đúng một dịch vụ như vậy và không có từ ghi nào được đưa vào brief với
   bước đọc duy nhất của dịch vụ đó (bước của lịch hay thư được cho trọn; với
   dịch vụ khác, các việc đọc của nó được liệt kê trừ những việc trả về toàn
   văn và dạng tự do), thời điểm trong ngày là của yêu cầu (`during` của
   trigger chỉ ghi dạng `HH:MM`, và chỉ khi yêu cầu nêu giờ: lần thử trên
   lamp-9adf ngày 5/11 đã tạo skill với 08:00 đến 08:30 được nêu làm ví dụ,
   dù yêu cầu không hề nói giờ), và câu "có" nói ra của nó nêu dịch vụ, thứ nó đọc và điều kiện, "when
   you're alone with the lamp, it reads you tomorrow's Google Calendar: how
   many events, then each time and title." Khi tắt, mọi yêu cầu nêu một dịch
   vụ là `NOT_A_SERVICE`, như trước. Khi đã giữ, một kỹ năng có bước đọc một
   dịch vụ chưa liên kết lúc đó (`services_connected`, hỏi một lần, tối đa
   500 ms) vẫn được giữ, và việc tạo báo điều đó: tạo bằng giọng nói,
   `needs_link` trong câu trả lời của việc tạo và một câu nói trước lời hỏi,
   "Gmail isn't linked yet: link it in the app, under Skills, then
   Connectors, and the skill will start then."; tạo từ ứng dụng,
   `needs_link: [{"code", "name"}]` trong `skills.made`. Khi `lamp-services`
   không trả lời thì không có cả hai, và việc tạo vẫn thành
   ([services_vi.md](services_vi.md), "Chưa liên kết lúc tạo").

   Bước `read = { service, what, ... }` (`lamp-skills`, `service_read`): mọi
   dịch vụ có việc đọc cố định (không phải `gmail-app` chỉ gửi, các dịch vụ
   MCP hay Ahrefs), mọi việc đọc cố định của nó, không bao giờ việc trả về
   toàn văn hay dạng tự do (`query` GraphQL, `get` đường dẫn bất kỳ, `call`
   của MCP; `lamp-services` giữ lại ở cửa của nó), `events` của lịch với `day = "today"` hoặc `"tomorrow"`;
   là bước duy nhất, đứng một mình, trong kỹ năng do trigger khởi động, không
   có tool hay `each`, với `[permissions] services = { <service> = "read" }`
   và `speak = true`. `lamp-skills` tự đọc qua socket tools của
   `lamp-services`, một lần mỗi ngày địa phương, khi khuôn mặt quen của chủ
   đèn ở một mình trong tầm nhìn và liên tục 5 s, không nghe giọng nào trong
   một phút qua và đèn không riêng tư, tạm dừng hay trong giờ yên lặng (chờ
   tối đa 60 phút, rồi bỏ qua ngày đó mà không nói), rồi nói số đếm và từng
   dòng ngắn của mỗi mục như văn bản chính xác, xét lại cổng trước mỗi dòng;
   có người đến thì dừng với "I'll tell you the rest later." Journal chỉ giữ
   số đếm và kết quả của cổng ([services_vi.md](services_vi.md), "Một kỹ năng
   tự tạo đọc dịch vụ").
5. **Kiểm tra.** Thẻ được đánh dấu `made: here` (nhóm "Tạo trên đèn này"
   của `lamp-cloud`). Sau đó `lamp-skills pack` (#1698) kiểm tra thư mục:
   mọi kiểm tra của một lần nạp, kể cả quyền, và gói được đọc lại như khi
   cài. Một kỹ năng làm nhiều hơn những gì nó khai báo bị từ chối ở đó.
   - **Sửa một lần** (#2813 phần 2). Khi bước kiểm tra từ chối một câu trả
     lời, nó được gửi lại cho mô hình một lần, trước khi nói gì: bản hướng
     dẫn cũ, kèm các dòng của bước kiểm tra (thư mục được thay bằng
     `<folder>`, tối đa 600 byte) trước lời yêu cầu của người dùng
     (`made_skill::repair_brief`). Câu trả lời đó được giữ, hoặc được nói là
     không tạo được; không bao giờ có lần thứ ba. Chỉ lỗi của bước kiểm tra
     (hoặc của phần nói rõ bên dưới, hoặc khi trả lại chính ví dụ) được
     sửa; sai định dạng hay xin dùng nhiều hơn đèn và đầu thì được nói là
     không tạo được ngay.
   - **Không bao giờ là ví dụ** (lamp-9adf, 3/11 16:15: "when I come back
     to my desk, glow warm" được trả lời bằng đúng nguyên văn ví dụ, ánh
     cyan, được giữ và hỏi; lần thử lại ra đúng các byte đó). Câu trả lời có
     `skill.toml` giống ví dụ (bỏ qua khoảng trắng), hoặc `SKILL.md` mang tên
     của ví dụ, bị từ chối trước bước kiểm tra
     (`make_skill_local::is_example`) và được gửi lại một lần như lỗi của
     bước kiểm tra, với "that is the example, not what was asked; make the
     skill the person asked for". Nếu lại là ví dụ thì được nói là không
     tạo được, "it gave back the lamp's own example, not what was asked";
     không giữ và không hỏi gì.
   - **Nói rõ** (Mobile trên #4710, 3/11). Khi bước kiểm tra đã qua, đèn tự
     viết kỹ năng làm gì từ `skill.toml`, không bao giờ lấy mô tả của mô
     hình (`made_skill::plainly`): "When the lamp has been quiet for a
     minute, between 22:00 and 22:30, it dims the light a step, at most
     once an hour." Câu đó là mô tả của thẻ trong gói (thay mô tả trong
     `SKILL.md`), trong `made.json`, trên ứng dụng và trong lời đèn nói.
     Mô hình được dặn nói câu đó đủ, không tóm tắt. Bị từ chối (và sửa
     một lần): mọi phần cấp ngoài cùng ngoài `kind`, `version`, `needs`,
     `format`, `permissions`, một bảng `[trigger]`, `[ask]` và `[[do]]`
     (nên `[quiet]`, `[[while]]`, `most_a_day`, `[budget]`, `[rest]` khiến
     kỹ năng chạy khác lời nói không bao giờ được giữ, P2 của Thirteen và
     Mobile trên #5042), và câu dài hơn 200 ký tự (nói đủ hoặc không nói,
     không bao giờ cắt). Cũng vậy, trigger có khóa nào ngoài `topic`, `field`, `equals`, `for`,
     `cooldown`, `rearm` và `during`, trigger khác "đèn nghỉ" hoặc "có/không
     có người", hay bước khác một lệnh đèn (`colour`, `level`, `for`, hoặc
     chỉ `dim`) hoặc một cử chỉ (`nod`, `curious`) đều bị từ chối, và được
     sửa một lần như lỗi của bước kiểm tra (dòng của `PLAIN_ONLY`; khi bật
     đọc, thêm `PLAIN_OR_READ`: với kỹ năng đọc một dịch vụ đã liên kết,
     chỉ một bước `[[do]]`, đúng bước đọc như bản tóm tắt đưa ra. Trên
     lamp-9adf, 13:26 ngày 5/11, một lần tạo từ ứng dụng "Read me my latest
     email when I sit down at my desk." bị từ chối hai lần vì lần sửa chỉ
     được dặn đèn và cử chỉ). Trên lamp-0c89, mô hình của đèn
     đã giữ "when someone says good night, dim the light slowly over a
     minute" thành bất kỳ phút yên lặng nào từ 22:00 đến 22:30, dưới mô tả
     của chính nó về điều được yêu cầu.
   - **Điều không kỹ năng nào làm được.** Mô hình được dặn rằng khi yêu cầu
     cần nghe lời nói hay âm thanh, hoặc bất cứ điều gì kỹ năng tạo ra không
     cảm nhận hay làm được, thì trả lời `cannot:` rồi một câu ngắn nói với
     người dùng, bằng lời dễ hiểu, vì sao, kết thúc bằng điều nên nhờ thay
     thế nếu có (với lời nhắc vào một giờ hay đều đặn: "Ask me for a
     reminder instead, like 'remind me every hour to stand up'."), không bao
     giờ thay bằng thứ khác, và không thêm giờ trong ngày mà yêu cầu không
     nêu. Câu trả lời như vậy, không có khối nào, là trọn vẹn (không hỏi lại
     như câu bị cắt) và được báo là không tạo được với đúng lý do đó (#5977;
     chủ đèn, 10/11: "phải nói rõ tại sao không được"): ứng dụng hiển thị nó
     là `why`, và giọng nói nói "I couldn't make that: " rồi lý do. Lý do
     tối đa 240 ký tự (`made_skill::CANNOT`), cắt ở cuối câu, hoặc nếu không
     thì ở cuối một từ, không bao giờ cắt giữa từ và không bao giờ có "…"
     (trên lamp-74ff một lý do cắt ở 120 ký tự kết thúc bằng "beyond s…");
     không giữ gì. Nhật ký cũng ghi: `lamp-voice: a skill made here wasn't
     kept in N ms: <lý do>` (#5977), lời của chính đèn, không bao giờ là
     lời nhờ của người dùng; với lý do `cannot:` của mô hình, có thể lặp
     lại lời của người dùng, chỉ ghi `made_skill::MODELS_CANNOT_JOURNALED`
     (HOLD của Second trên #5985). Khi một phần của yêu cầu không làm được ("dim slowly over a
     minute"), mô hình làm phần còn lại và thêm một dòng ngoài các khối bắt
     đầu bằng `left out:` (`made_skill::LEFT_OUT`); đèn nói dòng đó sau câu
     kỹ năng làm gì, trong một khung cố định như ghi chú của người tạo ("The
     maker notes what it couldn't do: …", một mệnh đề, bỏ dấu ngoặc kép, tối
     đa 120 ký tự), nên người dùng nghe cả phần bị bỏ (#5041). Hai
     dấu hiệu `cannot:` và `left out:` được đọc không phân biệt hoa thường.
6. **Giữ lại, không cài.** Gói nằm trong thư mục riêng của giọng nói,
   `/var/lib/lampOS/voice/made-skills` (0700, người dùng của giọng nói),
   được ghi là tạo từ `local:<ms>`:
   - mỗi kỹ năng một mục: cùng SHA-256, hoặc cùng tên mà chưa bật, khi
     giữ lại sẽ thay mục cũ, ghi theo lần yêu cầu mới nhất, và mục đang bật
     vẫn bật (lamp-9adf, 3/11: ví dụ được giữ hai lần thành hai mục);
   - giữ tối đa 5: quá số đó thì cái cũ nhất chưa bật bị bỏ cùng gói của
     nó, không bao giờ bỏ cái đang bật hay cái vừa giữ; nếu mọi cái khác
     đều bật thì giữ hết, quá 5. Gói mà một mục còn lại vẫn nêu tên thì
     được giữ (lamp-9adf, 3/11: mục trùng đã đẩy mất gói của
     `glow-yellow-on-return`, đang bật);
   - mỗi cái được giữ cùng SHA-256 và những gì nó xin dùng (`made.json`);
   - cùng một `local:<ms>` được giữ lại thì trả về cái đã giữ, không kiểm
     tra lại.
   Một câu trả lời không qua thì được nói là không tạo được ("the skill
   couldn't be made: …"), không có từ kỹ thuật, và không giữ gì. Các tệp
   không bao giờ được đọc ra.
7. **Nói ra, không hỏi** (#5977; chủ đèn, 10/11: kỹ năng đã tạo được bật
   mà không cần lời "có" nói thành tiếng; nút MAKE IT của ứng dụng và lời
   nhờ bằng giọng nói chính là lời hỏi). Khi đã giữ, kỹ năng được bật ngay
   (bước 8), và mô hình được báo tên, đã bật hay chưa, kỹ năng làm gì (được
   nói rõ như trên) và dùng gì ("đèn của nó và việc nó có đang nói hay
   không"), và được dặn nói đúng một câu ngắn, không hỏi gì về việc bật:
   "I made evening dim, and it's on: <what it does>, using its light. Say \"take it off\" to take it off." (`made_skill::on_line`), hoặc khi
   không bật được, "I made it but couldn't put it on: <lý do>."
   (`made_skill::not_on_line`, kèm `not_on` trong câu trả lời). Câu "Shall
   I put it on the lamp?" không bao giờ được nói, không có lời hỏi nào được
   xếp hàng, và không gì chờ lời "có": các quy tắc của #4880, #4914 và
   #4971 về lời hỏi đã bỏ theo. Lời nhờ tạo kỹ năng của chính người dùng
   vẫn là lời hỏi.
8. **Bật ngay** (`made_skill::put_on`): giọng nói đánh dấu kỹ năng đã giữ
   để bật (`goes_on_at_unix_ms`, `made_skill::goes_on`) theo đồng hồ của
   nó, rồi chỉ gửi cho helper quản lý SHA-256 của kỹ năng đó
   (`{"install_made": SHA256}`); helper chỉ cài đúng các byte đó, kiểm lại
   giới hạn, và chỉ trong vòng 30 giây sau dấu (`GOES_ON_WITHIN`, theo đồng
   hồ của chính helper, `made_skill::going_on`), nên một SHA-256 đã lưu
   không bao giờ được cài về sau bằng bất kỳ đường nào. Chờ helper tối đa
   8 giây. Helper trả lời xong thì kỹ năng được ghi `on`; nếu không, nó vẫn
   tắt và lý do được ghi cạnh nó là `not_on`, bằng lời rõ ràng: không với
   tới helper ("the lamp's skills couldn't be reached"), helper không trả
   lời trong 8 giây ("the lamp didn't say in time whether it is on"), một
   kỹ năng cùng tên được bật bằng cách khác ("a skill called evening dim is
   already on the lamp"), hoặc bị từ chối vì lý do khác ("the lamp's check
   didn't let it on"). Không bao giờ chạy khi chưa bật `LAMP_MAKE_SKILL`.
   Việc gỡ một kỹ năng đã tạo (gỡ trong ứng dụng, hoặc `{"off": NAME}`) vẫn
   như cũ.
9. **Thời gian chờ** (P3 của Reviewer-lead trên #4914): lời gọi giữ lượt tối
   đa 30 giây một lần hỏi; với một lần thử lại sau lỗi và một lần sửa sau khi
   bị từ chối, nhiều nhất khoảng 120 giây.

Gỡ một kỹ năng tạo ra bằng giọng nói sẽ đến sau.

Kiểm thử: `make_skill_local::tests` (kể cả
`a_good_answer_is_kept_and_put_on_with_no_yes`,
`a_kept_skill_not_put_on_is_said_with_why_and_stays_off`,
`with_no_tasks_part_a_skill_is_made_and_put_on_by_voice`: trên đèn không có
phần tasks, kỹ năng được tạo và được bật ngay mà không cần lời "có", và
`the_journal_says_why_a_make_was_not_kept`: #5977, nhật ký ghi
`lamp-voice: a skill made here wasn't kept in N ms: <lý do>` bằng lời của
đèn, không bao giờ là lời của người dùng;
`the_models_cannot_reason_is_told_never_journaled`: #5985, lý do được báo
đầy đủ, nhật ký chỉ ghi `MODELS_CANNOT_JOURNALED`), `made_skill::tests`, và
`crates/skills/tests/it/made.rs` với `lamp-skills` thật.

## Gõ trong ứng dụng (#2813, `LAMP_APP_MAKE_SKILL=on`, bật trong sản phẩm)

Cùng người tạo đó, từ chữ gõ trong ứng dụng; được bật như trên, ngay, không cần
lời "có" nói thành tiếng (#5977; nút MAKE IT của ứng dụng là lời hỏi). Cần
`LAMP_MAKE_SKILL=on` và dịch vụ nói (`LAMP_SAY=on`) để nói thành tiếng điều nó
đã tạo; thiếu một trong hai thì socket của ứng dụng không chạy
(`crate::app_make`). Không hỏi phần tasks nào, và không cần phần tasks.

- `{"make": {"words", "request_id"}}`: chữ thành cùng bản hướng dẫn (tối đa
  480 byte) và được mô hình của đèn tạo ngay trên một luồng riêng
  (`make_skill_local::make_kept`, `app_make::make_here`), được ghi là tạo từ
  `local:app:<ms>:1`: `making`, rồi `on`, hoặc `not_kept` kèm lý do.
- `{"ready": {}}`: `{"ready": true}` ngay, vì đèn tự tạo; `lamp-cloud` ghi
  `skills.make` vào `can` chỉ cần có công tắc, nên nút "Make a skill" của ứng
  dụng hiện ngay.
- Khi đã giữ, kỹ năng được bật ngay qua helper quản lý, như bằng giọng nói,
  rồi đèn nói một câu ngắn qua dịch vụ nói, vào lúc thích hợp trong vòng một
  giờ (`app_make::say_line`): "I made evening dim, and it's on: <what it does>, using its light. Say \"take it off\" to take it off.", hoặc "I
  made it but couldn't put it on: <lý do>.". Không hỏi gì, không chờ lời
  "có".
- `{"made": {"request_id"}}`: trạng thái của lời nhờ từ ứng dụng: `making`
  (đang tạo, hoặc đã giữ và đang bật), `on`, hoặc `not_kept` kèm `why` (lời
  rõ ràng của người tạo, "it couldn't be put on: …" khi helper không bật,
  hoặc "it wasn't put on" khi giữ hơn 30 phút mà chưa bật). Lời nhờ mới
  không bao giờ là `kept`, `asked` hay `left_off`; chỉ lời nhờ trước #5977
  chưa được đồng ý mới còn là `left_off`.

Kiểm thử: `app_make::tests`, kể cả `made_here_goes_on_at_once_with_no_ask_said`
(trên đèn không có phần tasks: lời nhờ gõ được tạo, được giữ và được bật
ngay, câu nói ra là câu "it's on") và
`made_here_and_not_put_on_says_why_and_tells_the_app`.

## Kỹ năng viết trong ứng dụng (`LAMP_SKILL_WRITTEN=on`, tắt)

Tắt nếu không đặt `LAMP_SKILL_WRITTEN=on` trong unit của `lamp-voice`, cùng
`LAMP_MAKE_SKILL=on` (thư mục kỹ năng tự tạo và việc bật nó) và
dịch vụ nói (`LAMP_SAY=on`, để nói thành tiếng); và trong unit của
`lamp-cloud`, cùng trang kỹ năng, cho `skills.write` của ứng dụng
([cloud.md](../cloud.md), "A skill written in the app"). First builder
duyệt trên Queue (3/11, 14:33). Người dùng tự viết một kỹ năng bằng lời
của mình, theo kiểu SKILL.md của V1: một tên, khi nào dùng ("khi tôi nói
chế độ học") và cần làm gì, vài dòng. Đèn giữ nguyên văn bản: không mô
hình nào viết hay viết lại một chữ (`made_skill::Written`,
`made_skill::keep_written`, `app_make::write_here`).

1. **Yêu cầu.** `lamp-cloud` gửi `{"written": {"name", "when",
   "instructions", "request_id"}}` tới socket app-make, socket này cũng
   nghe khi chỉ bật công tắc này (và chỉ trả lời yêu cầu này; `make` và
   `ready` vẫn thuộc `LAMP_APP_MAKE_SKILL`). Socket chỉ trả lời
   `lamp-cloud`: bên gọi được đọc bằng `SO_PEERCRED`, chỉ
   `lamp-cloud.service` chạy dưới user `lampos` được trả lời (`app_make::APP_CALLER_USER`; voice chạy bằng root), mọi bên gọi
   khác bị đóng không trả lời (`app_make::app_caller_may`; P2 của App Only
   trên #5985: không còn lời "có", nên không phần nào khác trong nhóm được
   bật kỹ năng). Một dòng tối đa 8 KiB. Mỗi
   trường được cắt khoảng trắng hai đầu và giữ nguyên phần giữa
   (`Written::new`): tên bắt đầu bằng chữ thường, rồi chữ thường, số và
   `-`, tối đa 40 byte (để tên hàm vừa 48 byte của lamp-skills); `when` một
   dòng, tối đa 200 byte; hướng dẫn là văn bản thường, chỉ có xuống dòng
   và tab, tối đa 2.000 byte. Trống, quá dài hay không phải văn bản thường
   thì bị từ chối bằng lời dễ hiểu, trước khi ghi lại bất cứ gì. Đây là
   một yêu cầu của ứng dụng (`app.json`, cùng quy tắc request id, mỗi lần
   một yêu cầu cùng `make`), có `"written": true`, và câu trả lời là
   `{"making": true}` ngay; phần còn lại chạy trên luồng riêng.
2. **Gói do giọng nói tự viết** (`Written::toml`, `card`): `skill.toml` là
   một kỹ năng installable chỉ có một hàm cho mô hình,
   `tool = "written_<tên, '-' thành '_'>"`, **không có trigger** (không bao
   giờ tự chạy) và không có `[ask]`; `[permissions]` chỉ có `speak = true`
   (không đèn, không thân, không topic, không camera, không code); bước duy
   nhất là `say`, một ghi chú. Định dạng của lamp-skills không có bước "mô
   hình làm theo văn bản này", nên dùng cơ chế gần nhất: `say` của một hàm
   được trả lại cho mô hình làm câu trả lời của lời gọi (`{"next": NOTE}`),
   và ghi chú là lời của người dùng, đóng khung là của họ: "The person who
   owns this lamp wrote this skill in the app, in their own words. Follow
   their instructions below now, as their own request; they never let you
   do more than you may.", rồi `When they use it: WHEN` và
   `Their instructions:` với hướng dẫn nguyên văn. Mô hình gọi hàm khi người
   dùng yêu cầu (mô tả nói khi nào), và câu trả lời là chỉ dẫn nó làm theo.
   `SKILL.md` có tên, `description` (lời dễ hiểu bên dưới, có câu `when`:
   điều mô hình được biết về khi nào gọi, và điều ứng dụng hiển thị),
   `made: here`, `written: app`, và phần thân là hướng dẫn nguyên văn.
3. **Kiểm và giữ như một kỹ năng tự tạo** (`made_skill::keep_written`):
   tên trùng một kỹ năng trên đèn (`/var/lib/lampOS/skills/NAME`, hoặc được
   đẩy lên, `/var/lib/lampOS/skills-dev/NAME`) thì bị từ chối; một kỹ năng
   đang giữ cùng tên mà chưa bật được thay bằng cái mới, cùng gói và dấu bật
   của nó, nên bản sửa gửi sau đó được nhận ngay; rồi giới hạn (`written_capped`:
   đúng hình dạng đó, ghi chú đóng khung như trên và trong cùng các giới
   hạn), `lamp-skills pack`, và cùng cách giữ, tối đa năm kỹ năng tự tạo,
   tạo từ `written:app:<ms>:1`. Câu trả lời của một mô hình không bao giờ
   được giữ như vậy: giới hạn của kỹ năng tự tạo (`capped`) từ chối một hàm.
4. **Nói bằng lời dễ hiểu** (`plainly`, mở rộng chỉ cho dạng này): "When
   you ask for it (WHEN), the lamp follows the instructions you wrote:
   INSTRUCTIONS…", `when` cắt còn 60 ký tự và hướng dẫn còn 70, mỗi cái ở
   ranh giới một từ. Nó dùng "its voice".
5. **Bật ngay, không cần lời "có" nói thành tiếng** (#5977; chủ đèn,
   10/11), như kỹ năng tự tạo: theo SHA-256 qua helper quản lý
   (`made_skill::put_on`), helper kiểm lại giới hạn
   (`made_skill::put_on_cap`: của kỹ năng viết khi có hàm, nếu không thì
   của kỹ năng tự tạo) và `made: here`. Rồi đèn nói qua dịch vụ nói, vào
   lúc thích hợp, "I made NAME, and it's on: WHAT IT DOES, using WHAT IT USES. Say \"take it off\" to take it off." (`made_skill::on_line`), hoặc
   lý do không bật được. `skills.made` báo cho ứng dụng `making`, `on` hoặc
   `not_kept` (kèm lý do, trong đó có tên đã dùng hay việc bật thất bại), với
   `"written": true`; trang kỹ năng hiển thị nó trong "Made on this lamp"
   với `"written": true`, không bao giờ như một kỹ năng từ Store.

Kiểm thử: `made_skill::tests::a_written_skill_*`,
`app_make::tests::a_written_skill_*`,
`made::a_written_skill_passes_the_real_checker_and_is_put_on_only_once_marked`
của lamp-skills, và `make_skill::tests::a_written_skill_is_handed_to_the_voice_as_written`
cùng `written_fields_are_held_as_lamp_voice_holds_them` của `lamp-cloud`.

## Gửi kỹ năng tự tạo lên Skills Store (fourteen-P13, `LAMP_SKILLS_SHARE`, bật trong sản phẩm từ 2/11, #4948)

Tắt nếu không đặt `LAMP_SKILLS_SHARE=on` trong unit của `lamp-cloud` (cùng
trang kỹ năng và `LAMP_CLOUD`); unit `lamp-cloud` của sản phẩm bật nó (#4948). Khi bật,
`lamp-cloud` nhận ba lệnh từ ứng dụng, không bao giờ bằng giọng nói:

- **`skills.share`** (`{"name", "author"?}`): chỉ một kỹ năng tạo trên đèn
  này (`made: here`) và đang bật. Thư mục của nó được chép vào `/tmp` riêng
  của `lamp-cloud`, bỏ các tệp chấm (`.package.json`) và bỏ dòng
  `made: here` khỏi `SKILL.md` (trên đèn khác nó không được tạo ở đó), đóng
  gói không ký bằng `lamp_skills::package::pack`, rồi xóa bản chép dù thế
  nào. Gói được gửi tới `POST {host}/api/v1/skills/publish` (`file`, và
  `author` nếu có: tên gọi, tối đa 32 ký tự, một dòng) bằng khóa của đèn.
  `{host}` là scheme và host của `llm_base_url` của tài khoản, không có
  đường dẫn phía sau: các route của Store nằm ở gốc host, không nằm dưới
  base của ping (base đó kết thúc bằng `/api/v1/ai/v1`). Store trả lời
  trong `{"status": 1, "data": ...}`; khi từ chối (lỗi HTTP, hoặc
  `"status": 0`), lý do lấy từ `error_message`, nếu không có thì
  `error_code`, rồi mới đến `message`. Một câu trả lời 2xx mà `data` không
  có id, tên hay trạng thái thì báo "the Skills Store's answer couldn't be
  read", vì ứng dụng sẽ không bao giờ rút lại được nó.
  **`"preview": true`** không gửi gì: bản chép giống hệt được đọc lại thành
  `{"preview": {"name", "files": [{"path", "text"}]}}` (tệp không phải văn
  bản chỉ cho `bytes`; tối đa 65.536 ký tự), để ứng dụng cho chủ đèn xem
  đúng những gì sẽ rời đi trước khi bấm Send.
- **`skills.shared`** (`{}`): `GET {host}/api/v1/skills/mine?limit=100`,
  trang lớn nhất Store cho, mới nhất trước: **chỉ 100 kỹ năng gửi gần
  nhất** hiện ra (với 5 gói một ngày của Store, khoảng hai mươi ngày), các
  dòng lấy từ `data.data`, trả
  `{"shares": [{"id", "name", "version", "review_status", "review_note"?}]}`.
- **`skills.unshare`** (`{"id"}`): `DELETE {host}/api/v1/skills/ID`.

Chỉ các tệp của chính kỹ năng và tên gọi chủ đèn gõ rời khỏi đèn; đường gửi
này không đọc những gì đèn tự giữ: trí nhớ, khuôn mặt, tên đèn biết, bản
ghi, ảnh hay cài đặt. **Nhưng nội dung
kỹ năng là những gì agent viết ra từ lời người ta nhờ,** nên một cái tên
hay một chi tiết trong lời đó ("nhắc Mai uống thuốc lúc 8 giờ") có thể nằm
trong mô tả hoặc thân kỹ năng và đi theo nó. Vì vậy ứng dụng cho xem toàn
bộ nội dung (`preview`) trước khi bấm Send. Ứng dụng của chủ đèn thay mặt
cả đèn, nên có thể gửi một kỹ năng mà người khác ở đèn đã tạo bằng giọng
nói; dù thế nào, phần xem trước vẫn cho thấy nội dung của nó. Người duyệt
của Store đọc nó trước khi lên danh sách; chỉ khi được duyệt nó mới được ký
bằng khóa của Store. Chi tiết, giới hạn và lý do từ chối ở
[cloud.md](../cloud.md), "A made skill sent to the Store".

## Đếm kỹ năng (#4710, cho báo cáo sức khỏe tự nguyện)

Cho trường `skills` trong báo cáo sức khỏe của `lamp-cloud` (`cloud.md`,
"Skills, counted"). Không thêm công tắc nào: `lamp-cloud` chỉ đọc khi ba cổng
của báo cáo đều mở và người dùng đã đồng ý. Chỉ có số và SHA-256, không bao
giờ có tên, lời hay tệp của một kỹ năng.

- **Số lần chạy, trong `lamp-skills`** (`lamp_skills::usage`). Có
  `--state-dir` thì mỗi lần chạy được đếm ngay khi bắt đầu theo nguồn của kỹ
  năng: core, Skills Store (theo SHA-256 của gói, từ `installed_from`), tạo
  tại đây theo người tạo ghi trên chính thẻ của nó (`made: here` kèm
  `made_by: voice` hoặc `made_by: app`, hoặc `written: app` cho kỹ năng viết
  trong ứng dụng; kỹ năng tạo trước khi có dấu này không ghi gì và được đếm
  là `made_unknown`, không bao giờ đoán: `usage::made_by`), do máy tính đã
  ghép đẩy lên, hoặc khác. Nguồn được xác định lúc nạp; mỗi lần chạy chỉ cộng
  một vào một biến nguyên tử, không khóa và không ghi tệp. Một luồng riêng
  cộng phần đã đếm vào `DIR/runs.json` tối đa mỗi phút một lần (ghi trọn bên
  cạnh rồi chuyển đè, 0600, giữ khóa trên `DIR`), nên khởi động lại mất tối
  đa một phút số đếm. Tệp là tổng cộng dồn: `{"version": 1, "core", "store",
  "made_voice", "made_app", "made_unknown", "pushed", "other", "store_ids"}`,
  bảng của Store theo SHA-256, tối đa 256 mục; không có SHA-256 của kỹ năng
  tự tạo. Tệp không đọc được thì bắt đầu lại. Kiểm thử: `usage::tests`.
- **Người tạo nằm trên thẻ** (`made_skill::Maker`). Khi đóng gói, giọng nói
  ghi `made_by: voice` hoặc `made_by: app` (lời nhờ từ ứng dụng có `from` là
  `local:app:…`) cạnh `made: here` trong `SKILL.md`, bên trong gói, nên
  SHA-256 dùng để bật nó cũng bao gồm nó; dòng `made` hay `made_by` do mô hình viết
  bị bỏ. Gói chia sẻ được bỏ dấu này cùng `made: here`.
- **Lời nhờ tạo kỹ năng, bằng giọng nói và từ ứng dụng, trong
  `lamp-voice`** (`made_counts`). Socket của ứng dụng nhận thêm
  `{"counts": {}}` và trả `{"made_by_voice": {"kept", "on", "refused",
  "left_off"}, "made_by_app": {...}}`, là tổng cộng dồn theo người tạo.
  `kept`, `on` và `left_off` (đã giữ mà không bật được, `not_on` của nó;
  hoặc đã được hỏi trước #5977 mà không được đồng ý) đọc từ chỉ mục kỹ năng tự tạo, mỗi kỹ năng đếm
  một lần theo SHA-256, dưới người tạo mà `from` của nó cho biết
  (`local:app:…` hoặc `written:app:…` là của ứng dụng, `local:…` là của
  giọng nói); `refused` (lời nhờ không tạo được gì) được cộng vào một biến
  nguyên tử riêng cho mỗi người tạo, ngay nơi nó được báo (lượt nói, hoặc
  `make_here` của ứng dụng), và ghi tối đa mỗi phút một lần bởi một luồng
  riêng, nên lượt nói không bao giờ chờ tệp. Cả hai người tạo được đếm ở đây,
  trên đèn, nên backend không tự đếm lời nhờ tạo từ ứng dụng và không gì bị
  đếm hai lần (chủ sở hữu, 3/11). Lưu trong `counts.json` cạnh các kỹ năng
  tự tạo. Câu trả lời không nêu gói nào:
  người tạo đã nằm trên thẻ. Kiểm thử: `made_counts::tests`.
- `lamp-cloud` đọc cả hai ở mỗi báo cáo sức khỏe, tính số của từng ngày theo
  giờ địa phương của đèn (múi giờ hệ thống, UTC khi không đọc được) từ chênh lệch giữa hai lần đọc, giữ trong trạng thái của báo cáo, và chỉ
  gửi ngày đã kết thúc, ngày cũ nhất chưa được xác nhận; ngày hôm nay không
  bao giờ được gửi, nên hai báo cáo cách nhau 15 phút không để lộ một lần
  chạy giữa chúng. Số kỹ năng đang bật và đang tắt chỉ đi kèm ngày đó, theo
  lần đọc cuối của ngày, nên cũng không lộ một kỹ năng vừa tạo, bật hay tắt
  giữa hai báo cáo. Khi đồng hồ chưa được đặt (`clock_synced: false`) thì
  không đếm gì. Một ngày đã được xác nhận không bao giờ được đếm thêm:
  đổi múi giờ về phía tây qua nửa đêm thì số đếm được dồn sang ngày sau.
- **Khi người dùng nói không** với báo cáo, `lamp-cloud` nhờ trình quản lý
  (`{"forget_counts": {}}`, của root, sở hữu cả hai tệp) xóa `runs.json` và
  `counts.json`, mỗi tệp dưới khóa của bên ghi nó (`usage::forget`,
  `made_counts::forget`); hỏi lại mỗi 10 giây cho đến khi xong. Số đếm còn
  trong bộ nhớ chưa ghi (tối đa một phút) bắt đầu tệp mới. Kiểm thử:
  `package::tests::forget_counts_deletes_both_counts`.


## Thêm kỹ năng từ một tệp (`LAMP_SKILL_UPLOAD=on`, bật trong sản phẩm)

Chủ đèn chọn một tệp trên điện thoại (người xây dựng đầu tiên duyệt trên
Queue, 3/11 14:33, theo quy tắc của Backend3; `crate::upload_skill`): một
gói như `lamp-skills pack` tạo ra, hoặc một `SKILL.md` trần. Ứng dụng tải
nó lên backend, backend gửi `skills.upload.<upload_id>` (16 ký tự hex đầu
của SHA-256 của tệp) không kèm tệp; `lamp-cloud` trả lời `starting` ngay
(backend giữ tệp), lấy `GET <base>/devices/get-message?kind=skills.upload.<upload_id>`
bằng khóa của đèn, trong đó có chính tệp (`{upload_id, name, version,
format, sha256, size, content_base64}`), giải mã tối đa 1 MiB, kiểm tra
kích thước, SHA-256 và `upload_id`; rồi gửi `success` khi một tiếng "có"
đã bật nó, hoặc `failure` khi một bước kiểm tra không qua hay nó bị để tắt,
làm câu trả lời thứ hai để backend xóa tệp ([cloud.md](../cloud.md), "A
skill added from a file"). `lamp-cloud` từ chối
gói mang chữ ký của Store ("add it from the Skill Store"), gói có `run.py`
hay tệp nào khác ngoài `skill.toml` và `SKILL.md`, và tên đã có trên đèn.
Sau đó, qua socket app-make (`{"upload": {"skill_md", "skill_toml"?}}`, một
dòng tối đa 64 KiB, chỉ nhận khi `lamp-voice` bật công tắc và có dịch vụ
say để nói ra; và `{"added": {"sha256"}}`, trạng thái `kept` (đang bật),
`on`, `left_off` (đã giữ mà không bật được; hoặc đã hỏi trước #5977) hay
`none`, mà `lamp-cloud` hỏi mỗi 5 giây, tối đa 2 giờ). Cùng
một lần tải lên gửi lại khi nó còn đang xử lý sẽ nhập vào lần đang chạy
(trả lời `starting`, một kết quả cho cả hai), không bao giờ là `BUSY`; và
những lần đang xử lý được giữ trong thư mục trạng thái của `lamp-cloud`
(chỉ id, kind, SHA-256 và trạng thái, không bao giờ tên hay lời của kỹ
năng), nên sau khi khởi động lại nó được hỏi tiếp và ứng dụng vẫn được báo:

1. **Tên đèn đang giữ bị từ chối, không bao giờ bị thay**: kỹ năng có tên
   đó trên đèn (lõi, từ Store, tự tạo), được đẩy từ máy tính, hay đang giữ
   ở đây, tự tạo hoặc thêm từ tệp (`upload_skill::taken`).
2. **Một `SKILL.md` trần** thành một kỹ năng viết (`upload_skill::written`):
   không có trigger, `tool` theo tên kỹ năng (`-` thành `_`), chỉ có quyền
   `speak`, và một bước `say` là phần chữ sau front matter trên một dòng
   (tối đa 900 byte, giới hạn lời của dịch vụ say, nên lời dài hơn bị từ chối ngay thay vì được bật rồi lỗi ở mọi lần gọi; file có xuống dòng kiểu Windows được đọc như mọi file khác): lời dặn cho mô hình khi người ta nhờ đến nó.
3. **Giới hạn** (`made_skill::capped_added`): như kỹ năng tự tạo, khác hai
   điểm: không bao giờ tự chạy (trigger, `[[while]]`, `[ask]` bị từ chối),
   phải có hàm (`tool`); và được `speak`, vì lời của hàm là câu trả lời của
   chính mô hình. Ngoài ra chỉ đèn, đầu, `voice/state` và `room/presence`.
4. **Nói rõ** bằng `made_skill::plainly`: "Only when you ask for it, it
   answers in its own words, as its file tells it, and never on its own."
5. **Kiểm tra và giữ như kỹ năng tự tạo** (`made_skill::keep_added`): thẻ
   được đánh dấu `made: here` và `added: file`, mô tả là lời của
   `plainly`, nhãn "Added from a file, not from the Skills Store" và lúc
   nên dùng theo lời của tệp; `lamp-skills pack` kiểm tra và đóng gói; giữ
   cạnh các kỹ năng tự tạo với `"added": "file"`, tối đa năm (tự tạo hoặc
   thêm). Trên trang kỹ năng (`skills.list`) nó thuộc nhóm tạo trên đèn, có
   `"uploaded": true`, không bao giờ là của Store.
6. **Bật ngay, không cần lời "có" nói thành tiếng** (#5977; chủ đèn,
   10/11): sau khi socket trả lời `kept`, nó được bật qua helper quản lý
   (`made_skill::put_on`), rồi được nói qua dịch vụ say: "I added NAME from
   a file, not from the Skills Store, and it's on: WHAT IT DOES, using WHAT IT USES. Say \"take it off\" to take it off."
   (`upload_skill::on_line`), hoặc "I added a skill from a file but
   couldn't put it on: <lý do>." (`upload_skill::not_on_line`).
7. **Helper quản lý kiểm tra lại** (`install_made`): thẻ có `added: file`
   theo `capped_added`, và không bao giờ thay chỗ kỹ năng nào, kể cả kỹ
   năng tự tạo.

Không bao giờ ghi nội dung tệp vào journal.

Kiểm thử: `upload_skill::tests`, `upload::tests`,
`online::tests::a_skill_from_a_file_is_acked_starting_and_its_result_follows`
và `made::a_skill_added_from_a_file_is_checked_and_never_takes_a_place`.
