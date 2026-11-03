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

1. **Hai hàm của riêng giọng nói.** `make_skill(request)` và
   `put_on_made_skill` là của giọng nói, khai báo một lần và đi tới
   `Route::MakeLocal`, có hay không có phần tasks; phần tasks không bao giờ
   khai báo chúng, và kỹ năng không bao giờ được giao cho máy tính.
   `make_skill` tạo ngay, không đọc lại: mô hình nói vài chữ rằng nó đang
   tạo, và người dùng chờ. Công tắc chỉ được đưa cho worker của tuyến thứ
   hai (`process.rs`), worker này giữ cấu hình chat như khi nó khởi động,
   kể cả khóa riêng của người dùng (`make_skill_local::set_config`); tuyến
   Live không bao giờ có nó.
2. **Bản hướng dẫn** (`made_skill::brief`): chữ cố định, gồm định dạng kỹ
   năng và những gì một kỹ năng tạo ra được phép dùng, rồi đến yêu cầu của
   người dùng.
   - Yêu cầu tối đa 180 byte; dài hơn thì bị từ chối, không bao giờ bị cắt,
     và khi đó không hỏi mô hình gì cả.
   - Không có gì khác của cuộc trò chuyện đi kèm.
3. **Mô hình của đèn viết.** Bản hướng dẫn được gửi tới chat của tuyến thứ
   hai (`llm_*`, relay hoặc DeepSeek riêng của người dùng), không có hàm,
   dạng stream, trên luồng và runtime riêng, tối đa 30 giây
   (`WRITE_WITHIN`) và 20 KiB. Lỗi 404 (của relay, khoảng một phần mười
   ngày 2/11), 5xx, quá giờ, mất kết nối hoặc câu trả lời bị cắt trước khi
   khối cuối đóng thì được hỏi lại một lần (`make_skill_local::again`); lỗi
   khác thì không. Mô hình được xem một kỹ năng hoàn chỉnh mà bước kiểm tra
   chấp nhận (`make_skill_local::EXAMPLE`) và được dặn rằng kỹ năng không
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
   Mở rộng giới hạn này là quyết định của trưởng nhóm.
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
     (hoặc của phần nói rõ bên dưới) được sửa; sai định dạng hay xin dùng
     nhiều hơn đèn và đầu thì được nói là không tạo được ngay.
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
     sửa một lần như lỗi của bước kiểm tra. Trên lamp-0c89, mô hình của đèn
     đã giữ "when someone says good night, dim the light slowly over a
     minute" thành bất kỳ phút yên lặng nào từ 22:00 đến 22:30, dưới mô tả
     của chính nó về điều được yêu cầu.
   - **Điều không kỹ năng nào làm được.** Mô hình được dặn rằng khi yêu cầu
     cần nghe lời nói hay âm thanh, hoặc bất cứ điều gì kỹ năng tạo ra không
     cảm nhận hay làm được, thì trả lời `cannot:` cùng lý do, không bao giờ
     thay bằng thứ khác, và không thêm giờ trong ngày mà yêu cầu không nêu.
     Câu trả lời như vậy, không có khối nào, là trọn vẹn (không hỏi lại như
     câu bị cắt) và được nói là không tạo được, "a skill can't do that: "
     cùng lý do của mô hình (tối đa 120 ký tự, `made_skill::CANNOT`); không
     giữ gì. Khi một phần của yêu cầu không làm được ("dim slowly over a
     minute"), mô hình làm phần còn lại và thêm một dòng ngoài các khối bắt
     đầu bằng `left out:` (`made_skill::LEFT_OUT`); đèn nói dòng đó sau câu
     kỹ năng làm gì, trong một khung cố định như ghi chú của người tạo ("The
     maker notes what it couldn't do: …", một mệnh đề, bỏ dấu ngoặc kép, tối
     đa 120 ký tự), nên lời "có" cũng là đồng ý với phần bị bỏ (#5041). Hai
     dấu hiệu `cannot:` và `left out:` được đọc không phân biệt hoa thường.
6. **Giữ lại, không cài.** Gói nằm trong thư mục riêng của giọng nói,
   `/var/lib/lampOS/voice/made-skills` (0700, người dùng của giọng nói),
   được ghi là tạo từ `local:<ms>`:
   - giữ tối đa 5, cái cũ nhất bị bỏ cùng gói của nó;
   - mỗi cái được giữ cùng SHA-256 và những gì nó xin dùng (`made.json`);
   - cùng một `local:<ms>` được giữ lại thì trả về cái đã giữ, không kiểm
     tra lại.
   Một câu trả lời không qua thì được nói là không tạo được ("the skill
   couldn't be made: …"), không có từ kỹ thuật, và không giữ gì. Các tệp
   không bao giờ được đọc ra.
7. **Nói ra, và hỏi.** Khi đã giữ, mô hình được báo tên, kỹ năng làm gì
   (được nói rõ như trên) và sẽ dùng gì ("đèn của nó và việc nó có đang nói
   hay không"), và được dặn hỏi đúng từng chữ "Shall I put it on the lamp?"
   trong cùng câu trả lời. Lúc đó nó chưa được ghi là đã hỏi (P2 của
   Reviewer-lead trên #4914): chỉ khi một lời "có" đến ngay sau một câu trả
   lời của đèn có nói câu hỏi đó, trong vòng 120 giây, `put_on_made_skill`
   mới ghi nó là đã hỏi (`made_skill::asked_in_answer`, quy tắc của #4880)
   rồi cài đúng SHA-256 đó. Nó được ghi giờ lúc có lời "có", không phải lúc
   câu trả lời bắt đầu (`make_skill_local::asked_at_yes`). Một lời "có" sau
   khi bị nói chen hoặc sau một câu hỏi khác thì không cài gì.
   - Một kỹ năng mà lời hỏi bị cắt là "chưa kể". Khi chỉ có một kỹ năng như
     vậy, giữ trong `UNTOLD_WITHIN` (10 phút), tuyến thứ hai nhắc mô hình,
     và mô hình nói nó làm gì rồi hỏi (`make_skill_local::untold_note`).
   - Khi một kỹ năng đã được hỏi, lời "có" chỉ cài nó khi đèn chưa nói gì
     từ lúc hỏi, hoặc câu trả lời cuối của đèn chính là câu hỏi: lời "có"
     cho một câu hỏi khác ("For ten minutes?") không cài gì
     (`make_skill_local::yes_is_to_the_ask`, P2 của Reviewer-lead trên
     #4971), và mô hình được dặn hỏi lại đúng từng chữ. Câu hỏi được nhận dù
     viết hoa hay nằm giữa câu (`made_skill::says_the_ask`).
8. **Cài khi người dùng nói "có" thành tiếng** (quyết định của trưởng nhóm,
   20/10 08:37 trên #1711: lời đồng ý bằng giọng nói gắn với SHA-256, một
   ngoại lệ của quy định 18:33 chỉ cho kỹ năng tạo trên đèn; ứng dụng không
   phải xác nhận thêm). Khi người dùng đồng ý, mô hình gọi
   `put_on_made_skill`:
   - chỉ lời "có" nói thành tiếng mới tính; gõ chữ hay câu do đèn tự viết
     thì không cài gì;
   - chỉ một lời "có" rõ ràng (`known_faces::plain_agreement`): "No, not
     yet" mà mô hình hiểu nhầm là "có" thì không cài gì;
   - chỉ lời hỏi mới nhất, trong vòng 120 giây;
   - giọng nói chỉ gửi cho helper quản lý SHA-256 của kỹ năng đó
     (`{"install_made": SHA256}`), helper chỉ cài đúng các byte đó và kiểm
     lại giới hạn;
   - helper cũng tự kiểm tra lời hỏi theo đồng hồ của chính nó
     (`made_skill::waiting`): chỉ kỹ năng được hỏi gần nhất, trong vòng 120
     giây và chưa được cài, mới được cài, nên một SHA-256 đã lưu không bao
     giờ được cài về sau bằng bất kỳ đường nào; lời hỏi mới nhất được xét
     trước: khi nó đã được cài hoặc đồng hồ lùi về trước nó thì không có gì
     chờ, và một lời hỏi cũ hơn không bao giờ thế chỗ;
   - trên tuyến thứ hai, khi một kỹ năng đang chờ, mỗi yêu cầu được nhắc
     (`made_skill::waiting_note`);
   - không có helper thì người dùng nghe là không cài được; helper không
     trả lời trong 8 giây thì nghe là đang cài;
   - không bao giờ chạy khi chưa bật `LAMP_MAKE_SKILL`: khi tắt, hai hàm đó
     không thuộc về ai.
9. **Thời gian chờ** (P3 của Reviewer-lead trên #4914): lời gọi giữ lượt tối
   đa 30 giây một lần hỏi; với một lần thử lại sau lỗi và một lần sửa sau khi
   bị từ chối, nhiều nhất khoảng 120 giây.

Gỡ một kỹ năng tạo ra bằng giọng nói sẽ đến sau.

Kiểm thử: `make_skill_local::tests` (kể cả
`with_no_tasks_part_a_skill_is_made_and_put_on_by_voice`: trên đèn không có
phần tasks, kỹ năng được tạo, được hỏi và được cài bằng một lời "có" nói
thành tiếng), `made_skill::tests`, và `crates/skills/tests/it/made.rs` với
`lamp-skills` thật.

## Gõ trong ứng dụng (#2813, `LAMP_APP_MAKE_SKILL=on`, bật trong sản phẩm)

Cùng người tạo đó, từ chữ gõ trong ứng dụng; chỉ được cài như trên, bằng một
lời "có" nói ở đèn. Cần `LAMP_MAKE_SKILL=on` và dịch vụ nói (`LAMP_SAY=on`) để
hỏi thành tiếng; thiếu một trong hai thì socket của ứng dụng không chạy
(`crate::app_make`). Không hỏi phần tasks nào, và không cần phần tasks.

- `{"make": {"words", "request_id"}}`: chữ thành cùng bản hướng dẫn (tối đa
  180 byte) và được mô hình của đèn tạo ngay trên một luồng riêng
  (`make_skill_local::make_kept`, `app_make::make_here`), được ghi là tạo từ
  `local:app:<ms>:1`: `making`, rồi `kept`, hoặc `not_kept` kèm lý do.
- `{"ready": {}}`: `{"ready": true}` ngay, vì đèn tự tạo; `lamp-cloud` ghi
  `skills.make` vào `can` chỉ cần có công tắc, nên nút "Make a skill" của ứng
  dụng hiện ngay.
- Khi đã giữ, đèn nói thành tiếng qua dịch vụ nói, vào lúc thích hợp trong
  vòng một giờ: "I made the skill you typed in the app." cùng việc nó làm,
  rồi đúng câu "Shall I put it on the lamp?". Chỉ một lời "có" ngay sau câu
  đó mới cài kỹ năng; không gì từ ứng dụng cài được nó.
- `{"made": {"request_id"}}`: trạng thái của lời nhờ từ ứng dụng: `making`,
  `kept`, `asked`, `on`, `left_off` hoặc `not_kept` kèm `why` (lời rõ ràng
  của người tạo).

Kiểm thử: `app_make::tests`, kể cả trên đèn không có phần tasks: lời nhờ gõ
được tạo, được giữ, được hỏi thành tiếng và được cài bằng lời "có" nói ngay
sau câu hỏi đó.

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
  SHA-256 và lời đồng ý bao gồm nó; dòng `made` hay `made_by` do mô hình viết
  bị bỏ. Gói chia sẻ được bỏ dấu này cùng `made: here`.
- **Lời nhờ tạo kỹ năng, bằng giọng nói và từ ứng dụng, trong
  `lamp-voice`** (`made_counts`). Socket của ứng dụng nhận thêm
  `{"counts": {}}` và trả `{"made_by_voice": {"kept", "on", "refused",
  "left_off"}, "made_by_app": {...}}`, là tổng cộng dồn theo người tạo.
  `kept`, `on` và `left_off` đọc từ chỉ mục kỹ năng tự tạo, mỗi kỹ năng đếm
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
  chạy giữa chúng.
- **Khi người dùng nói không** với báo cáo, `lamp-cloud` nhờ trình quản lý
  (`{"forget_counts": {}}`, của root, sở hữu cả hai tệp) xóa `runs.json` và
  `counts.json`, mỗi tệp dưới khóa của bên ghi nó (`usage::forget`,
  `made_counts::forget`); hỏi lại mỗi 10 giây cho đến khi xong. Số đếm còn
  trong bộ nhớ chưa ghi (tối đa một phút) bắt đầu tệp mới. Kiểm thử:
  `package::tests::forget_counts_deletes_both_counts`.
