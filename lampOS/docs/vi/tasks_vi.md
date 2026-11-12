# Giao việc: kỹ năng tạo bằng giọng nói

Trang này chỉ dịch phần mới về việc giao một kỹ năng để tạo. Toàn bộ tài
liệu giao việc là [tasks.md](../tasks.md), bằng tiếng Anh; nếu hai trang khác
nhau, bản tiếng Anh đúng.

## Kỹ năng tạo bằng giọng nói (#1685): không qua phần tasks

Một kỹ năng được tạo bởi chính mô hình của đèn, không bao giờ giao cho tác tử
trên máy tính (người xây dựng đầu tiên, 3/11, trên Queue;
[skills_vi.md](skills_vi.md)). Phần tasks không khai báo `make_skill` hay
`put_on_made_skill`, và cờ `--tasks-make-skill` của worker đã bị bỏ: chỉ cần
`LAMP_MAKE_SKILL=on` là giọng nói có hàm của riêng nó, `make_skill`
(`put_on_made_skill` đã bị bỏ từ #5977: kỹ năng đã tạo được bật ngay khi được
giữ lại), có hay không có phần
tasks, và một kỹ năng gõ trong ứng dụng (`LAMP_APP_MAKE_SKILL`) cũng được tạo
như vậy, không bao giờ được giao đi. Các câu tin của kỹ năng đã tạo (`made`,
`not_kept`) và việc sửa nó qua một việc giao cũng bị bỏ theo. Một việc mà lời
của nó là bản hướng dẫn của một kỹ năng, do một bản phát hành trước giao đi,
không bao giờ được kể: câu trả lời của nó là các tệp.

- **Khi đặt lại đèn,** `/var/lib/lampOS/voice/made-skills` bị xóa cùng các kho
  khác của người dùng.

### Lời nhờ có tên là một lần giao việc (tắt trừ khi `LAMP_TASKS_NAMED_ASK=on`, #1722)

Trên lamp-4ace ngày 2026-10-21, khi đã đọc được hai người trong đội (Claude
Code và Codex, #2297), những lời nhờ được phát như "Hey Lamp, ask Claude Code to
write a two-line poem about the rain." lại đi vào `not_for_me`, hoặc đèn tự trả
lời ("poetry's mine to write, no need to ask anyone"): mỗi lượt chạy chỉ 0 đến 3
trên 3. Người dùng đã nêu tên ai sẽ làm, vậy mà đèn vẫn từ chối.

Với `LAMP_TASKS_NAMED_ASK=on` (voice chỉ chuyển nó cho worker của tuyến thứ
hai, qua `second_route_worker_environment`, và worker đó đọc nó; khi phần tasks
bật): lượt
chat đầu tiên của một câu nói có tin nhắn cuối là lời người dùng, và lời đó nhờ
đích danh một người trong đội làm việc gì, sẽ được gửi kèm
`tool_choice: {"type": "function", "function": {"name": "start_agent_task"}}`
(`agent_tasks::named_ask`, `second_provider::worker::named_asks`,
`net::Chat::forcing`). Mô hình vẫn tự viết nội dung việc, nhưng không thể từ
chối giao việc hay tự làm. Lượt sau một lần gọi kết thúc bằng kết quả
của lần gọi đó, và được hỏi như trước.

- **Là lời nhờ:** "ask", "tell", "get" hoặc "have" đứng ngay trước tên đầy đủ
  của agent, theo sau là "to" ("ask Claude Code to …", "tell Codex to …"), có
  thể có một nơi chốn trước "to" ("ask Claude Code on my office computer to
  …": chữ ngay sau tên là "on", "at", "in" hoặc "from", và "to" nằm trong 6
  chữ sau nó), hoặc "can", "could", "would", "will" + "you ask/get <tên> …".
- **Theo cách speech to text nghe** (b0934 trên lamp-4ace, 2026-10-21): tên viết
  hoa hay thường đều được, và cả khi bị nghe sai (`Kind::heard` cạnh `KINDS`):
  Claude Code thành "cloud code", "Claude Coat", "Claude Cold", "clawed code"
  hoặc "ClaudeCode"; Codex thành "code X", "codecs" hoặc "Kodex". Một dạng nghe
  sai chỉ được tính với "ask" hoặc "tell": "get" và "have" chỉ dành cho tên
  thật ("Claude Code", "ClaudeCode", "Codex"), vì "I can't get codecs to work"
  là câu tiếng Anh bình thường. "ask <tên> …" không có "to" ở chỗ bắt đầu một
  câu ("Ask Claude Code write a poem.", "…. Ask Codex, write a haiku."), trừ
  khi ngay sau tên là "?" hoặc "." ("Ask Claude Code? No, never mind." không
  phải lời nhờ; dấu phẩy không tính là dấu dừng). "asked", "sk" hoặc "k" thay
  cho "ask", chỉ khi là chữ đầu tiên của câu nói, trước nó chỉ có "Hey Lamp,"
  hay tương tự ("Asked Claude Code to …", "sk Claude Code to …"); "k" chỉ khi
  có "to" sau tên, vì nó cũng là "OK" ("K, Claude Code finished?" không phải
  lời nhờ). Sau một chủ ngữ thì đó là kể lại,
  không phải lời nhờ ("I asked Claude Code to do it yesterday, is it done?").
  Một câu mở đầu bằng "Asked Claude Code to …" được coi là lời nhờ, kể cả khi
  người dùng muốn kể lại.
- **Không phải lời nhờ:** khi chỉ nhắc đến ("I told Claude Code yesterday",
  "is Codex done?", "what did Claude Code say?", "did you ask Codex to …",
  "tell Claude Code I'm late", "Claude Code's answer", "get Codex's answer",
  "have Claude Code's PR reviewed", "I'll ask Claude Code later"); một lời phủ
  định ngay trước động từ ("don't ask Claude Code to …", "do not tell …",
  "never ask Codex to …", "not get …"); một câu người dùng tự hỏi mình ("should
  I ask Claude Code to …", "shall I …", "do I …"); khi tên không thuộc đội ("ask
  Devin to …" lúc Devin chưa được cài); một dạng nghe sai mà không có chữ nhờ
  ("the cloud code is broken", "a cold coat"); khi tên bị cắt cụt ("Claude Co"; ghi chú
  của đội vẫn nhận ra tên này). Khi chưa đọc được ai trong đội, chỉ nhận
  Claude Code.
- **Đánh đổi:** các quy tắc có thể cho qua một câu nói với người khác (trong
  phòng yên tĩnh mà không có `LAMP_SPOKEN_TO`, hoặc như một cuộc trò chuyện
  đang diễn ra). Hiện nay mô hình vẫn có thể nói câu đó không dành cho đèn
  (`not_for_me`). Khi câu đó nêu tên một agent ("…, ask Claude Code to fix the
  build"), việc ép gọi lấy mất lựa chọn đó: nó thành một lần giao việc được
  chuẩn bị sẵn và một câu hỏi cho cả phòng. Tiếng "có" nghe được vẫn canh lần
  bắt đầu: `start_agent_task` chỉ chuẩn bị, và không có gì chạy nếu không có
  một tiếng "có" nghe được sau đó trong vòng 120 giây.
- **Khi tắt:** yêu cầu gửi đi giống hệt như trước, từng chữ (có test so sánh
  nội dung). `tool_choice` không bao giờ được gửi kèm các hàm có tên của một
  câu bị giữ lại.
- **Relay:** đã kiểm tra trên lamp-06c1 ngày 2026-10-21 lúc 03:4x: với câu
  "What's two plus two?", chat DeepSeek qua relay (tắt chế độ suy nghĩ) gọi
  đúng hàm mà `tool_choice` chỉ định 3 trên 3 lần, và 0 trên 3 lần khi không có
  `tool_choice`.
- **Ai làm** (lượt thử phát b0701 trên lamp-4ace, #1722, 2026-10-21 07:19):
  khi bị ép gọi, câu "Hey Lamp, can you ask Claude Code on my office computer
  to write a short story …" lại đi đến Codex. Lời nói có khớp (Claude Code),
  nhưng lệnh ép chỉ nêu tên hàm, nên `who` là do mô hình chọn. Giờ đây, trong
  lượt bị ép, `start_agent_task` chỉ được cho biết agent mà lời nói nêu tên:
  `enum` của `who` chỉ còn đúng giá trị đó, và `who` là bắt buộc
  (`net::Forced`). Một lần gọi ở lượt 1 có `who` là agent khác, hoặc bỏ trống,
  sẽ được giao cho agent được nêu tên trước khi phần tasks chuẩn bị nó
  (`second_provider::Engine::asking_by_name`).
- **Nhờ nhiều người** (người review trên #2410: câu "Hey Lamp, ask Codex to
  check the build and ask Claude Code to write a poem." giao cả hai lần gọi cho
  Claude Code): `named_ask` trả về mọi agent trong đội mà lời nói nhờ, mỗi
  agent một lần, theo thứ tự lời nói nhờ họ. Khi có từ hai agent trở lên, lượt
  1 vẫn bị ép gọi `start_agent_task`, nhưng `who` được cho biết như đã khai
  báo, không ghim giá trị nào. Mỗi lần gọi ở lượt 1 có `who` là một trong các
  agent được nhờ thì giữ nguyên. Một lần gọi bỏ trống `who`, hoặc nêu một agent
  mà lời nói không nhờ, được giao cho agent được nhờ đầu tiên, theo thứ tự lời
  nói, mà chưa lần gọi nào ở lượt 1 nêu tên, nếu không thì cho agent được nhờ
  đầu tiên (`second_provider::given_to_the_named`). Vậy hai việc trong một câu,
  một việc nêu Codex và một việc bỏ trống, mỗi agent nhận một việc, chứ không
  phải cả hai cùng về agent được nêu đầu tiên.
- **Nhật ký**, mỗi câu một dòng, không có lời nói nào: `named ask: matched
  <agent>` (nhiều agent thì theo thứ tự lời nói: `matched Codex, Claude Code`),
  kèm, với mỗi agent được giao một lần gọi, `; the call named another agent,
  given to <agent>` hoặc `; the call named nobody, given to <agent>`; `named ask: no
  match (<n> words)`; `named ask: matched <agent>, not applied: round 1 called
  <các hàm>, not start_agent_task` (hoặc `answered in words`); và với một câu
  bị giữ lại, `named ask: matched <agent>, not applied: the line is held`.
- **Nói đè lên giọng đèn** (b0701, request 58: "Hey Lamp, ask Codex to write a
  haiku about coffee.", nói khi đèn đang nói, nghe thấy một cụm từ dừng, rồi
  `not_for_me`): mọi câu mà tuyến thứ hai trả lời đều bị ép theo cùng một cách.
  Khi nói đè lên giọng đèn, sau một lần dừng, hay như một cuộc trò chuyện đang
  diễn ra, các ghi chú đứng trước lời người dùng, và lời người dùng vẫn ở cuối.
  Lời của câu đó có khớp (Codex), và relay giữ `tool_choice` (ở trên). Vậy rất
  có thể request 58 đã được hỏi bằng lời khác. Một cụm từ dừng nghe được khi
  đèn đang nói sẽ bỏ câu đang được nghe cho đến hết câu (`coordinator/hand.rs`,
  `cancel`, rồi `InputAdmission::abandon_until_end`), nên request kế tiếp có
  thể chỉ là phần còn lại của lời đã nói. Giờ dòng nhật ký cho thấy là trường
  hợp nào: `no match` kèm số từ, hoặc `matched` và lượt 1 đã làm gì.

**Probe** (`cargo run -p lamp-voice --example probe_handover`, chạy ở nơi voice
chạy, bằng quyền root, với các công tắc tasks của unit voice): mỗi lời nhờ đã
phát được gửi như lượt đầu của một câu nói, kèm đúng system message, ghi chú
của đội, ghi chú gọi tên, các hàm và client chat mà worker thứ hai dựng; hai
người trong đội được giữ sẵn như cách phần tasks sẽ trả lời. Với
`PROBE_HANDOVER_HISTORY=on`, mỗi lượt chạy trước hết nhờ "tell me a story about
a clever fox, at least eight sentences" và lưu câu trả lời thật vào thread như
một lượt trao đổi. Probe chỉ in số đếm. Trên lamp-06c1 ngày 2026-10-21, mỗi câu
10 lần:

| Cách viết | Kể chuyện trước | Thơ mưa | Thơ biển | Haiku (Codex) |
|---|---|---|---|---|
| A (ghi chú hiện tại) | không | 10/10 | 10/10 | 10/10 |
| B (#2337, đã revert) | không | 10/10 | 9/10 (1 tự làm) | 10/10 |
| C (quy tắc lên đầu) | không | 10/10 | 10/10 | 10/10 |
| A | có | 10/10 | 10/10 | 10/10 |
| B | có | 10/10 | 10/10 | 9/10 (1 tự làm) |
| C | có | 10/10 | 10/10 | 10/10 |
| D (A, `LAMP_TASKS_NAMED_ASK=on`) | có | 10/10 | 10/10 | 10/10 |

Như vậy probe chưa tái hiện được kết quả 0 trên 3 của lamp-4ace: những gì lượt
chạy thật gửi thêm (ghi chú của bộ nhớ, các dòng của skill, các hàm tìm kiếm
web, tên vừa nghe thấy) không có trong probe. D giúp việc giao việc không còn
phụ thuộc vào những thứ đó. Chưa được phát thử trên đèn.

### Một tiếng "không" cho lời đọc lại bỏ việc đã chuẩn bị (#6582)

Trước đây (#2838), sau khi đèn đọc lại một việc, một tiếng "không" đơn thuần
không gọi gì cả: việc đã chuẩn bị vẫn chờ, trong giọng nói (`PENDING`) và trong
phần tasks, suốt `CONFIRM_WITHIN` (120 s), nên một tiếng "có" sau đó vẫn bắt
đầu được nó. Nay, trên tuyến thứ hai, một câu nghe được là một tiếng không đơn
thuần (`services::plain_no`, danh sách giọng nói đã dùng cho tiếng không với
một lần ghi của kỹ năng: "No.", "No thanks.", "Never mind.", "Don't.",
"Cancel that.", "Không, thôi.", và "No, don't start it.": "start", "begin",
"do" và "run" được tính, như "send", chỉ khi đứng sau "don't", phát hiện trên
lamp-c0a4 ở 12431, nơi "No, don't start it." không được coi là không) trong lúc một việc đang chờ, từ một yêu cầu
có thể nói có với nó (`may_confirm`), bỏ việc đó (`agent_tasks::declined`,
gắn vào engine bằng `declining_a_task_waiting`):

- `PENDING` được xoá, nên không tiếng "có" nào sau đó trong 120 s bắt đầu nó;
- phần tasks được gửi lệnh `cancel` cho việc đó, cùng lệnh mà một lời sửa gửi,
  sau một `status` để lấy guard, trên một luồng riêng giới hạn bởi `DEADLINE`
  (3,5 s): một lệnh huỷ bị từ chối hoặc không được trả lời không bao giờ giữ
  giọng nói lại (nhật ký ghi `could not be cancelled; this voice no longer
  confirms it, and it lapses in the tasks part`);
- nhật ký ghi `lamp-voice: tasks: request N: task <id> dropped: the person
  said no to its read-back`;
- câu đó được báo, thay cho ghi chú việc đang chờ, rằng người ta đã nói không,
  việc đã bị bỏ và chưa có gì bắt đầu: chỉ nói là sẽ không bắt đầu nó, không
  gọi gì.

Một tiếng không có thêm nội dung ("no, write a poem about tea instead", "no,
I said tea", "no, ask Codex") không phải tiếng không đơn thuần: nó vẫn là một
lời sửa như trước (#2838). Một tiếng không khi không có việc nào chờ, trong
chính yêu cầu đã chuẩn bị việc, hoặc sau 120 s, không làm gì. Tuyến Live chưa
có chỗ gắn này: ở đó một tiếng không vẫn không gọi gì. Test:
`a_plain_no_drops_the_task_and_a_failed_cancel_keeps_the_correction`,
`a_plain_no_then_a_yes_within_the_window_starts_nothing`,
`a_correction_after_the_read_back_is_not_a_plain_no`,
`a_plain_no_with_nothing_pending_does_nothing`.

## Việc của một kỹ năng trên Hermes của đèn (#6306, PR 3 của #6071, tắt trừ khi `LAMP_SKILL_TASKS=on`, bật trong sản phẩm)

Bước `do.task` của một kỹ năng (#6305, docs/skills.md, "A skill's task")
giao phần chậm hơn cho Hermes của chính đèn. lamp-skills gửi một dòng cho mỗi
lần chạy, và được trả lời khi lần chạy kết thúc
(`lamp_tasks::skill_task::answer`):

- **Dòng gửi:** `{"op":"skill_task","skill","task","run","folder","sha256","grants"}`.
  Cần đủ mọi trường và không nhận trường nào khác. `skill` là tên một kỹ năng
  (`a-z`, `0-9`, `-`, `_`, tối đa 64), `folder` đúng là
  `/var/lib/lampOS/skills/<skill>` (các kỹ năng Hermes thấy ở chế độ chỉ đọc)
  hoặc, với kỹ năng do máy tính của chủ đèn đẩy lên,
  `/var/lib/lampOS/skills-dev/<skill>` (`skill_task::PUSHED`, chỉ lần chạy
  của kỹ năng mới thấy, chỉ đọc), so đúng như viết (`..` hay thư mục khác bị
  từ chối "the skill's folder is not its own among the lamp's skills"); thư
  mục được đẩy lên chỉ được nhận khi trong `skills/` không có gì mang tên đó
  (thư mục, tệp hay liên kết; `skill_task::lamp_has_skill_named`, cùng quy
  tắc với lamp-control và lamp-skills), nếu không thì bị từ chối "a skill of
  that name is on the lamp: a pushed one never runs in its place",
  `run` là một tệp trong `scripts/` của kỹ năng (mỗi phần chỉ gồm `A-Z a-z
  0-9 . _ -`, không bắt đầu bằng `.`, nên không `..`, không phần ẩn, không
  xuống dòng, dấu backtick hay khoảng trắng,
  không đường dẫn tuyệt đối), `task` dài 1 đến 500 byte, `sha256` là 64 chữ
  số hex viết thường, và `grants` mỗi quyền một lần và là quyền đã biết (`grant_known`):
  một quyền của lamp-control (`CONTROL_GRANTS`: `body`, `light`, `speak`,
  `look`, `hear`, `sound`, `air`, `device`, `camera`, `microphone`, `notes`,
  `speech`; không bao giờ là `skills`), hoặc `<code>-read` hay
  `<code>-write` của một dịch vụ liên kết (#6432, `SERVICE_GRANTS`: mã
  trong danh mục của lamp-services, `gmail-app` tính là `gmail` và
  `figma-api` là `figma`, nên có `gmail-read`, `google_calendar-write`).
  Một test giữ `SERVICE_GRANTS` khớp với danh mục. Mọi thứ khác là
  `bad_request`, và không xin token nào.
- **Câu trả lời:** `{"ok":true,"id":N}` khi lần chạy đã kết thúc, hoặc
  `{"ok":false,"error","message"}`: `off` (công tắc không phải `on`),
  `bad_request`, `too_long` (quá 8192 byte), `no_token` (lamp-control không
  cấp token một lần chạy), `run_failed`, hoặc `busy` (việc của
  một kỹ năng khác đang chạy: mỗi lúc chỉ chạy một việc).
- **Token một lần chạy** (điều kiện 12:45 của người xây dựng đầu tiên trên
  #6071): với mỗi lần chạy, lamp-tasks xin lamp-control (#6304, PR #6408)
  một token trên socket quản trị `/run/lampos-control/admin.sock`
  (`ControlTokens`, trait `Tokens`): `{"mint":{"skill","run","expires_in_ms"}}` (thời hạn 14 phút,
  `RUN_LIFETIME`, dưới mức tối đa 30 phút của lamp-control và ít hơn một phút so với 15 phút lamp-skills chờ câu trả lời; một lần xin token mất câu trả
  lời thì cũng được kết thúc ngay, vì token có thể đang sống), với mã lần
  chạy của riêng lamp-tasks, mới cho mỗi lần giao (`run_id`), và
  `{"end":{"run"}}` khi lần chạy kết thúc. lamp-control tự đọc lời đồng ý
  của chủ đèn và nêu các quyền; lần chạy nào có quyền được cấp không khớp
  đúng các quyền dòng giao việc xin thì bị từ chối `no_token` và token bị
  kết thúc, nên lần chạy không bao giờ nhận quyền mà kỹ năng không xin. Dòng
  giao việc không xin quyền nào và lời đồng ý được giữ không có quyền nào thì
  khớp: token không nêu quyền nào, và lần chạy chạy mà không dùng gì của
  đèn. Lời
  từ chối của chính lamp-control (chưa có đồng ý, tắt chạy kỹ năng, không
  liên lạc được trong 5 s) là thông điệp `no_token`. Lần chạy nhận token qua
  `LAMP_CONTROL_TOKEN`,
  cùng `LAMP_CONTROL_SOCKET=/run/lampos-control/control.sock`, và không thêm
  gì khác vào môi trường. Token kết thúc khi lần chạy kết thúc, kể cả khi lần
  chạy lỗi. Nó không bao giờ có trong lời của việc, nhật ký hay dòng debug.
  Không có token thì không chạy. Trong lúc lần chạy còn sống, token nằm
  trong môi trường tiến trình của wrapper ngoài cùng (`/proc/<pid>/environ`),
  các tiến trình khác cùng uid với lamp-tasks đọc được; điều này được chấp
  nhận cho một token chỉ dùng một lần chạy và sống tối đa 14 phút.
- **File token của lần chạy** (#6510 Part 1): `harness new` không nhận môi
  trường, nên lần chạy đi qua Harness (`HarnessRunner`, #6510 Part 1 bước 2,
  bên dưới) được giao token bằng một file thay cho `LAMP_CONTROL_TOKEN`:
  `/run/lampos-tasks/runs/<run-id>` (`run_token_file::RUNS_DIR`, tmpfs),
  run id phải là id thuần (`a-z`, `0-9`, `-`, 1 đến 64 ký tự), nếu không
  thì bị từ chối trước khi tạo gì. `RunTokenFile::write` tạo thư mục 0700
  khi chưa có và từ chối thư mục là link, không thuộc user của lamp-tasks
  hoặc mở cho người khác; file được tạo 0600, độc quyền (file có sẵn hay
  link ở đó bị từ chối, không bao giờ đi theo hay ghi đè), ghi đủ rồi sync.
  Wrapper của engine (lamp-update) đọc nó một lần rồi xóa; lamp-tasks cũng
  xóa nó khi lần chạy kết thúc dù kết thúc thế nào (drop của guard; đã bị
  xóa trước thì không sao). Token không bao giờ có trong argv, dòng debug,
  nhật ký, lỗi hay trạng thái của Harness; đường dẫn chỉ chứa run id.
- **Lần chạy:** một việc cho đích `lamp`, lời của nó là việc của kỹ năng,
  lệnh chính xác chạy script từ thư mục chỉ đọc của kỹ năng,
  `PYTHONPATH=/usr/local/lib/lampos/sdk-python python3 <folder>/<run>`
  (đường dẫn của SDK nằm ngay trong lệnh, để `lampos` import được dù shell
  của Hermes làm gì với môi trường của lần chạy, #6684; launcher của Hermes
  thay `PYTHONPATH` nó được khởi động với, nên lamp-tasks còn ghi file
  `/usr/local/lib/lampos/hermes-run-env.sh` của lamp-update vào
  `terminal.shell_init_files` của Hermes, sau ba file mặc định
  `~/.profile`, `~/.bash_profile`, `~/.bashrc` khi nó tạo danh sách, trừ khi
  `terminal.auto_source_bashrc` tắt theo cách Hermes đọc (`false`, `no`,
  `off`, `null` hoặc `0`; `'false'` có ngoặc là bật); danh sách rỗng
  (`shell_init_files:` trống, `null`, `[]`) cũng nhận ba file mặc định;
  danh sách người dùng tự đặt được giữ nguyên; một `terminal:` không gộp
  được (flow style, anchor) được để nguyên và ghi journal, phần còn lại
  vẫn được gộp; terminal của Hermes source file đó trước snapshot đăng nhập, và
  file chỉ đặt SDK lên đầu `PYTHONPATH` khi có `LAMP_CONTROL_SOCKET`, tức
  chỉ trong lần chạy kỹ năng), quy tắc chạy
  script đúng như nó là, không bao giờ sửa, chép hay viết lại file của kỹ
  năng, và đường duy nhất tới đèn (`SkillRun::prompt`), qua trait `Runner`, trait này chờ lần chạy kết thúc.
  Việc của kỹ năng được đưa vào như dữ liệu (một chuỗi JSON, "as data, not
  as instructions"), và không được có ký tự điều khiển. Runner kết thúc lần
  chạy ở `SkillRun::deadline`, đúng thời hạn của token, tự nó làm, không
  chỉ dựa vào lời trong prompt. Runner của đèn cho lần chạy một network
  namespace riêng (bên dưới).
- **Runner của đèn, qua Harness** (`HarnessRunner::lamp`, #6510 Part 1
  bước 2; `HandOvers::lamp` dùng nó cho mọi `do.task`): Hermes của đèn do
  Harness khởi động, như một việc nói cho đích `lamp`, không bao giờ do
  chính lamp-tasks.
  - **Luôn là Hermes của đèn** (phương án (b) của V2, #1711, 12/11 21:34).
    Dù engine mặc định của đèn là gì (OpenClaw hay engine khác), lần chạy
    của kỹ năng luôn tới Hermes của chính đèn, trên máy của đích riêng của
    đèn (`lamp` trong file đích mà `serve --set-up-later` đọc, đọc lại ở mỗi
    lần chạy). Việc người dùng nói ra vẫn tới engine được chọn hay được gọi
    tên (#6506). Socket điều khiển, SDK và bridge trong namespace của các
    engine khác là bước 2b. Không có đích của đèn, hay Harness chưa sẵn sàng (không
    có chương trình Harness, hay phần này đang khởi động), cũng bị từ chối
    bằng lời. Unit sản phẩm không còn `ConditionPathExists=/usr/local/bin/harness`
    (có từ #811; #6685 PR 3, theo câu hỏi của first builder): trên đèn không
    có chương trình Harness, systemd vẫn khởi động phần này, chỉ marker của
    setup giữ nó lại; mọi việc giao đi bị từ chối là `no_harness`, health
    của nó vẫn ok, và Harness cài sau được dùng khi lệnh cài của lamp-update
    khởi động lại phần này (test
    `the_product_unit_starts_the_part_with_no_harness_program`). Khi không có đích của đèn, hay Harness không khởi động lần
    chạy (`harness new` không gửi được hoặc không chắc), lamp-tasks đọc
    trạng thái đăng nhập của Harness một lần (`harness auth status
    --json`); nếu chưa đăng nhập, lời từ chối nói lý do bằng lời cho người
    dùng (`skill_task::SIGNED_OUT`, theo yêu cầu 10:44 ngày 13/11 của
    builder đầu tiên, cho tới khi PR 3 của #6685 chạy được mà không cần
    Harness): "This skill's task can't run yet: Harness isn't signed in on
    this lamp. Sign in to Harness from the app to let it run." Lời này được
    trả về lamp-skills với mã `run_failed`, và dòng nhật ký của lamp-skills
    cho `do.task` của kỹ năng mang nó; giọng nói và app chưa nói nó. Khi đã
    đăng nhập, offline hay không đọc được trạng thái, lời vẫn như trước.
    Một `/usr/local/bin/hermes` thiếu các dòng nhận lần chạy
    (`WRAPPER_RUN_LINES`) hay các dòng mạng (`WRAPPER_NET_LINES`) bị từ
    chối `NOT_FOUND_BY_FOLDER` (`LAMP_SKILL_TASKS` của lamp-update tắt hoặc
    vòng của nó chưa chạy): không tạo hay khởi động gì, nên một lần chạy
    không bao giờ bắt đầu mà không có token trên mạng của đèn.
  - **Agent mặc định của đèn, theo một đường chạy duy nhất, với
    `LAMP_SKILL_DEFAULT_AGENT=on`** (#6685, tắt cho tới khi hàng trên c0a4
    đạt và có hai lượt đọc). Một cách chạy kỹ năng cho mọi agent trong
    Harness của đèn: không có code riêng cho từng agent và không có tên
    engine nào trong đường chạy. lamp-tasks đọc engine mặc định của đèn ở
    mỗi lần chạy (`runtime::lamp_default`, lựa chọn trong app trước; trước
    khi Harness báo, engine của đích của đèn) và kiểm tra một lần, như nhau
    cho mọi engine (`skill_task::default_agent_takes_runs`):
    - agent chạy trên cloud của nhà làm ra nó (`EngineKind::cloud`, Devin
      Cloud) không bao giờ chạy trên đèn;
    - wrapper `/usr/local/bin/<engine>` của nó là wrapper đường chạy duy
      nhất mà lamp-update ghi cho nó: `ONE_PATH_MARK` ở dòng thứ hai, phần
      dữ liệu gọi tên nó (`E=<engine>`), các dòng nhận lần chạy
      (`WRAPPER_RUN_LINES`) và các dòng mạng của đường chạy
      (`AGENT_NS_NET_LINES`: `LAMP_CONTROL_TOKEN` và `LAMP_NS_NET` của bên
      gọi bị bỏ, `--net` chỉ trong một lần chạy, một `unshare` duy nhất vào
      `/usr/local/lib/lampos/agent-ns`);
    - namespace agent `/usr/local/lib/lampos/agent-ns` lấy thư mục home từ
      wrapper (`AGENT_NS_HOME_LINE`) và cho thấy socket của lamp-control
      (`AGENT_NS_TASKS_LINE`, `LAMP_SKILL_TASKS` của lamp-update bật);
    - **thứ Harness sẽ khởi động chính là wrapper đó**, hỏi trực tiếp lúc
      bắt đầu lần chạy, không bao giờ trong lượt nói và không bao giờ từ
      ảnh chụp của một vòng (`skill_task::login_finds_wrapper`): Harness
      khởi động thứ mà login shell của `lampos` tìm thấy bằng `command -v
      <engine>`, và chương trình riêng của engine nằm trước trên `PATH` đó
      (`~/.local/bin/<engine>`) sẽ chạy không có namespace và có mạng của
      đèn. lamp-tasks chạy dưới `lampos` (với `NoNewPrivileges`), nên không
      chạy được `runuser -l lampos` của lamp-update; nó chạy đúng lệnh của
      lamp-update (`skill_task::login_command`, `command -v <engine>`, một
      test giữ nó khớp với mã nguồn của lamp-update) trong một login shell
      tự khởi động (`LoginLookup`): `/bin/sh -l -c`, shell mà Harness dùng
      để đọc môi trường khởi động của engine (`hold::LAUNCH_SHELL`; shell
      riêng của `lampos` là `/usr/sbin/nologin`), môi trường được xoá chỉ
      còn `HOME` (home của Harness, chính `/var/lib/lampos-tasks` của
      lamp-tasks, nơi có `~/.local/bin` của trình cài Harness), `SHELL`,
      `USER`/`LOGNAME` `lampos` và `PATH=/usr/local/bin:/usr/bin:/bin`,
      trong nhóm tiến trình riêng, giới hạn 2 giây (`LOGIN_LOOKUP_WITHIN`;
      quá hạn thì cả nhóm bị kill). Dòng duy nhất nó in ra và
      `/usr/local/bin/<engine>` phải là cùng một file sau khi cả hai được
      phân giải như `readlink -f` (`found_is_wrapper`): symlink tới wrapper
      thì được, file khác thì không. Tìm thấy chương trình khác trước,
      không tìm thấy gì, shell lỗi, in ra hơn một dòng, không khởi động
      được hoặc chạy quá 2 giây đều bị từ chối bằng lời, không ghi file
      token và không hỏi gì Harness.

    Cùng các dòng đó cho Hermes, Claude Code hay agent nào khác: một test
    ghim chúng vào các wrapper mà lamp-update ghi cho Hermes và Claude Code
    (`crates/update/src/agent_ns/wrapper/one-path-*.sh`). Mặc định không
    qua được kiểm tra bị từ chối bằng lời, có ghi nhật ký: "the lamp's
    default agent (<engine>) isn't set up for a skill's run yet (<lý do>),
    so nothing was started", không tạo hay hỏi gì, và không agent nào khác
    chạy thay. Một wrapper do lamp-update ghi (có dấu `WRAPPER_MARK` trong
    512 byte đầu) mà không có các dòng nhận lần chạy được báo là agent chưa
    có đường chạy đã rào cho lần chạy kỹ năng, bất kể công tắc của
    lamp-update, và phép kiểm tra không gọi tên engine nào: gateway duy nhất
    của OpenClaw không có wrapper khởi động theo từng lần chạy, nên khi là
    mặc định nó bị từ chối như vậy cho tới khi có đường chạy đã rào của
    riêng nó (một bước sau). File token được ghi
    trước khi khởi động, như với Hermes, và lần chạy là `harness new
    <engine>` trong thư mục riêng của nó (bên dưới). Các thông báo sau khi
    khởi động nói "the lamp's default agent". Tắt: mọi lần chạy là của
    Hermes của đèn qua wrapper riêng của nó, như trên (giai đoạn tạm).
  - **Không có tài khoản Harness: agent của chính đèn, do lamp-tasks tự
    khởi động** (#6685 PR 3, với `LAMP_SKILL_DEFAULT_AGENT=on`). Khách hàng
    không có tài khoản Harness vẫn có việc của kỹ năng: agent của chính đèn
    là sản phẩm. Khi nào dùng (`skill_task::starts_without_harness`): công
    tắc bật, và Harness được đọc là đã đăng xuất qua các lượt xem đăng nhập
    của chính phần này (`harness auth status`, `serve --set-up-later`, giữ
    bởi `skill_task::harness_sign_in_read`; chỉ tính là đăng xuất khi
    Harness đã được thiết lập ở đây và lượt quét lúc khởi động đã xong),
    hoặc không có chương trình Harness nào (sau một lượt quét các thư mục
    và file token mà lần khởi động trước để lại,
    `skill_task::sweep_without_harness`; `ConditionPathExists=/usr/local/bin/harness`
    của unit sản phẩm vẫn giữ lamp-tasks tắt trên đèn như vậy). Đã đăng
    nhập, đăng nhập nhưng đang offline, hay chưa đọc: qua Harness như trên.
    Lúc bắt đầu lần chạy không hỏi gì: trạng thái đăng nhập là lần đọc gần
    nhất, nên việc bắt đầu không chờ mạng và không gọi Harness. Agent mặc
    định vẫn như vậy (`runtime::lamp_default`), nếu không thì engine đầu
    tiên được bật của bản build (`runtime::lamp_order`), nếu không thì
    engine của đích của đèn; không có thì bị từ chối ("the lamp has no
    default agent yet …").
    - **Một lần kiểm tra, rồi một cách khởi động cho mọi agent**
      (`skill_task::default_agent_takes_tasks`): kiểm tra đường chạy duy
      nhất ở trên, và lối vào việc của wrapper (`TASK_ENTRY_LINES`: `if [
      "${1-}" = --lamp-task ]` và `agent_task "$Q"`, cùng các dòng cho mọi
      agent) với `agent_task` trong phần dữ liệu của agent, trước đường
      chạy (`AGENT_TASK_LINE`). Wrapper không có chúng (lamp-update cũ hơn,
      hay agent chưa có việc của riêng nó) bị từ chối bằng lời, có ghi
      nhật ký: "the lamp's default agent (<engine>) isn't set up for a
      skill's run yet (its wrapper /usr/local/bin/<engine> takes no skill's
      run without Harness: …), so nothing was started", không tạo hay khởi
      động gì. Hiện tại phần dữ liệu của Hermes có `agent_task` (`chat -q`
      với lời của việc, như Harness và một lần make khởi động nó); Claude
      Code và các agent thường khác chưa có; OpenClaw hoàn toàn chưa có
      wrapper đường chạy duy nhất, nên bị từ chối như trên.
    - **Cùng thư mục và file token.** `skill-<run-id>` (0700) trong cùng
      project Harness, file token của lần chạy được ghi trước khi khởi động
      (`RunTokenFile`), và lời của việc trong file `task` của thư mục (0600,
      `TASK_FILE`). Rồi lamp-tasks tự khởi động wrapper của agent:
      `/usr/local/bin/<engine> --lamp-task` trong thư mục đó, môi trường
      chỉ còn `PATH`, `HOME` và `SHELL`, stdin, stdout và stderr là null,
      trong một process group riêng. Wrapper đọc file token một lần rồi
      xoá, cho lần chạy token, socket, SDK và `--net` (cùng các dòng nhận
      lần chạy), và `agent_task` của nó biến file việc thành các đối số
      riêng của agent, khởi động bằng cùng một `unshare` vào `agent-ns`
      (docs/install.md, "The one run path"). Token không nằm trong đối số
      nào và trong môi trường nào lamp-tasks đưa.
    - **Kết thúc.** Wrapper thoát, tới hạn của lần chạy (14 phút) hay
      lamp-tasks dừng, cái nào tới trước, xem mỗi 50 ms (`RUN_TICK`). Thoát
      0 là `{"ok":true,"id":0}`; trạng thái khác là `run_failed` "the
      lamp's default agent ended with status <n>"; tới hạn là "the run was
      ended at its deadline, 840 s"; khi dừng là "the run was ended as
      lamp-tasks stops". Dù kết thúc thế nào, guard của nó trước hết cho
      lamp-control kết thúc token, rồi giết cả process group của lần chạy
      (`unshare --kill-child` của wrapper kéo theo namespace của agent) và
      thu dọn nó, rồi xoá thư mục (cả file việc) và file token. Không giữ
      gì lần chạy in ra. Các process của nó vẫn ở trong cgroup và giới hạn
      của `lamp-tasks.service`. Test:
      `signed_out_the_default_agent_starts_with_no_harness_and_leaves_nothing`,
      `a_run_with_no_harness_is_ended_at_its_deadline_or_the_stop_and_its_status_said`,
      `signed_in_a_skill_run_still_goes_through_harness`.
  - **Thư mục riêng.** lamp-tasks tạo `skill-<run-id>` (0700,
    `RUN_FOLDER_PREFIX`) trong project Harness của nó
    `/var/lib/lampos-tasks/harnesses/lamp-tasks` (tạo 0700 khi chưa có),
    không bao giờ dùng thư mục đã có, và ghi file token của lần chạy (ở
    trên). Rồi nó giao lần chạy bằng Cli mà việc nói dùng (`Cli::launch`),
    với định tuyến, phiên theo tên, resume, thư mục `--new`, chỉ mục phiên
    và việc khởi động lại daemon đều tắt (`SkillHarness::new`): `harness new
    hermes --machine <lamp> <folder> --name skill-<run-id> --json --task
    <words>`, thư mục là đối số vị trí của `new` (Harness chỉ kiểm tra đó là
    thư mục; Hermes không có folder trust cần đăng ký). Run id lấy từ thư
    mục đó: wrapper của Hermes lấy nó từ `pwd -P` (docs/install.md, "Hermes
    on the lamp"), đọc file token một lần, xoá nó, và cho lần chạy
    `LAMP_CONTROL_TOKEN`, `LAMP_CONTROL_SOCKET`, `PYTHONPATH` và
    `LAMP_NS_NET=run`, trước khi namespace bắt đầu. Theo đường chạy duy
    nhất (bên trên) lần chạy là `harness new <engine>` và wrapper đường
    chạy duy nhất của agent làm đúng như vậy, với cùng các dòng. Token không
    bao giờ nằm trong đối số, môi trường hay trạng thái của Harness.
  - **Kết thúc.** `harness wait --machine <lamp> <agent> --any --timeout
    600s --json` trên một agent đó, hỏi lại (một `wait` lỗi thì sau 1 s,
    `WAIT_RETRY`) cho tới khi nó kết thúc hoặc hạn chót (14 phút, đời của
    token) huỷ nó. Kết thúc `idle`, `stopped`, `closed` hay `gone` là
    `{"ok":true,"id":0}`; `failed`, hay `needs-input` (không ai trả lời lần
    chạy của kỹ năng), là `run_failed`. Một `stopped` chỉ được tính khi
    `harness show` thấy một phiên (một câu hỏi, một câu trả lời hay một
    công cụ; #6432): đôi khi Harness giữ pid của wrapper `hermes` ngay
    trước khi nó `exec` vào namespace, báo `stopped` khoảng 2,5 s sau, và
    liệt kê Hermes thật, còn đang nạp, thành một agent khác trong thư mục
    của lần chạy. Khi chưa có phiên, lamp-tasks xem `harness ps` mỗi 0,5 s,
    tối đa 5 s (`LOST_START_LOOK`), tìm một agent còn sống trong chính thư
    mục của lần chạy, và chờ agent đó thay vào, kết thúc của nó là kết thúc
    của lần chạy (guard đóng nó). Không thấy agent nào là `run_failed`
    "Harness lost the lamp's Hermes as it started", cũng ghi trong nhật ký.
    Không khởi động Hermes thứ hai. Việc gắn lại thuộc về Harness; đây là
    phía đèn. Test: `a_skill_run_whose_agent_stopped_before_a_session_*`.
    Ở hạn chót, lần chạy bị đóng bằng
    `harness close --machine <lamp> <agent> --now --json`
    (`Cli::close_now`) và là `run_failed` "the run was ended at its
    deadline, 840 s". `new` không khởi động được (`not_sent`, `uncertain`,
    hay một lần tạo `pending`) cũng là `run_failed`.
  - **Lần chạy không tới được agent của nó** (#6835 gap 1). Khi engine
    thoát trước khi sẵn sàng, Harness giữ pane của lần chạy làm một shell:
    cùng agent id, agent "Terminal", và `wait` đọc nó là `idle` trong vài
    giây. Vì vậy khi `wait` kết thúc (và phiên không `closed` hay `gone`),
    lamp-tasks hỏi `harness show --machine <lamp> <agent> --json`
    (`Cli::shown_agent`); không hỏi lại khi `show` của chính lần kết thúc
    `stopped` (#6432) đã thấy phiên được gắn, tức agent đã bắt đầu.
    Agent "Terminal" nghĩa là Harness không thấy tiến
    trình của engine, không phải agent đã không làm gì: lần chạy không bao
    giờ là `{"ok":true}`. Token của nó được kết thúc và phiên được đóng, và
    nó là `run_failed` "the skill's task may not have run: Harness kept the
    run's pane as a shell (agent Terminal, not hermes) …", nói thành lời,
    thêm "its session was not closed" khi đóng phiên thất bại. Nó không bao
    giờ được tạo lần thứ hai, vì một Hermes có thể đã bắt đầu nó (first
    builder, #6811), và việc ghi cố định của nó không bao giờ là
    `not_started`: agent của nó có thể đã bắt đầu (`SkillRun::started`).
    Một `show` nêu đúng
    engine, hay không được trả lời, để lần chạy được xét theo kết thúc của
    `wait` như trước. Kiểm thử:
    `a_run_harness_kept_as_a_shell_never_started_and_is_not_said_done`,
    `a_run_harness_kept_as_a_shell_is_run_failed_and_never_run_again`.
  - **Một dòng nhật ký cho mỗi lần chạy**: `[lamp-tasks] skill task
    <run-id>: ended, Harness says <state>, after <n> s; its agent: started
    (Hermes)`, hoặc `never started (Harness shows a shell, agent
    Terminal)`, hoặc `not known (<why>)` (`show` không được trả lời, phiên
    đã mất, hay việc chờ kết thúc ở hạn chót hoặc khi dừng, ghi `Harness
    says none`). Kiểm thử:
    `a_runs_journal_line_says_its_end_its_time_and_whether_its_agent_started`.
  - **Dù kết thúc thế nào**, guard của nó trước hết nhờ lamp-control kết
    thúc token của lần chạy (`{"end":{"run"}}`, một lần), rồi đóng phiên
    (`close --now`, một lần; không đóng phiên Harness báo đã `closed` hay
    `gone`), xoá thư mục của lần chạy, và xoá file token nếu wrapper chưa
    xoá (thứ tự V2 chốt trên #6510: thu hồi token, rồi `close --now`, rồi
    SIGKILL bên dưới, để lần chạy đang bị dừng không làm được gì trên
    đèn). Một lần tạo mà Harness hoàn tất sau
    đó không thấy thư mục hay token, và wrapper của nó dừng lại ("no token
    for run <run-id>"). lamp-tasks không giữ gì lần chạy in ra; bản ghi
    phiên của Harness và phiên của Hermes trong `HERMES_HOME` như với mọi
    phiên Harness.
  - **Khi lamp-tasks dừng** (SIGTERM: khởi động lại, cập nhật), lần chạy
    đang được chờ cũng kết thúc theo cùng cách, vì daemon của Harness, không
    phải lamp-tasks, giữ Hermes của nó và nó sẽ chạy tiếp sau lamp-tasks:
    lệnh dừng huỷ lần chờ, token bị kết thúc, rồi `close --now`, rồi thư
    mục và file token bị xoá ("the run was ended as lamp-tasks stops").
    lamp-tasks chờ việc này tối đa 20 giây (`skill_task::STOP_WAIT`) rồi
    mới thoát. `lamp-tasks.service` không đặt `TimeoutStopSec`, nên mặc
    định 90 giây của systemd đủ cho 20 giây đó; unit nào rút ngắn xuống
    dưới mức đó sẽ cắt ngang lần dừng. SIGKILL bỏ qua bước này; lần khởi
    động sau dọn những gì nó để lại (bên dưới). Test:
    `a_skill_run_is_ended_when_lamp_tasks_stops`.
  - **Các worker tự dừng** (#6717). Sau lệnh dừng, lamp-tasks chờ tối đa
    5 giây (`runtime::SHUTDOWN_WAIT`) để owner của store và worker Harness
    làm xong bước đang làm. Bước của owner có thể gồm một lần ghi store bền
    vững, mất hơn 300 ms nhiều trên thẻ SD hay ổ đĩa đang tải nặng. Chỉ
    worker vẫn bận sau thời gian đó mới báo "Tasks worker did not stop;
    exit this process before restarting".
  - **Dọn khi khởi động** (#6691). Một lần chạy bị SIGKILL cắt, hay một
    `close --now` quá `STOP_WAIT` hoặc lỗi, để lại phiên, thư mục hay file
    token. Ở lần khởi động sau, khi Harness đã sẵn sàng, lamp-tasks tìm mỗi
    lần chạy như vậy qua bất kỳ một dấu vết: thư mục `skill-<run-id>` trong
    project Harness của nó (không bao giờ là link), file token trong
    `/run/lampos-tasks/runs/`, hay một phiên trên máy của đèn tên
    `skill-<run-id>` làm việc trong chính thư mục của lần chạy đó,
    `<project>/skill-<run-id>` (`harness ps --json`, trường `folder`). Chỉ
    run id đúng dạng lamp-tasks tạo mới được tính, `task-<millis>-<pid>-<n>`
    (`made_run_id`), nên một pane, thư mục hay file người dùng đặt tên
    `skill-…` (ví dụ `skill-writer`) không bao giờ bị đóng hay xoá, và một
    phiên không có thư mục hoặc ở thư mục khác được để yên (review #6713). Token của lần chạy bị kết thúc
    với lamp-control trước (lần chạy giữ token trong bộ nhớ, nên xoá file
    không thu hồi gì), rồi mỗi phiên được `close --now` (trong
    `CLOSE_BUDGET`), rồi thư mục và file token bị xoá, kể cả khi đóng lỗi.
    Mỗi lần chạy có một dòng nhật ký nói đã kết thúc, đóng và xoá gì ("a
    run an earlier start left, skill-<id>: token end asked, closed …, folder
    removed, token file removed"). Việc dọn chạy trên một thread riêng trước khi
    Harness của skill được gắn, nên lần chạy của skill bị từ chối ("Harness
    isn't set up on this lamp yet") cho tới khi dọn xong, không lần chạy
    nào của lần khởi động này bị dọn, và việc nói không phải chờ. Khi đèn
    chưa có target, phiên không được tìm (có ghi lại); thư mục và file
    token vẫn bị xoá (`skill_task::sweep_at_start`). Test: `issue_6691_*`.
  - **Chưa có SIGKILL dự phòng.** Phần dự phòng trong thiết kế, SIGKILL
    tới pid `unshare` của namespace khi `close --now` lỗi, chưa được xây:
    daemon của Harness khởi động Hermes trong pane riêng, và cả `new` lẫn
    `show` đều không cho lamp-tasks pid đó (một `TODO(#6510)` trong
    `skill_task.rs`). Một `close --now` lỗi được ghi trong nhật ký; token
    của lần chạy bị lamp-control từ chối từ lúc nó kết thúc (hay hết đời 14
    phút), và file token đã mất, nên lần chạy còn sót không còn đường nào
    tới đèn.
  - **Giới hạn đã biết.** Mọi lần chạy dùng chung một `HOME` của Hermes
    (`/var/lib/lampos-hermes`); một home mới cho mỗi lần chạy vẫn còn mở
    (P3 của Heron và Reviewer-lead trên #6652). `/run/lampos-tasks/runs`
    nằm ngoài tầm của mọi phiên trong namespace: namespace phủ một tmpfs
    trống mới lên `/run`, nên không engine nào Harness khởi động trên đèn,
    và không lần chạy kỹ năng nào khác, đọc được file token của lần chạy
    khác. Một tiến trình ngoài namespace chạy bằng `lampos` (user của chính
    lamp-tasks và Harness) vẫn đọc được, giữa lúc file được ghi và lúc
    wrapper đọc.
- **Runner của đèn không qua Harness** (`HermesRunner::lamp`, giữ lại cho
  việc tạo kỹ năng của #6643 tới khi nó chuyển sang `HarnessRunner`; không
  `do.task` nào dùng nó): Hermes của đèn được khởi động
  như Harness khởi động nó cho đích `lamp`: `/usr/local/bin/hermes chat -q`
  và lời của lần chạy là đối số cuối (wrapper chỉ chạy Hermes trong
  namespace của nó, `hermes-ns`: không thư mục hay socket nào của dịch vụ
  đèn, các kỹ năng chỉ đọc; với `LAMP_SKILL_TASKS=on` trong unit của chính
  lamp-update, chỉ riêng `/run/lampos-control/control.sock` của lamp-control,
  chỉ đọc, không bao giờ `admin.sock` (docs/install.md, "Hermes on the
  lamp")), trong thư mục làm việc của đèn
  `/var/lib/lampos-tasks/harnesses/lamp-tasks/lamp`; thư mục đó không có thì
  là `run_failed` và không khởi động gì. Môi trường chỉ có `PATH`, `HOME`,
  `HERMES_HOME` và `SHELL` (như lamp-tasks.service cho), `LAMP_NS_NET=run`
  (bên dưới), cùng `LAMP_CONTROL_SOCKET` và `LAMP_CONTROL_TOKEN` của lần chạy; không gì khác
  của chính lamp-tasks. Token không bao giờ là đối số, và đầu ra của lần
  chạy không đi đâu cả, không vào nhật ký. Lần chạy là một nhóm tiến trình
  riêng, bị kill cả nhóm ở hạn chót (14 phút); mã thoát 0 là
  `{"ok":true,"id":0}` (chính mã thoát: không có số việc của Harness), mã
  khác là `run_failed` nêu mã đó.
- **Socket** (`HandOvers`): với `LAMP_SKILL_TASKS=on` trong môi trường của
  lamp-tasks, `serve` mở `/run/lampos-tasks/skill-task.sock`
  (`HAND_OVER_SOCKET`), tạo như các socket khác của nó (0600 trong thư mục
  runtime 0700); tắt thì không mở gì. Socket không mở được thì việc của kỹ
  năng vẫn tắt và phần còn lại vẫn chạy, có ghi trong nhật ký. Chỉ
  `lamp-skills.service` chạy bằng chính user của lamp-tasks được trả lời
  (đọc từ kernel, như tuyến của app đọc lamp-cloud); người gọi khác bị đóng
  kết nối, không trả lời. Mỗi kết nối là một dòng (bỏ ký tự xuống dòng, đọc
  trong 5 s), được trả lời trên một luồng riêng, nên vòng accept không bao
  giờ chờ một lần chạy. Mỗi lúc một lần chạy: dòng thứ hai trong khi một
  lần đang chạy được trả lời `busy` ngay, và tối đa 4 kết nối được đọc hay
  trả lời cùng lúc, kết nối thứ năm cũng là `busy`. Nhật ký chỉ giữ mã kết
  quả, không bao giờ giữ dòng, lời của việc hay token. lamp-skills chỉ tới
  socket bằng `--tasks-socket` hoặc `LAMP_SKILLS_TASKS_SOCKET` (#6346); unit
  lamp-skills của sản phẩm truyền `/run/lampos-tasks/skill-task.sock`.
- **Không mạng nào khác: hai đường ra** (thiết kế của người xây dựng đầu
  tiên trên #6443). Lần chạy được khởi động với `LAMP_NS_NET=run`
  (`NET_ENV`), cách chọn duy nhất, và nó chỉ lấy bớt quyền truy cập: với
  `LAMP_SKILL_TASKS=on` trong unit của chính lamp-update, wrapper của
  Hermes khi đó cho nó một network namespace riêng chỉ có `lo`
  (docs/install.md, "Hermes on the lamp"), nên không địa chỉ nào khác phân
  giải hay kết nối được. Các pane của chính Harness không bao giờ đặt biến
  này, và giữ mạng như cũ. Cả hai phía đều dừng an toàn (fail closed):
  script kết thúc lần chạy khi namespace có bất kỳ link nào ngoài `lo` (các tunnel dự phòng của
  kernel, `@NONE`, không có thiết bị bên dưới nên được bỏ qua), và
  `HermesRunner` không khởi động gì khi wrapper đã cài thiếu các dòng mạng
  (`WRAPPER_NET_LINES`, `NOT_HELD_OFF`: `LAMP_SKILL_TASKS` của lamp-update
  đang tắt hoặc vòng của nó chưa chạy), nên một lần chạy không bao giờ được
  khởi động với mạng của chính đèn (P2 của Heron và Fourteen trên #6443);
  `HarnessRunner` cũng vậy, và cả khi thiếu các dòng nhận lần chạy
  (`NOT_FOUND_BY_FOLDER`). Lần chạy qua Harness có `LAMP_NS_NET=run` do
  chính wrapper đặt, theo thư mục của nó.
  Hai đường ra là các tệp được cho thấy ở chế độ
  chỉ đọc trong đó: `control.sock` của lamp-control (ở trên), và cầu nối
  của lamp-tasks tới bộ chuyển tiếp model của lamp-cloud (`net_bridge`):
  - **Cầu nối** (`Bridge`): với `LAMP_SKILL_TASKS=on`, `serve` mở
    `/run/lampos-tasks/model.sock` (`MODEL_SOCKET`), tạo như các socket
    khác của nó (0600 trong thư mục runtime 0700), và nối mỗi kết nối tới bộ
    chuyển tiếp của lamp-cloud trên TCP `127.0.0.1:7741` (`FORWARDER`). Tắt
    thì không mở gì; không mở được thì ghi trong nhật ký, và script
    namespace của lần chạy khi đó dừng lại (fail closed).
  - **Relay** (`lamp-tasks net-relay --listen 127.0.0.1:7741 --to
    /run/lampos-tasks/model.sock`, `relay_main`): script namespace của lần
    chạy khởi động nó trước Hermes, bên trong namespace; nó chỉ nghe trên
    địa chỉ loopback, in `ready`, đóng đầu ra, và nối mỗi kết nối tới cầu
    nối cho tới khi PID namespace của lần chạy kết thúc nó. Các lệnh gọi
    model của Hermes vẫn tới `http://127.0.0.1:7741/v1/deepseek` như trước.
  - **Có giới hạn, và không nhìn thứ nó nối:** mỗi phía tối đa 4 kết nối
    cùng lúc (bộ chuyển tiếp của lamp-cloud chạy 4; thêm một thì bị đóng
    ngay), 5 s để tới phía bên kia, mỗi chiều kết thúc sau 130 s không có
    byte nào và mỗi kết nối sau 700 s (quá giới hạn đọc 120 s và trả lời
    600 s của chính bộ chuyển tiếp). Byte được nối thẳng, không bao giờ đọc
    như HTTP, và không gì của body hay header (token của bộ chuyển tiếp nằm
    trong một header) được ghi nhật ký: vài dòng được ghi là văn bản cố
    định. Không cái nào cho lần chạy nhiều hơn uid của nó vốn có: bộ chuyển
    tiếp trả lời mọi người gọi cục bộ, chỉ với token của nó.

**Chưa có (nói rõ để không hiểu code là nhiều hơn):** `ControlTokens` nói
theo socket quản trị của lamp-control (#6408, đã gộp), nhưng lamp-control
chưa giữ lời đồng ý nào của kỹ năng (#6509 thêm nó), nên hiện mọi lần xin
token ở đó đều bị từ chối, và khi `LAMP_CONTROL` tắt thì lamp-control không
mở socket chạy nào cho đến (b) (#6304).
Các unit sản phẩm đặt `LAMP_SKILL_TASKS=on` và truyền socket cho lamp-skills
(`LAMP_SKILLS_TASKS_SOCKET=/run/lampos-tasks/skill-task.sock`, 12/11), nên
việc của kỹ năng được giao đi, và bị từ chối ở lần xin token cho đến khi hai
phần đó vào. hermes-ns cũng chỉ cho lần chạy thấy
`control.sock` của lamp-control khi có `LAMP_SKILL_TASKS=on` trong unit của
chính lamp-update; network namespace riêng của lần chạy cũng vậy. Kết quả của
lần chạy chưa thành Task news, và thư mục của kỹ năng được
nêu trong lời của lần chạy, chưa nằm trong `AGENTS.md` của thư mục làm việc.

**Giới hạn của lamp-tasks.service** (#6306; bình luận 09:32 của first
builder trên #6306, 12/11). Lần chạy của một kỹ năng do chính lamp-tasks
khởi động, và wrapper của Hermes chỉ tách namespace, nên lần chạy vẫn nằm
trong cgroup của lamp-tasks.service, cạnh các tiến trình con Harness của
phần này (một `harness wait` và tối đa bốn `harness watch`, Node, mỗi cái
khoảng 130 MB trên lamp-75c9). Các pane của chính Harness chạy dưới user
unit của harnessd, không ở đây. Một việc của kỹ năng có thể chạy cạnh một việc nói, nên cgroup được tính
cho hai phần tăng của việc cùng lúc (`skill_task::TASKS_AT_ONCE`): giới hạn
bộ nhớ và task là **lúc nghỉ + 2 × phần tăng × 1,5**, làm tròn lên
(`skill_task::two_tasks_cap`), còn giới hạn CPU là CPU của hai việc nhân
1,5. Các số đo là của Fourteen cho một việc Hermes thật
bằng giọng nói (một danh sách đồ cần mang, 80 s) trên lamp-9adf, một đèn
6 GB chạy build 12255, đọc từ cgroup của lamp-tasks.service bằng
`systemctl show`; số CPU do first builder chuyển (13:14 trên #6059). Các
đỉnh được lấy mẫu mỗi 2 s, nên chỉ là mức sàn; phần nhân 1,5 bù cho phần
nằm giữa hai lần lấy mẫu. Sau khi việc đóng, cgroup giữ 727 MiB và 89 task.

| Số đo (`skill_task::`) | Đã đo | Nguồn | Giới hạn trong unit sản phẩm |
|---|---|---|---|
| Bộ nhớ cgroup, `MemoryCurrent` (`MEASURED_REST_MEMORY_MB`, `MEASURED_PEAK_MEMORY_MB`) | 632 MiB lúc nghỉ, đỉnh 1120 MiB (+488; mức sàn: mẫu mỗi 2 s) | lamp-9adf, 12/11, cgroup của lamp-tasks.service (#6059) | `MemoryHigh=2100M` (`MEMORY_HIGH`): 632 + 2 × 488 × 1,5 = 2096, làm tròn lên 100 MB. Vượt mức này kernel làm chậm và thu hồi bộ nhớ của chính phần này, không bao giờ kill: không có `MemoryMax`. Đèn không có swap, nên vượt mức này mọi tiến trình trong lamp-tasks.service, kể cả serve, bị dừng chờ thu hồi bộ nhớ cho tới khi cgroup xuống lại dưới mức (suy luận, chưa đo) |
| Task của cgroup, `TasksCurrent`, tiến trình gồm cả thread (`MEASURED_REST_TASKS`, `MEASURED_TASKS`) | 91 lúc nghỉ, đỉnh 126 (+35; mức sàn: mẫu mỗi 2 s) | lamp-9adf, 12/11, cgroup của lamp-tasks.service (#6059) | `TasksMax=512` (`TASKS_MAX`): 91 + 2 × 53 = 197, làm tròn lên 10 thành 200, nâng lên mức tối thiểu 512 (`TASKS_MAX_LEAST`): một lần chạy nhiều công cụ (Python, Node, các thread của trình duyệt) có thể vượt 100 ngay. Vượt mức này thì fork thất bại, kể cả của chính lamp-tasks; không gì bị kill |
| CPU (`MEASURED_CPU_PERCENT`) | khoảng 1,2 nhân (120%): trung bình trên cả lần chạy 80 s từ `CPUUsageNSec` của cgroup, là cận trên, không phải đỉnh | lamp-9adf, 12/11, cgroup của lamp-tasks.service (#6059); first builder, 13:14 | `CPUQuota=360%` (`CPU_QUOTA`): 2 × 120 × 1,5 = 360%, để lại hơn 4 trong 8 nhân của đèn (`LAMP_CORES`, chip A733) cho giọng nói; một kiểm thử giữ nó dưới 7 nhân |
| MemAvailable thấp nhất (`MEASURED_MIN_AVAILABLE_MB`) | 3208 MB (3.285.476 kB) | lamp-9adf, 12/11, cgroup của lamp-tasks.service (#6059) | không có; một kiểm thử giữ nó, trừ 488 MB của việc thứ hai (2720 MB), bằng hoặc trên mức sàn khi cài Hermes (`HERMES_FLOOR_MB`, ≥1.350 MB) |
| Thời gian chạy, từ `new` tới khi đóng (`MEASURED_RUN_SECS`) | 75 s | lamp-9adf, 12/11, cgroup của lamp-tasks.service (#6059) | không có; một kiểm thử giữ nó, nhân 1,5, trong vòng đời một lần chạy (`RUN_LIFETIME`, 14 phút) |
| Kích thước thư mục làm việc (`MEASURED_WORK_FOLDER_MB`) | 1 MB, làm tròn lên (36K trước, 40K sau) | lamp-9adf, 12/11, cgroup của lamp-tasks.service (#6059) | không có; chỉ ghi lại |

Mỗi `MEASURED_*` và giới hạn của nó phải khớp nhau, cả trong unit: các kiểm
thử thất bại khi một số đo đổi mà giới hạn không đổi theo.
Kiểm thử:
`skill_task::tests` (công tắc, vòng đời token, lần chạy lỗi, không có token,
mọi dòng bị từ chối; socket trả lời một dòng, lần chạy thứ hai bị từ chối
`busy`, tắt công tắc thì không mở gì, người gọi không phải lamp-skills không
được trả lời; lần chạy bị kill ở hạn chót, mã thoát của nó, và token nằm
trong môi trường, không bao giờ trong đối số; runner của đèn đặt
`LAMP_NS_NET=run`) và `net_bridge::tests` (một lệnh gọi được nối TCP sang
unix sang TCP và câu trả lời quay về, đúng từng byte, qua một bộ chuyển
tiếp giả trên cổng tạm; kết nối vượt mức tối đa bị đóng ngay và chỗ được
trả lại; đường nối không hoạt động bị kết thúc; không gì được nối bị ghi
nhật ký; đối số của relay; tắt công tắc thì không mở gì), và các giới hạn
(`skill_task::tests`: mỗi giới hạn có một lần trong `[Service]` của unit sản
phẩm, không có dòng giới hạn bộ nhớ cứng, mỗi giới hạn đủ cho hai việc với
phần nhân 1,5, MemAvailable thấp nhất trừ phần tăng của việc thứ hai bằng
hoặc trên mức sàn của Hermes, thời gian chạy
nhân 1,5 nằm trong vòng đời một lần chạy), chạy trên máy tính bất kỳ, không
cần Hermes. `HarnessRunner` (#6510 Part 1 bước 2):
`skill_task::tests::the_wrapper_run_lines_are_lamp_updates_own` (các dòng
nó tìm khớp với mã nguồn của lamp-update, và thư mục chúng khớp là thư mục
nó tạo), và trong `tests/harness.rs` với fixture đóng vai Harness,
`a_skill_run_starts_hermes_in_its_own_folder_through_harness_and_leaves_nothing`
(đối số của `new`, `wait` và `close --now`, thư mục là đối số vị trí,
không token nào trong đối số hay môi trường, thư mục và file token đã
mất), `a_skill_run_past_its_deadline_is_closed_now`,
`a_skill_run_that_fails_leaves_nothing` và
`a_skill_run_is_refused_with_no_harness_or_an_old_wrapper` (không đích,
không Harness, wrapper cũ: bị từ chối, không hỏi hay tạo gì).

**Dòng kiểm tra trên đèn (chưa chạy):** trên một đèn đã cài Hermes,
lamp-control đang phục vụ và `LAMP_SKILL_TASKS=on` trong cả hai unit, lần
chạy của một kỹ năng có script chạy `curl https://example.com` thì thất bại
(không tên nào phân giải được, không có route), trong khi lệnh gọi model của
cùng lần chạy được trả lời và lệnh gọi `control.sock` của nó chạy được với
token.

### Một lần tạo bằng giọng nói trong namespace của lần chạy kỹ năng (`skill_make`, #6643)

Điều kiện của first builder trên #6643 (12/11 18:15): một lần tạo bằng giọng
nói mà DeepSeek không giữ được, được giao cho agent của đèn
(`LAMP_MAKE_SKILL_AGENT`, [skills_vi.md](skills_vi.md), "Giao cho agent của
đèn khi DeepSeek thất bại"), chạy trong namespace của lần chạy kỹ
năng và không bao giờ chạy như một task Harness có công cụ riêng của
agent (`lamp_tasks::skill_make`).

- **Socket:** khi có `LAMP_SKILL_TASKS=on` (công tắc mà các chỉnh sửa
  namespace của lamp-update dùng), `serve` mở
  `/run/lampos-tasks/skill-make.sock` (`MAKE_SOCKET`) cạnh
  `skill-task.sock`, tạo như các socket khác của nó. Nó chỉ nhận dòng từ
  `lamp-voice.service` chạy bằng root, theo tên người gọi mà kernel cho;
  người gọi khác bị đóng mà không được trả lời. Mỗi lần chỉ chạy một lần
  tạo; lần thứ hai bị từ chối `busy`. Socket được mở theo
  `LAMP_SKILL_TASKS`, không chỉ khi việc giao được bật: công tắc giao,
  `LAMP_MAKE_SKILL_AGENT`, chỉ thuộc về lamp-voice (đặt trong unit của
  lamp-voice, và kiểm tra unit giữ nó ở đó), nên lamp-tasks không thấy
  được; một socket không ai giao lần tạo thì không nhận gì, vì chỉ
  lamp-voice chạy bằng root được gọi nó. Khi khởi động, lamp-tasks quét
  bỏ các thư mục `make-*` (cùng home của chúng) mà một lamp-tasks dừng
  giữa chừng để lại (`skill_make::sweep`).
- **Dòng:** `{"op":"skill_make","rules","within_ms"}`, được kiểm tra trọn
  vẹn (`MakeLine::parse`): không có trường nào khác, luật (system prompt và
  brief) tối đa 48 KiB và không có NUL, `within_ms` từ 1 đến 120 giây.
- **Lần chạy: chỉ Hermes** (quyết định của first builder, 12/11 19:14):
  Hermes của đèn được khởi động đúng như một lần chạy `do.task` của kỹ
  năng (`skill_task::HermesRunner` bên trong `skill_make::MakeHermes`,
  đường của row-6443), qua wrapper namespace của nó: chỉ thư mục riêng,
  thư mục làm việc và các kỹ năng, và chỉ `control.sock` của lamp-control,
  không bao giờ `admin.sock`. Hiện chỉ Hermes có trình chạy namespace, nên
  một lần tạo không bao giờ chạy bằng engine khác; giọng nói chỉ giao lần
  tạo khi target `lamp` là Hermes. Không token nào được tạo và môi trường
  của lần chạy chỉ mang home riêng của nó: một lần tạo không có quyền nào.
  Lần chạy bị giết khi hết `within_ms`, cùng cả nhóm tiến trình.
- **Một home mới của riêng nó:** mỗi lần tạo có `make-<run id>/home`,
  0700, mới và trống ngoài cấu hình nhà cung cấp,
  `home/.hermes/config.yaml` (`hermes_config::merge_config` với cổng của
  bộ chuyển tiếp, khóa được gọi bằng tên biến, `LAMP_HERMES_TOKEN`, không
  bao giờ là một khóa, cùng bốn thiết lập Hermes của lamp-update,
  `MAKE_CONFIG`, một kiểm thử giữ chúng bằng `hermes_install::CONFIG`:
  không kiểm tra cập nhật, không language server, lần chạy `chat -q` kết
  thúc cùng lượt của nó), và không có gì từ bộ nhớ, phiên hay trạng thái
  của Hermes; bên cạnh đó là hai liên kết tới runtime đã chuẩn bị của
  Hermes trong home lâu dài, `home/.hermes/installs` (bản ghi phụ thuộc và
  các môi trường, `installs/<key>/facts.json`) và `home/.hermes/tools` (uv,
  Python và trình quản lý gói), để một lần tạo không bao giờ tự chuẩn bị
  môi trường riêng. Hermes 0.22.2 giữ trạng thái phụ thuộc theo từng data
  root (`pm/environments.py`, `<HERMES_HOME>/installs/<key>`): khi một
  `HERMES_HOME` mới không có, nó đồng bộ một môi trường trước ("preparing
  dependencies for this data root"), việc mà lần chạy của một lần tạo
  không thể làm xong, và kết thúc `run_failed` trong chưa tới một giây trên
  lamp-c0a4 (#6643). Dùng liên kết, không dùng bind mount: Hermes chỉ nhận
  một môi trường đã ghi khi, sau khi resolve, nó nằm dưới
  `<HERMES_HOME>/installs/<key>/environments`, và một liên kết resolve về
  đúng đường dẫn lâu dài mà bản ghi ghi tên. Lần chạy nhận `HOME`, `HERMES_HOME` (`home/.hermes`) và
  `LAMP_NS_HOME` trỏ tới đó. Wrapper của lamp-update chỉ nhận
  `LAMP_NS_HOME` khi đường dẫn khớp
  `/var/lib/lampos-tasks/harnesses/lamp-tasks/lamp/make-*/home` và không có
  `..` (`WRAPPER_HOME_LINE`), rồi script namespace đặt `HOME` thành nó,
  che home lâu dài của Hermes (`/var/lib/lampos-hermes/.hermes`: bộ nhớ,
  phiên, trạng thái) dưới một tmpfs trống chỉ đọc, chỉ cho thấy lại bản
  checkout, `tools` (Python mà Hermes chạy trên đó) và `installs` (bản ghi
  phụ thuộc của nó), chỉ đọc, để các liên kết resolve được và không gì
  trong đó bị thay đổi, để thư mục của Hermes và thư mục làm việc ở chế độ
  chỉ đọc, và che phần còn lại của `lamp` trong thư mục làm việc dưới một
  tmpfs khác, chỉ thư mục của lần tạo được đưa lại, đọc-ghi
  (`NS_HOME_LINE`; docs/install.md).
  Home bị xóa cùng thư mục của lần tạo dù lần tạo kết thúc thế nào: có câu
  trả lời, thất bại, quá hạn hay panic.
- **Dấu của wrapper, cách ly hay không:** trước mỗi lần tạo, lamp-tasks
  kiểm tra `/usr/local/bin/hermes` có `WRAPPER_HOME_LINE` và
  `/usr/local/lib/lampos/hermes-ns` có `NS_HOME_LINE`; nếu không, lần tạo
  bị từ chối `no_namespace` trước khi bất cứ gì được tạo; lần tạo mà thư
  mục làm việc không phải `/var/lib/lampos-tasks/harnesses/lamp-tasks/lamp`
  (`MAKE_WORK`), nơi wrapper nhận home của lần tạo, cũng vậy. Cho tới khi một
  vòng lamp-update viết lại wrapper và script, mọi lần tạo đều bị từ chối
  như vậy và vẫn là của DeepSeek.
- **Mạng của nó (`LAMP_MAKE_AGENT_NET`, không unit nào đặt):** mặc định giữ
  mạng, như agent của V1 (yêu cầu của chủ sở hữu, 12/11; quyết định của first
  builder, 12/11 19:14). Khi đó mô hình được gọi qua bộ chuyển
  tiếp của lamp-cloud trên `127.0.0.1:7741`, chính nơi `model.sock` nối tới.
  Đúng giá trị `held` cho lần chạy nghiêm ngặt, `LAMP_NS_NET=run`: chỉ `lo`
  và các tunnel `@NONE` của kernel, mô hình chỉ qua `model.sock`. Khi
  `held`, wrapper không có các dòng mạng (`WRAPPER_NET_LINES`) bị từ chối
  `not_held_off` trước khi bất cứ gì được tạo (`Runner::ready`). lamp-tasks
  ghi nhật ký chế độ nó dùng khi khởi động.
- **Thư mục của lần tạo:** `make-<run id>` trong thư mục làm việc của đèn
  (thư mục mà lần chạy kỹ năng bắt đầu trong đó, namespace của Hermes cho
  thấy), 0700, luật nằm trong `rules.md`, 0600. Lần chạy bắt đầu ở đó và được
  bảo ghi câu trả lời vào `answer.md` ở đó và không gì khác. Khi lần chạy đã
  kết thúc, `answer.md` được đọc bằng `openat` từ descriptor riêng của thư
  mục, lấy khi thư mục được tạo, `O_NOFOLLOW` trên từng phần (một tệp
  thường, không bao giờ đi theo liên kết, tối đa 20 KiB), nên một thư mục bị
  thay bằng liên kết không bao giờ được đọc qua. Thư mục bị xóa dù lần tạo
  kết thúc thế nào.
- **Các câu trả lời:** `{"ok":true,"started":true}` khi lần chạy sắp bắt đầu
  (giọng nói chỉ nói câu chờ khi đó), rồi `{"ok":true,"answer"}`. Nếu không,
  một lời từ chối: `off`, `bad_request`, `too_long`, `busy`, `not_held_off`,
  `no_namespace`, `run_failed` hoặc `no_answer`. Giọng nói không giao gì
  khi gặp `off`, `not_held_off`, `no_namespace` hoặc không có socket (bản
  cũ). Nhật ký chỉ ghi mã kết quả,
  không bao giờ ghi luật hay câu trả lời.
- **Kiểm thử:** `skill_make::tests`: một lần tạo chạy trong thư mục riêng,
  luật nằm ở đó và câu trả lời được đọc lại, môi trường chỉ có home riêng,
  thư mục bị xóa; home của mỗi lần tạo lúc bắt đầu chỉ có cấu hình nhà cung
  cấp, lần tạo thứ hai không bao giờ thấy những gì lần đầu để lại trong
  home, và home biến mất sau câu trả lời, thất bại, quá hạn và panic; lần
  chạy không cách ly được hoặc không có dòng make-home thì không khởi động
  gì và không tạo thư mục; không có câu trả lời, câu trả lời là liên kết,
  thư mục bị thay bằng liên kết, công tắc tắt và mọi dòng sai đều bị từ
  chối; lần tạo được khởi động qua wrapper và script phải có các dòng
  make-home dù cách ly hay không, và các dòng mạng khi cách ly, với
  `LAMP_NS_NET=run` chỉ khi cách ly, không có token và không có admin
  socket trong môi trường, trong thư mục của lần tạo, với `HOME` và
  `HERMES_HOME` riêng; các dòng đúng từng chữ của lamp-update; thư mục làm
  việc khác với của wrapper bị từ chối; cấu hình mới có bốn thiết lập của
  lamp-update, bằng đúng của nó; thư mục tạo cũ được quét bỏ khi khởi
  động; `a_makes_namespace_hides_hermess_lasting_home_and_the_rest_of_the_work_folder`
  của lamp-update (mount giả lập) và, trên Linux,
  `inside_a_makes_namespace_hermess_lasting_memory_is_not_there` (namespace
  thật: không thấy bộ nhớ lâu dài, chỉ thư mục của lần tạo ghi được); văn bản namespace của lamp-update chỉ cho thấy `control.sock` và
  không bao giờ nhắc `admin.sock`; chỉ lamp-voice chạy bằng root được giao
  một lần tạo; socket nói `started`, rồi câu trả lời; mặc định giữ mạng mà
  vẫn không cho thấy admin socket, và `held` cho `LAMP_NS_NET=run`.

**Các dòng kiểm tra trên đèn (chưa chạy):** trên lamp-9adf với bản này, hỏi
"Hey Lamp, make me a skill that checks the weather every morning". Câu trả
lời của DeepSeek không được giữ, và nhật ký ghi "handed to the agent". Trong
lúc lần tạo chạy, tìm Hermes của nó và xem từ bên trong namespace:

```sh
P=$(pgrep -u lampos -f 'hermes chat -q Everything you need' | head -1)
sudo nsenter -t "$P" -m ls /run/lampos-control                # chỉ control.sock, không có admin.sock
sudo nsenter -t "$P" -n ip -o link show                       # các link của mạng
sudo nsenter -t "$P" -n curl -sS -m 5 -o /dev/null -w '%{http_code}\n' https://example.com; echo "exit $?"
```

- **Mặc định (giữ mạng):** `curl` trả 200 (mã thoát 0), và
  `/run/lampos-control` chỉ có `control.sock`.
- **Held** (`sudo systemctl edit lamp-tasks` với
  `Environment=LAMP_MAKE_AGENT_NET=held`, rồi khởi động lại; nhật ký ghi "is
  held off the network"): `ip` chỉ cho thấy `lo` và các tunnel `@NONE`,
  `curl` thất bại (mã thoát 6 hoặc 7), `/run/lampos-control` vẫn chỉ có
  `control.sock`, và lần tạo vẫn kết thúc với câu trả lời qua relay mô hình.
  Sau đó xóa drop-in.

**Wrapper của mọi engine (#6510 phần 1, bước 1, tắt trừ khi
`LAMP_AGENT_NS=on`):** wrapper chung của lamp-update cho mọi engine
(docs/install.md, "One agent namespace for every engine") mang cùng các
dòng nhận lần chạy như của Hermes (bước 2): khởi động trong
`/var/lib/lampos-tasks/harnesses/lamp-tasks/skill-<run-id>`, nó đọc
`/run/lampos-tasks/runs/<run-id>` một lần, xoá tệp, và đưa cho engine
`LAMP_CONTROL_TOKEN`, `LAMP_CONTROL_SOCKET` và `PYTHONPATH` chỉ trong môi
trường, kèm `LAMP_NS_NET=run`; `LAMP_RUN_ID` tạm thời đã bỏ. `HOME` của
engine là thư mục riêng `/var/lib/lampos-agents/<engine>`. Một engine chỉ
được coi là có khi login shell của `lampos` tìm thấy wrapper trước (không
phải chương trình của chính nó trong `~/.local/bin`). Khi unit riêng của
lamp-update có `LAMP_AGENT_NS=on` (và `LAMP_APP_ENGINES=on`), mỗi vòng ghi
nó cho mọi engine mà Harness báo đã cài, trừ Hermes và OpenClaw, và gỡ
wrapper do nó ghi (đúng y văn bản của nó, không gì khác) khi Harness không
còn báo engine đó đã cài hoặc không còn chương trình riêng nào của engine
trên `PATH` đăng nhập của `lampos` (lần tìm thất bại thì giữ lại); một dòng
vẫn đúng qua nhiều vòng chỉ được ghi log một lần. Lần chạy của kỹ năng vẫn
chỉ tới Hermes (`HarnessRunner`, ở trên).

### Lệnh ghi cố định của một kỹ năng tự tạo trên Hermes của đèn (`LAMP_SKILL_WRITE_AGENT`, tắt)

Quyết định của chủ đèn (13/11): mọi thứ chạm vào một dịch vụ đã liên kết
đều chạy qua tác tử của đèn. Khi `LAMP_SKILL_WRITE_AGENT=on` (đúng `on`,
không bật trong unit sản phẩm nào) và `LAMP_AGENT_SERVICES=on` trong
lamp-tasks, socket trao việc cũng nhận lệnh ghi cố định của một kỹ năng tự
tạo (`do.write`, docs/skills.md) như một lượt chạy
(`skill_task::WriteHandOver`, `answer_with`):

- **Dòng:** `{"op":"skill_write","skill","folder","grants","write"}`, cần
  mọi trường và không nhận trường nào khác. `skill` và `folder` được kiểm
  như của một task; `grants` đúng một quyền, `-write` của chính dịch vụ của
  lệnh ghi (`gmail-app` và `figma-api` là `gmail` và `figma`); `write` là
  một object có `service` và `action`, tối đa 8 KiB dạng JSON. Mọi thứ khác
  là `bad_request`, và không xin token nào.
- **Lượt chạy** là của một task (`run_on_token`, cùng việc cấp token, kiểm
  quyền, kết thúc và runner): lamp-control cấp token cho đúng một quyền đó
  (docs/control.md), và Hermes được bảo, kèm tên kỹ năng và id của lượt
  chạy, gọi `service_write` của `lamp-services` một lần chỉ nêu dịch vụ và
  action của lệnh ghi (`{"service":"gmail-app","action":"send_mail"}`),
  không gọi lại dù nó trả lời gì. lamp-control đưa cho lệnh gọi đó lệnh ghi
  của kỹ năng theo token của lượt chạy (`runs::FixedWrite`), và
  lamp-services chỉ gửi như lời đồng ý đã giữ của chủ, một lần mỗi lượt
  chạy (docs/services.md, "Through the lamp's agent").
- **Không bao giờ có nội dung trong lời nhắc** (P2 của Reviewer-lead trên
  #6824 và #6879): lời nhắc là argv của tác tử (`harness new --task`, hoặc
  `hermes chat -q` khi không có Harness, đọc được trong
  `/proc/<pid>/cmdline`) và transcript của Harness, nên không người nhận,
  tiêu đề hay nội dung nào của lệnh ghi có trong đó
  (`WriteHandOver::prompt`). `Debug` của `SkillRun` chỉ in độ dài của lời
  nhắc, với cả lượt chạy của một task.
- **Tắt** (một trong hai công tắc), dòng `skill_write` bị từ chối như
  trước: `bad_request`, "not a skill's task line".
- **Chưa bắt đầu** (#6823): lượt chạy của một lệnh ghi cố định thất bại
  trước khi tác tử có thể đã bắt đầu (`SkillRun::started`, do runner đặt khi
  nó nhờ Harness launch, trừ khi Harness chưa hề gửi, hoặc khi Hermes đã
  được spawn: không có Harness, không có target, wrapper cũ) bị từ chối
  `not_started` (`skill_task::NOT_STARTED`) bằng lời của runner, nên
  lamp-skills tự gửi thẳng lệnh ghi; lượt thất bại sau đó là `run_failed`,
  không biết lần gửi ra sao, và lamp-skills không thử gì thêm. Lượt chạy của
  một task vẫn là `run_failed` trong cả hai trường hợp, như trước. Test:
  `a_fixed_write_runs_on_its_one_grant_naming_only_its_service`,
  `a_fixed_writes_words_argv_and_debug_carry_none_of_its_content`.

### Lần chạy của kỹ năng agent và quyền `net` (`LAMP_UPLOAD_TO_AGENT`, `LAMP_TASK_NET`, tắt)

Người xây dựng đầu tiên đồng ý lúc 19:44 (12/11), cả hai công tắc tắt và
không đặt trong unit nào; xây trên đường `skill_make` của #6643, để chuyển
sang runner chung của #6510 bước 2 mà không cần luật mới.

- **`net`** (`skill_task::NET_SWITCH`): một quyền của lần chạy, không phải
  của lamp-control. Lần chạy có `net` đi qua runner của lần tạo, giữ mạng
  (`skill_make::lamp_runner(home, false)`); lần khác vẫn `LAMP_NS_NET=run`.
  lamp-control giữ `net` trong lời đồng ý nhưng không bao giờ đặt nó vào
  token, nên sau khi tạo token lamp-tasks hỏi lamp-control
  (`task_yes_check` với SHA-256 thư mục và các quyền của dòng): chỉ khi
  lời đồng ý có `net` cho thư mục đúng như hiện tại thì lần chạy mới giữ
  mạng, nếu không thì `no_token` và không chạy gì. Tắt: `net` bị từ chối
  trước khi có token.
- **Lần chạy của kỹ năng agent** (`run` = `scripts/SKILL.md`): Hermes được
  bảo đọc tệp đó và làm theo, chỉ chạm tới đèn qua `control.sock` với token
  của lần chạy, không bao giờ hỏi khóa hay token bằng giọng nói. Nó bắt đầu
  trong một thư mục mới `agent-<run id>` (0700), bị xóa khi kết thúc. Môi
  trường chỉ có socket, token và đường dẫn SDK: không có khóa.

## Harness của đèn đang thế nào (#6061 phần B, tắt trừ khi `LAMP_TASKS_HARNESS_STATUS=on`)

Trước đây giọng nói không nói được Harness của đèn đã đăng nhập chưa, hay giao
việc được cho máy tính và trợ lý nào, dù lamp-tasks biết cả hai. Với
`LAMP_TASKS_HARNESS_STATUS=on` và phần tasks (`LAMP_TASKS_DIR`), cờ
`--tasks-harness-status` của giọng nói (đứng cuối, sau `--tasks-reply-named`)
cho mô hình thêm hàm `harness_status`. Hàm này trả lời "Harness đăng nhập chưa?",
"đèn có những máy tính nào?" và "đèn giao việc được cho ai?".

- **Đọc ở đâu.** Ba yêu cầu trên socket của phần tasks: `status` (lấy
  `signed_in`), `targets` (bí danh, tên riêng và engine của từng máy) và
  `tasks_now` (đếm số việc đang làm). Cả ba đều là bộ nhớ riêng của phần tasks,
  không gọi Harness, nên trả lời ngay. lamp-tasks không đổi gì.
- **Mô hình nhận được.** `signed_in` (`yes`, `offline`, `no` hoặc `unknown`);
  `computers`: mỗi máy một lần (các target cùng tên riêng là một máy), luôn
  gọi bằng bí danh đầu tiên của máy (`office`), không bao giờ bằng tên riêng,
  dù có `LAMP_TASKS_MACHINE_NAMES` hay không, kèm các trợ lý máy đó có (bỏ engine
  `installed: false`), tối đa ba máy, phần còn lại đếm trong
  `more_computers`; `working`: số việc đang làm, không có lời của việc nào,
  và bỏ trống khi `tasks_now` bị từ chối; `next`: chỉ trả lời điều được hỏi,
  một hai câu. Khi chưa đăng nhập, chỉ mời đăng nhập bằng giọng nói khi có
  `LAMP_TASKS_SIGN_IN`, không thì bảo đăng nhập trong app. Chưa đăng nhập mà đèn có
  target của chính nó (#6407), `next` nói trước là việc vẫn đi tới trợ lý của
  chính đèn, và chỉ cần "sign in to hand tasks to your computers", không bao
  giờ nói là chưa đăng nhập thì không làm được gì.
- **Khi lỗi.** Không có phần tasks thì nói như mọi lệnh giao việc khác. `status`
  bị từ chối thì nói nhẹ nhàng là không làm được, journal chỉ ghi mã lỗi.
- **Tắt** thì hàm không được khai báo, các hàm khác giữ nguyên từng chữ.
- **Ai nghe.** Bất kỳ ai hỏi đèn đều nghe máy tính theo bí danh (`office`) và
  trợ lý của người dùng, không bao giờ nghe tên riêng của máy (thường chứa tên
  người). Công tắc tắt trong bản product cho đến khi first builder xét
  (TWO READS).
- **Test** (`agent_tasks`): `harness_status_is_declared_and_run_only_with_its_switch`,
  `harness_status_says_the_sign_in_the_computers_and_the_work_as_counts`,
  `harness_status_refused_says_it_lightly`,
  `harness_status_signed_out_with_the_lamps_own_target_says_computers_need_the_sign_in`.

## Chính đèn, với OpenClaw (#6226)

`LAMP_OPENCLAW_ON_LAMP=on`, chỉ cùng `--engines-from-machines`; tắt nếu không
đặt. OpenClaw là engine thứ hai của đèn (không có ngưỡng bộ nhớ: chủ sản
phẩm, #6226, mọi đèn đều có 6 GB): nó làm target riêng của đèn, đúng vai trò
của Hermes (`tasks.md`, "The lamp itself, with Hermes"), khi được chọn hoặc
khi Hermes chưa cài; Hermes lại là mặc định (#6498, quyết định của lead, thay
cho OpenClaw của #6226). `lamp_order` (`crates/tasks/src/runtime.rs`) chỉ bỏ
OpenClaw ra khi Harness trên đèn cũ hơn 0.6.11.

- **Engine nào:** `LAMP_LAMP_ENGINE` là `openclaw` hoặc `hermes`, không đặt
  thì `hermes`, mặc định (#6498; #6226 từng là `openclaw`). Target
  của đèn vẫn là `lamp`, với engine đó; khi engine được chọn chưa cài mà
  engine kia đã cài, dùng engine kia.
- **Các engine của đèn, có nêu hay không, và lựa chọn trong app (#6492).**
  Không có gì trên đèn được dựng riêng cho từng engine: Harness có nhiều
  engine, Hermes và OpenClaw chỉ là hai trong số đó. Các engine của đèn là
  những engine mà lần xem engine của Harness
  (`harness machines engines <id> --json`, đọc cùng các lần xem máy tính,
  có giới hạn và làm mới như chúng) báo là đã cài trên chính máy của đèn
  (máy mà `harness machines` đánh dấu `current`), theo thứ tự của Harness,
  trong số những engine Harness khởi động được kèm một việc: engine mà
  lamp-tasks không có tên là engine Harness khởi động không có prompt đầu
  (`PROMPT_UNSUPPORTED`), nên không bao giờ được đưa ra, và Devin Cloud
  chạy trong cloud của Devin, không bao giờ trên đèn. `LAMP_HERMES_ON_LAMP`
  và `LAMP_OPENCLAW_ON_LAMP` chỉ là công tắc tắt: engine có công tắc tắt
  thì không được đưa ra (OpenClaw cũng không khi Harness cũ hơn 0.6.11);
  mọi engine khác không có công tắc. Trên đèn, một engine chỉ được đưa ra
  khi `/usr/local/bin/<engine>` của nó là wrapper do lamp-update ghi, có dòng
  `Written by lamp-update.` trong 512 byte đầu (`LAMP_WRAPPER_MARK`, #6506):
  wrapper chạy engine trong namespace riêng, tách khỏi các dịch vụ của đèn và
  đồ của người dùng. Chương trình đặt ở đó theo cách khác (một `claude` cài
  tay) thì không được đưa ra và app không đặt nó làm mặc định được
  (`engine_not_installed`), dù Harness báo gì. Trước khi một lần xem báo về (hoặc khi
  đăng xuất, #6407), các engine của đèn là những engine đang bật, như
  trước. Engine mặc định đứng đầu, rồi mọi engine khác theo thứ tự của
  Harness (`lamp_order`): lựa chọn của chủ trong app khi nó nằm trong số
  đó, nếu không thì của `LAMP_LAMP_ENGINE` (tên bất kỳ engine nào), nếu
  không thì của bản build (engine đầu tiên đang bật của `lamp_engines`),
  nếu không thì engine đầu tiên trong số đó. Mục `lamp` trong câu trả lời
  `targets` báo engine mặc định là `engine` của nó và liệt kê tất cả, mặc
  định đứng đầu:
  `{"alias":"lamp","engine":"claude","engines":["claude","hermes","openclaw"]}`
  (không có `engines` khi không có engine nào). Một `prepare` cho target
  `lamp` có thể mang `"engine"` với tên bất kỳ engine nào (không có thì
  không ghi; target khác bỏ qua). Có nêu, việc chạy đúng trên engine đó khi
  nó nằm trong các engine của đèn và không được biết là thiếu ở đó, được
  chuyển cho Harness theo tên (`harness new <engine>` trong thư mục của
  đèn, như Harness khởi động engine đó: không có runner riêng cho từng
  engine ở đây; phần chờ Gateway và kiểm tra mất prompt đầu của OpenClaw,
  #6226, vẫn theo tên nó vì đó là hành vi Harness đã ghi rõ); nếu không,
  `prepare` bị từ chối với `engine_not_here` và không khởi chạy gì: không
  bao giờ chuyển sang engine khác. Tên rỗng hoặc chỉ có khoảng trắng bị từ
  chối với `engine_unknown`. Không nêu, việc chạy trên engine mặc định.
  Thư mục của đèn chỉ nhận hướng dẫn của Hermes hoặc OpenClaw cho hai
  engine đó; engine khác đọc cài đặt riêng của nó, như Harness khởi động
  nó. App đặt engine mặc định trên route của app (`--app-socket`) với
  `{"op":"app_engine_default","engine":"claude"}`: nằm trong các engine của
  đèn thì được lưu và có hiệu lực ngay; rỗng thì `engine_unknown`; không
  nằm trong số đó thì `engine_not_installed`; `engine_default_unsaved` khi
  không ghi được file. Nó được lưu ở `engine-default.json` trong thư mục
  tasks (`--dir`) dạng `{"version":1,"engine":"claude"}`, ghi bên cạnh rồi
  đổi tên đè, và có hiệu lực ngay, không cần khởi động lại: thứ tự của đèn
  đặt nó lên đầu, câu trả lời `targets` báo nó là engine của target `lamp`
  (để voice gọi đúng tên), và target của đèn được ghi lại ở lần xem kế
  tiếp. `{"op":"app_engine_default_state"}` đọc nó. Cả hai trả
  `{"ok":true,"default":"claude","chosen_in_app":true}` (`default` là null
  khi đèn không có engine; `chosen_in_app` chỉ true khi lựa chọn của app
  đang là mặc định). File được đọc khi khởi động; thiếu, không hợp lệ, hoặc
  nêu một engine Harness khởi động không kèm việc thì là chưa chọn. Một lựa
  chọn không còn nằm trong các engine của đèn (Harness không còn báo nó,
  hoặc công tắc của nó tắt) bị bỏ qua, không bị xoá. Reset của đèn xoá nó
  cùng thư mục tasks. Có trong test:
  `engine::tests::a_named_engine_runs_the_lamps_work_on_it_even_when_not_first`,
  `engine::tests::a_named_engine_not_in_the_lamps_order_is_refused_never_moved`,
  `engine::tests::no_engine_named_runs_on_the_lamps_default_and_another_target_ignores_it`,
  `engine::tests::a_third_engine_harness_reports_on_the_lamp_can_be_named_and_be_the_default`,
  `runtime::tests::the_apps_engine_default_is_kept_used_at_once_and_read_again`,
  `runtime::tests::the_lamps_engines_are_harness_s_the_default_first_then_harness_s_order`,
  `runtime::tests::the_lamp_is_kept_only_with_hermes_chosen_for_it`,
  `readiness::tests::the_lamps_installed_engines_keep_harness_s_order`,
  `protocol::tests::a_prepare_may_name_an_engine_and_the_app_sets_the_default`.
- **Nêu trong lời nghe được, do code đặt (#6492, V2 của lead):** khi đèn
  có target riêng, voice đọc câu chính người nói trong request đó (câu do
  đèn viết thì không) và tự đặt `where`/`engine`/`who`, để model bỏ sót
  cũng không gửi việc đi nơi khác. Tên chỉ lấy từ những gì đèn này có: các
  engine của đèn như lần đọc gần nhất (theo id hoặc tên gọi) và staff trên
  máy tính (Claude Code khi chưa đọc được); không bao giờ là danh sách agent
  cố định. Tên chỉ tính khi là người làm: sau "ask", "have", "tell", "get"
  hoặc "let", hoặc "with", "using", "via" hoặc "use" (không tính "don't ask
  …", "did you ask …"), hoặc đứng đầu câu trước dấu phẩy ("Hey Lamp,
  Hermes, …"); hoa thường gì cũng được, các chữ của tên viết liền hay rời
  ("Open Claw"). Nêu một engine của đèn: `where` `lamp` và `engine` đó, bất
  kể model gửi gì. Nêu một staff: `where` `computer`, `who` là họ, không có
  `engine`. Tên có cả trên đèn và trên máy tính thì chạy trên đèn, trừ khi
  máy tính được nêu là nơi làm hoặc lời nói cần đồ của máy tính: "my" +
  repo, repository, codebase, code, project, folder, files hoặc branch,
  hoặc "the repo/repository/codebase" (chỉ là từ chung; riêng dấu hiệu đó
  thì không đặt gì). "On/use my/the computer", "Mac", "MacBook", "laptop"
  hoặc "PC" (có thể có một từ ở giữa: "my office computer") đặt `where`
  `computer` và bỏ `engine` (và `who` chỉ nêu engine của đèn); phủ định
  ("don't use my computer") hoặc "on the lamp", "yourself", "right here"
  đặt `where` `lamp`, giữ `engine` của model. Chỉ nhắc tên thì không tính
  ("what is Hermes?", "OpenClaw's update"), tên không có trong danh sách
  nào cũng không; lời mâu thuẫn (hai agent, đèn và máy tính, agent chỉ có
  trên đèn kèm "on my computer", agent chỉ có trên máy tính kèm "on the
  lamp") không đặt gì, giữ nguyên tham số của model. Mỗi lần ghi đè log một
  dòng nêu cái được đặt và cái model gửi, không bao giờ ghi lời nói. Câu
  đọc lại vẫn nói nơi chạy như trước ("OpenClaw on the lamp", "Claude Code
  on the office computer"). Lượt mà model không gọi `start_agent_task` thì
  được bắt ở route thứ hai (mục kế tiếp). Test:
  `agent_tasks::tests::the_heard_words_name_the_doer_and_the_place`,
  `agent_tasks::tests::an_agent_on_both_works_on_the_lamp_unless_the_computer_is_needed`,
  `agent_tasks::tests::the_heard_words_set_where_engine_and_who`,
  `agent_tasks::tests::the_heard_words_set_the_engine_and_place_over_the_model`.
- **Có nêu tên mà không giao việc: hỏi lại một lần (#6547 (1b)).** Ở route
  thứ hai, khi có phần tasks, một câu nói mà chính lời người nói nêu một
  agent để làm việc, đọc đúng như trên (chỉ tính người làm, lấy từ các
  engine của đèn khi đèn có target riêng và từ staff, Claude Code khi chưa
  đọc được; chỉ nhắc tên như "What is Hermes?" thì không tính; một agent
  chỉ được gọi tên ở đầu câu, "Hermes, …", chỉ tính khi phần còn lại của
  câu nhờ làm việc: một động từ làm việc như write, make, fix, list hay
  look (`WORK_VERBS`), hoặc ít nhất `WHOLE_ASK_FEWEST` (4) từ nhờ việc
  ngoài các từ đồng ý, đếm như `new_ask`, `agent_tasks::asks_for_work`;
  nên "Hermes, what time is it?", "Hermes, how are you?", "Hey Hermes,
  tell me a joke." và "Codex, what can you do?" không nêu ai và được trả
  lời như bình thường, còn "Hermes, write a poem about rain into rain.md"
  nêu Hermes; dạng "ask/have/tell Hermes to …" vẫn tính như trước; P2 của
  Thirteen trên #6572), có lời của vòng 1 bị giữ đến hết vòng (`agent_tasks::named_for_work`,
  `Engine::catching_named_agents`). Nếu vòng 1 chỉ trả lời bằng lời, không
  gọi hàm nào, lời đó không được nói: lượt được hỏi lại một lần với một ghi
  chú hệ thống nêu tên agent ("the person asked Hermes to do this … Call
  start_agent_task now to hand the work to Hermes; never do it yourself"),
  và journal ghi `request N: named agent, no start_agent_task: asked
  again`, rồi kết quả (`…: asked again: start_agent_task called`, `…: still
  no call, said it wasn't done`, hoặc `…: called <tên>, not start_agent_task`).
  Nếu vòng 1 chỉ gọi các công cụ riêng của đèn ("Hermes, look up the
  weather" được trả lời bằng `weather`: P1 của V2 trên 12587), không công
  cụ nào được chạy: mỗi lệnh gọi được trả lời là không chạy, lời không được
  nói, lượt được hỏi lại một lần với cùng ghi chú, và journal ghi `request
  N: named agent, called <tên> not start_agent_task: none run, asked again`
  (`agent_tasks_named::named_answered_by_an_own_tool_runs_none_and_is_asked_again_once`;
  khi không nêu agent nào, công cụ riêng chạy như trước). Các công cụ việc
  của agent (`confirm_agent_task`, `check_agent_task`, `tasks_now`, đọc trạng
  thái) không phải công cụ riêng: "Hermes, go ahead and run it" được trả lời
  bằng `confirm_agent_task` thì chạy như đã gọi, không hỏi lại (P1 của
  Reviewer-lead trên #6786,
  `agent_tasks_named::an_agent_task_tool_with_the_agent_named_runs_as_called`).
  Lần bắt này cũng chờ hết cửa sổ nối câu bên dưới; câu mới bắt đầu trong đó
  được journal ghi `…: a newer line began within the join window: none run,
  not asked again`. Chỉ hỏi lại khi cửa sổ nối câu (`JOIN_WITHIN`, 3 s tính từ lúc bắt đầu
  trả lời, khoảng cuối câu nói) đã qua mà không có câu mới nào bắt đầu:
  nửa sau của một lời nhờ có ngắt nghỉ, bắt đầu trong cửa sổ đó, được nối
  vào nửa đầu (`LAMP_JOIN_CUT_WORDS`), và nửa đầu không bao giờ bị hỏi lại
  hay giao việc một mình (P1 của W3 trên #6572: "Make a file called
  beach.md." đã được giao, phần còn lại đến sau 3392 ms và không được
  nối). Câu mới bắt đầu trong cửa sổ sẽ kết thúc lượt này, journal ghi
  `…: a newer line began within the join window: not asked again`. Mỗi
  request chỉ một lần (`agent_tasks::AskedAgain`, giữ 8 request gần nhất):
  lời của vòng 2 cũng bị giữ, chỉ nói khi có gọi hàm; nếu vẫn chỉ trả lời
  bằng lời, lời đó không được nói mà đèn nói câu rõ ràng "Sorry, I didn't
  do that. Could you ask me again?" (`action_claim::NOT_DONE_LINE`, theo
  ngôn ngữ của chủ đèn). Request đã hỏi lại trước đó được nói kèm dòng
  `…: asked again before, said as it came`; không bao giờ lặp. Vòng 1 có gọi bất kỳ hàm nào (`start_agent_task`,
  `not_for_me`, một skill) thì như trước, lời của nó được nói khi hết vòng;
  câu không nêu agent nào thì không bị đụng tới và vẫn phát luồng như
  trước. Route Live không được bắt. Test:
  `second_provider::tests::agent_tasks_named`
  (`named_with_no_start_is_asked_again_once`,
  `named_with_no_start_twice_says_it_was_not_done`,
  `named_with_start_is_untouched`,
  `not_named_and_a_question_about_an_agent_are_untouched`,
  `a_request_asked_again_before_is_not_asked_again`,
  `a_paused_ask_is_joined_and_never_asked_again_half_said`),
  `agent_tasks::tests::a_named_agent_is_the_doer_and_asked_again_once_a_request`.
- **Đọc lại việc mà không gọi hàm: hỏi lại một lần (#6547 C).** Trên 52e6
  ngày 12/11 đèn hay nói "I'll have Hermes on the lamp … Shall I start
  it?" mà không gọi hàm, nên chưa có gì được chuẩn bị. Ở route thứ hai,
  khi có phần tasks, một vòng 1 được nói ra không gọi hàm nào và câu trả
  lời là một lời đọc lại việc (`agent_tasks::speaks_a_read_back`: kết thúc
  bằng một câu hỏi có "start", "begin", "hand", "send", "go" hoặc "kick",
  và nêu một agent mà voice biết, "agent", hoặc máy tính; "Shall I start a
  timer?" thì không phải) cũng được hỏi lại như trên, dù có nêu tên agent
  hay không, sau cùng cửa sổ nối câu, mỗi request một lần
  (`agent_tasks::AskedAgain`). Lời của nó đã được nói khi phát luồng
  (người nghe đã nghe lời đọc lại và có thể nói yes trong cửa sổ; tiếng
  yes đó là câu mới và kết thúc lượt này), nên ghi chú
  (`agent_tasks::read_back_no_start_note`) bảo gọi `start_agent_task` với
  đúng việc đã đọc lại và không nói thêm gì trừ khi không chuẩn bị được;
  lời bị giữ (vòng bị giữ vì lý do khác) thì không được nói. Journal ghi
  `request N: read back, no start_agent_task: asked again` và kết quả như
  trên; câu trả lời chỉ bằng lời lần thứ hai không được nói, đèn nói
  `NOT_DONE_LINE`. Test: `second_provider::tests::agent_tasks_named`
  (`a_words_only_read_back_is_asked_again_for_the_call`,
  `a_read_back_twice_says_it_was_not_done_and_never_loops`,
  `an_answer_that_is_no_read_back_is_untouched`),
  `agent_tasks::tests::a_read_back_is_told_from_other_answers`.
- **Lời nhờ mới khi đang có việc chờ thì thay việc đó (#6547 B).** Trước
  đây `pending_note` bảo model không gọi lại `start_agent_task` khi có việc
  đang chờ (`CONFIRM_WITHIN`, 120 s), nên lời nhờ mới chỉ được đọc lại và
  tiếng yes khởi động việc cũ. Giờ ghi chú đọc chính lời của câu: câu không
  phải yes đơn thuần (`plain_yes`), không phải no đơn thuần
  (`services::plain_no`) và có ít nhất `WHOLE_ASK_FEWEST` (4) từ nhờ việc
  (`ask_words`, như của #6527) ngoài các từ đồng ý (`CONSENT_WORDS`: yes,
  yeah, yep, yup, sure, ok, okay, go, ahead, do, it, start, begin, send,
  hand, over, sounds, good, great, fine, please, now, right, away) là một
  lời nhờ mới. "Yes, go ahead and start it now.", "Yeah sure, go ahead and
  do it.", "Yes, sounds good, start it.", "Okay, yes, please start that."
  và "Yes please, hand it over." không phải: mỗi câu đều khởi động việc
  đang chờ, dù `LAMP_TASKS_ONE_YES` bật hay tắt (P2 của Heron trên #6572).
  Ghi chú của lời nhờ mới nói một lần gọi `start_agent_task` với lời nhờ đó
  sẽ thay việc đang chờ, việc cũ sẽ không chạy. Không có gì được đánh dấu
  khi dựng ghi chú, trước khi model đọc câu: chỉ một `start_agent_task`
  trong chính request của lời nhờ mới mới thay việc đang chờ, có hay không
  có `LAMP_TASKS_CORRECT`, và hủy nó lúc đó bằng op `cancel` của phần
  tasks, như khi sửa lời nhờ (journal `lamp-voice: tasks: request N: a
  corrected or new ask replaces task <id>, which this voice no longer
  confirms`); tiếng yes sau đó chỉ khởi động việc mới. Trước khi có lần gọi
  đó, việc đang chờ vẫn giữ tiếng yes của nó: một câu hỏi không phải việc gì
  ("What's the weather going to be like in Hanoi today?"), được trả lời mà
  không gọi hàm, rồi "yes" vẫn khởi động việc đó (P3 của Heron trên #6572).
  Yes, no hay câu ngắn ("What was that?") giữ ghi chú như trước. Test:
  `agent_tasks::tests::a_new_ask_while_a_task_waits_replaces_it_and_the_yes_starts_the_new_one`,
  `agent_tasks::tests::a_question_while_a_task_waits_leaves_its_yes`,
  `agent_tasks::tests::a_yes_said_at_length_starts_the_waiting_task`.
- **Một tiếng yes cho lời đọc lại chỉ bằng lời (#6547 A, bật trong sản
  phẩm, không có công tắc tắt).** Một tiếng yes đáp lại lời đọc lại chỉ
  bằng lời của đèn, ngay trong lượt trao đổi đó và trong `CONFIRM_WITHIN`
  (120 s), sẽ chuẩn bị và khởi động việc: `confirm_agent_task` khi chưa có
  gì được chuẩn bị trả lời "Nothing is prepared yet: call start_agent_task
  now with the task you said back, then call confirm_agent_task again."
  khi đó là một tiếng yes đơn thuần nghe ngay sau câu hỏi của đèn trong
  120 s và không bị cắt (`agent_tasks::yes_to_a_question`), và quy tắc
  cùng lượt (`yes_in_the_same_turn`, #5169) áp dụng dù có hay không có
  `LAMP_TASKS_ONE_YES`, với mọi giới hạn của nó: chỉ yes cho câu ngay
  trước đó của đèn, câu đọc lại đủ việc này. Yes cho một câu cũ hơn, quá
  120 s, hoặc sau khi bị cắt không khởi động gì mới. `LAMP_TASKS_ONE_YES`
  giờ chỉ đổi mô tả của `start_agent_task` (gọi hàm trước) và lời của
  `task`. Test: `one_yes_to_a_words_only_read_back_prepares_and_starts_it`,
  `one_yes_counts_only_inside_the_exchange_and_the_window`.
- **Không có máy tính trong `where` khi chưa ghép đôi (#6547 (1c)).** Khi
  chưa ghép máy tính nào (không có staff: cùng phép thử mà phần mô tả đã
  dùng), `enum` của `where` chỉ còn `["lamp"]` và phần mô tả của nó không
  nhắc tới máy tính; khi có máy ghép đôi thì `["lamp", "computer"]` như
  trước. Một lần giao việc vẫn nhắm tới máy tính trong trạng thái đó
  (`where` `"computer"`, do lời nghe được đặt từ "on my computer", "on my
  Mac" hay tên một agent của máy tính, hoặc một `who` không phải đèn cũng
  không phải engine của đèn) bị từ chối, không chuẩn bị gì, và model được
  báo là chưa ghép máy tính nào, hãy hỏi người dùng có muốn agent của đèn
  làm không: không bao giờ lặng lẽ thành việc của đèn (P1 của V2 trên
  12587, row-6547 của Fifth: claude, garbled, doc, mac); khi công tắc
  `LAMP_TASKS_WHO` tắt, nơi staff không bao giờ được đọc, nó đi tới target
  đã cấu hình như trước. Khi chưa có lần đọc staff nào trả về (chưa đọc
  lần nào, hoặc mọi lần đến giờ đều lỗi), staff rỗng, tức là coi như chưa
  ghép máy nào: `computer` bị bỏ khỏi `where`. Một lần đọc lỗi sau đó giữ
  staff của lần đọc gần nhất. Test:
  `agent_tasks::tests::where_offers_no_computer_without_a_pairing`,
  `agent_tasks::tests::no_computer_paired_a_computer_asked_for_is_refused_never_the_lamps`,
  `agent_tasks::tests::new_work_goes_to_the_lamp_unless_a_computer_is_named`.
- **Khởi động** như Harness khởi động OpenClaw:
  `openclaw tui --session harness-<agentId> --message "<prompt>"`, không có
  chế độ quyền và không định tuyến model (OpenClaw tự chọn model, theo phần
  cài đặt bên dưới).
- **Rảnh mà chưa được hỏi vẫn tính là đã giao** (#6499): sau một `new`
  thành công, lamp-tasks đọc `show` cho đến khi harness được hỏi hoặc rảnh,
  trong cùng `launch_timeout`. Cả hai trường hợp, việc đều là `Created`, như
  Harness đã trả lời: trên đèn, `show` của OpenClaw có thể báo rảnh và chưa
  có `ask` vài giây sau một `new` mà TUI thật ra đã nhận `--message`. Rảnh mà
  chưa được hỏi chỉ ghi một dòng journal, ngoài ra không làm gì: không đóng
  harness, không gọi `new` lần nữa, không gõ vào bằng `harness reply`.
  `show` lỗi thì kết quả là uncertain, kèm mã lỗi. Lỗi mất câu hỏi thật sự,
  khi OpenClaw bỏ mất `--message` lúc TUI ghép đôi với một Gateway còn
  nguội, đã được sửa trong Harness 0.6.23 (autonomous-ai/openharness#4172:
  Harness khởi động OpenClaw không kèm `--message` và gõ task vào khi pane
  đã sẵn sàng); đèn có bản sửa này khi pin Harness được dời lên (#5957). (Cách
  tạm thời của #6504, đóng rồi khởi động lại, đã bỏ.)
- **Chưa bao giờ bắt đầu: báo và đóng** (#6577): một việc giao cho OpenClaw
  của chính đèn, trong lần khởi động này, mà sau khi được nhận
  `OPENCLAW_BEGIN_WITHIN_MS` (130 s: lần câu hỏi đầu chậm nhất thật đã thấy,
  65,1 s trên lamp-fc4b với Harness 0.6.16 ngày 12/11, nhân hai; số đo với
  0.6.41 sẽ có sau) vẫn đọc là rảnh,
  chưa được hỏi, chưa có câu trả lời và chưa dùng công cụ nào, và mọi lần đọc
  từ đó cũng vậy, thì được giữ là `failed` (`engine::never_began`, một dòng
  journal). Giọng nói báo nó như mọi việc lỗi, một câu ngắn rằng việc đã dừng
  khi chưa xong, và harness trống được đóng như một việc của đèn đã báo
  (`close_due`). Không thử lại, không chuyển sang Hermes (quy tắc của lead).
  Việc đã được hỏi, đã trả lời hoặc đã dùng công cụ trước giới hạn, việc từ
  lần khởi động trước, phiên của chính người dùng, Hermes và các agent trên
  máy tính đều như trước.
- **Voice** (`agent_tasks`, `task_news`): đọc engine của target của đèn từ
  `{"op":"targets"}` và gọi đúng tên, nên một việc giao không nêu máy nào được
  nói là "OpenClaw on the lamp" và mô tả công cụ nói "OpenClaw here on the
  lamp"; nếu là `hermes` thì "Hermes" như trước. Khi chưa đọc targets, voice
  nói Hermes, mặc định (#6498). `who`
  "OpenClaw" hoặc "Hermes", khi đèn có target của mình, là của đèn, không bao
  giờ của máy tính. OpenClaw cũng là một agent mà voice có thể gọi tên trên
  máy tính, nghe được là "OpenClaw", "open claw", "open clause", "open
  close", "open clow" hoặc "open klaw". Tin về việc do engine nào làm trên
  đèn cũng được kể là làm ngay tại đèn.
- **Chỉ thấy trong test:** `agent_tasks::tests::new_work_goes_to_openclaw_on_the_lamp`,
  `agent_tasks::tests::with_the_lamps_target_the_hand_over_says_what_goes_where`,
  `agent_tasks::tests::with_named_only_the_words_name_an_agent_not_set_up`,
  `task_news::tests::work_done_by_openclaw_on_the_lamp_is_told_as_done_here`.
  Chưa thấy trên đèn.

## Cài đặt của OpenClaw trên đèn (#6226)

Khi `LAMP_OPENCLAW_ON_LAMP` bật, lamp-tasks lúc khởi động gộp những gì
OpenClaw trên đèn cần vào `<state>/openclaw.json`
(`/var/lib/lampos-openclaw/state`, JSON5), đúng như `hermes_config` làm cho
Hermes, và giữ mọi khóa khác (trong đó có khối `gateway` của lamp-update). Đây
là phần của một coder khác trong #6226; thiết kế:

- `models`: `mode: "merge"`, với provider `lampos`: `baseUrl`
  `http://127.0.0.1:7741/v1/deepseek` (forwarder của lamp-cloud, docs/cloud.md),
  `apiKey` `"${LAMP_HERMES_TOKEN}"` (wrapper export nó từ file token của
  forwarder), `api` `"openai-completions"`, và một model, `deepseek-flash`
  (model duy nhất của forwarder). Target loopback có thể cần
  `request.allowPrivateNetwork: true` trên provider.
- `agents.defaults.model.primary`: `"lampos/deepseek-flash"`.
- `agents.defaults.workspace`: thư mục làm việc riêng của đèn
  (`/var/lib/lampos-tasks/harnesses/lamp-tasks/lamp`, `LAMP_PROJECT` trong
  home của `lampos`), nơi các khóa skill của đèn trỏ tới và hướng dẫn
  (`AGENTS.md`) được ghi, như với Hermes.

## Hook của Harness bên trong namespace agent của đèn (#6530)

Hook của Harness cho một engine (`harness hook hermes session-start`,
`harness hook hermes tool`) chạy bên trong namespace agent của đèn, nơi CLI và
trạng thái của Harness bị ẩn. Phần namespace dùng chung
(`crates/update/src/agent_ns.rs`, "một namespace cho mọi engine" của #6510,
hiện được namespace của Hermes và OpenClaw dùng) đặt một `harness` thay thế lên
đầu `PATH` của namespace: `/usr/local/lib/lampos/ns-bin/harness`, của root,
0755, do lamp-update ghi. Nó trả lời `harness hook …` bằng không gì cả và mã
thoát 0, từ chối mọi lệnh khác, và không lộ trạng thái nào của Harness. Mỗi lần
engine khởi động ghi một dòng journal: "Harness hooks aren't available inside
the lamp's agent namespace yet (openharness#4194)". Vì vậy, tạm thời Harness
không theo dõi được phiên của engine chạy trong namespace qua hook. Bản thay
thế này sẽ được gỡ khi Harness được ghim có chế độ hook cho sandbox
(openharness#4194) (docs/install.md, "Hermes on the lamp").

## Đèn chưa đăng nhập vẫn giao việc cho engine của chính nó (#6407)

Một đèn product chưa từng đăng nhập Harness, đã cài Hermes 0.22.2, trả lời
"I can't reach Hermes": target `lamp` chỉ được ghi từ `harness machines`, mà
lệnh đó chỉ chạy khi đã đăng nhập. Harness 0.6 khi chưa đăng nhập từ chối
`machines` và mọi `--machine`, còn `start` và `new` không có `--machine` vẫn
chạy trên chính máy này, dưới id riêng của nó (`computerId`, một UUID, mà
`auth status --json` vẫn trả khi chưa đăng nhập; id máy của tài khoản là 32 chữ
số hex). Vì vậy, khi bật `LAMP_OPENCLAW_ON_LAMP` hoặc `LAMP_HERMES_ON_LAMP`
và có `--targets-from-machines`:

- **Target lấy từ chính đèn.** Mỗi lần đọc sign-in thấy chưa đăng nhập và có id
  của máy này, engine của đèn coi là đã cài khi chương trình của nó nằm ở chỗ
  Harness tìm, `/usr/local/bin/<engine>` (`LAMP_ENGINES_BIN`; `hermes` và
  `openclaw` là wrapper của lamp-update), chạy được, và là wrapper do
  lamp-update ghi: 512 byte đầu có `Written by lamp-update.`
  (`LAMP_WRAPPER_MARK`, #6506). Khi đó file target được ghi như khi ghi
  từ danh sách máy, không bao giờ đè file mà phần tasks không tự ghi, chỉ có
  đèn: `lamp`, engine đó, `~/harnesses/lamp-tasks/lamp`, dưới id của máy này.
  Target của các máy tính ghi trước đó bị bỏ, vì chưa đăng nhập thì không tới
  được. Chưa cài thì target ghi kiểu này trước đó bị xoá; file khác giữ
  nguyên. Engine là engine đầu tiên theo `lamp_order` (đang bật, theo
  `LAMP_LAMP_ENGINE`: Hermes trước nếu không đặt, #6498; bỏ OpenClaw khi Harness
  quá cũ) mà đã cài ở đây; không ghi cứng Hermes hay OpenClaw.
- **Không xem danh sách máy, không `--machine`.** Target mang id của máy này
  được giao không cần `machines --json` trước, và mọi lệnh cho việc của nó
  (`new`, `show`, `ps`, `wait`, `watch`, `reply`, `resume`, `close`, `route`)
  bỏ `--machine`; dòng journal vẫn ghi máy.
- **Daemon lúc khởi động.** Lần đọc sign-in đầu tiên thấy chưa đăng nhập cũng
  khởi động daemon của Harness, khi bật target của đèn.
- **Đăng nhập lại** thì danh sách máy ghi target như trước, và target của đèn
  mang id máy của tài khoản. Việc đã bắt đầu dưới id máy của tài khoản chỉ đọc
  được khi đã đăng nhập; các máy tính vẫn cần đăng nhập.
- **Lời nói.** Dòng health nói `computers_need_sign_in` ("Harness is not signed
  in: tasks go to the lamp's own agent; sign in to hand tasks to your
  computers") thay cho `not_signed_in`, và `harness_status` của giọng nói nói
  như trên.
- **Test:** `a_lamp_never_signed_in_hands_work_to_its_own_engine`,
  `the_lamps_own_target_signed_out_is_asked_with_no_machines_look_and_no_machine`,
  `signed_out_the_computers_own_id_is_read_with_the_sign_in`.
- **Chưa thử trên đèn:** chưa đăng nhập, "ask Hermes …" được trả lời và câu hỏi
  tiếp theo tới đúng session đó.

## Engine của target mỗi máy: engine chạy được trước (#6657)

Khi target được ghi từ danh sách máy (`--targets-from-machines ENGINE`), mỗi
máy tính có một target, và engine của nó là một engine được biết đã cài trên
máy đó, chọn theo việc có chạy được không (phương án (b) của first builder,
không có công tắc; `readiness::Known::chosen`): trước hết engine được biết đã
đăng nhập, rồi engine chưa biết đăng nhập hay chưa, rồi engine được biết chưa
đăng nhập; trong mỗi nhóm, `ENGINE` trước, rồi claude, codex và opencode. Vì
vậy một engine được biết chưa đăng nhập, mà lần giao việc sẽ bị từ chối
(`engine_not_signed_in`), không bao giờ thắng một engine chạy được: trên một
laptop có claude đã cài nhưng chưa đăng nhập và codex đã đăng nhập, target là
codex, và quay về claude ở lần xem đầu tiên sau khi claude đăng nhập. Không
engine nào được biết đã cài thì vẫn là `ENGINE` như trước. File do người khác
ghi vẫn không bao giờ bị ghi đè. Test: `the_engine_chosen_is_one_that_can_run`.

## Mở hoặc hỏi một phiên đã có theo tên (#6274, tắt trừ khi `LAMP_TASKS_REPLY_NAMED=on`)

Người dùng hay nói "mở phiên customer survey" hoặc "phiên customer survey đến
đâu rồi", nhưng không nói việc gì cần làm. Trên lamp-c0a4, mô hình không gọi
hàm nào (`calls=none`): `start_agent_task` bắt buộc có `task`, còn
`check_agent_task` chỉ biết việc đèn đã giao. Vì vậy `named_session` của
lamp-tasks không bao giờ chạy. Khi bật cả hai công tắc (`LAMP_TASKS_REPLY_NAMED`
của giọng nói và `lamp-tasks --reply-named`):

- **Chỉ cho chủ đèn.** Chỉ chủ đèn, khi chính họ nói ra, mới được mở hay
  hỏi một phiên, giống cách các dịch vụ kiểm tra một lần đọc
  (`services::owner_of`, `agent_tasks::Asker`; trong phòng riêng thì ai nói
  cũng được). Người khác trong phòng được trả "only the lamp's owner can
  open or ask about their sessions", và một dòng không ai nói (chỉ mô hình
  tự gọi) cũng bị từ chối. Khi đó lamp-tasks không được hỏi gì, nên không
  lộ tên, trạng thái hay việc của phiên nào (P1 của Reviewer-lead trên
  #6422).

- **Yêu cầu.** Giọng nói gửi lên socket riêng của giọng nói:
  `{"op":"session","look":"voice-<request>-look-<n>","target":"<bí danh>","session":"<lời người nói>","join":true|false}`.
  `target` được chọn như khi giao việc: target của chính đèn nếu việc làm ở
  đèn, nếu không thì trợ lý được gọi tên, nếu không thì mặc định. Yêu cầu trả
  lời ngay `{"ok":true,"look":…,"done":false}`, vì mọi yêu cầu phải trả lời
  trong 1,1 s, còn liệt kê các phiên của người dùng mất vài giây. Việc tìm
  chạy trên một lane riêng (`SessionLane`, `Cli::session_lane`), cạnh lane giao
  việc, tối đa 4 lần cùng lúc, mỗi câu trả lời giữ 60 s. Giọng nói hỏi lại
  bằng cùng `look` mỗi 250 ms, tối đa 16 s (`LOOK_WAIT`; giới hạn của
  lamp-tasks là `LOOK_BOUND`, 15 s). Việc hỏi lại dừng (`cancelled`) ngay
  khi lượt nói kết thúc: phạm vi bộ nhớ của yêu cầu bị thu hồi khi lượt hết
  hoặc khi người dùng nói chen vào. Route không có phạm vi thì chỉ dừng theo
  các giới hạn trên. Xong thì có `"found"` (`state`, `name`,
  `ask`, `about` cắt ở một từ, tối đa 160 ký tự, `last_active_unix_ms`,
  `resumed`, `on`) hoặc `"code"`. `Cli::look_session` tìm phiên giống hệt lúc
  giao việc vào một phiên có tên (`find_named`). Nó không gõ gì và không
  route. Điều duy nhất nó có thể khởi động là mở lại một phiên đã đóng được
  gọi đúng tên, như dưới đây.
- **Mở (`join`).** Đây là `start_agent_task` có `session` mà không có `task`,
  và chỉ chạy khi người dùng tự nói ra. Một phiên đang chạy và rảnh thì được
  mở như đang có. Phiên đang bận thì nói là bận. Phiên đã dừng và được gọi
  đúng tên thì được mở lại bằng `harness resume` khi bật
  `LAMP_TASKS_RESUME_CLOSED`, việc này khởi động lại agent của nó trên máy
  đó; nếu không thì trả `session_stopped`. Phiên đã dừng mà chỉ tìm ra nhờ
  tên nói gần đúng thì không bao giờ được mở lại: đèn nói đó là phiên gần
  nhất, đã đóng, và hỏi người dùng có phải phiên đó không, muốn mở lại thì
  nói đúng tên của nó. Mô hình nhận `{"open":true,"session":…,"where":…,"next":…}`.
  `next` bảo nói rằng phiên đã mở và hỏi phiên nên làm gì. Điều người dùng nhờ
  tiếp theo sẽ đi vào phiên đó qua `start_agent_task` kèm session ấy, được đọc
  lại và chờ một tiếng "có" như mọi lần giao việc vào phiên có tên.
- **Hỏi (không `join`).** Đây là `check_agent_task` có `session`. Phiên khớp
  được nói đúng như nó đang thế nào, kể cả phiên đã dừng, và không mở lại gì.
  Cách này dùng được cả với phiên đèn chưa từng giao việc. Mô hình nhận
  `session`, `where`, `state` bằng lời thường, `doing` (`{"untrusted": …}`,
  đường dẫn thay bằng "a file") và `last_active`.
- **Không đoán.** Không phiên nào khớp thì nói thẳng. Nhiều phiên khớp thì hỏi
  lại người dùng. Phiên đã đóng, phiên bận, máy tính offline và danh sách không
  đọc được thì mỗi trường hợp có vài lời riêng.
- **Tên nói gần đúng.** Chỉ khi xem một phiên (không bao giờ khi giao việc
  gõ vào phiên), một từ bị nghe nhầm hoặc viết sai vẫn tìm ra phiên gần
  nhất (`near_session`, `near_name`), ví dụ "costumer survey" hay "survay"
  cho "Customer Survey". Cách này chỉ dùng khi trên mọi máy, tên, thư mục và
  việc đã làm đều không khớp: mọi máy được đọc trước, nên một phiên gọi
  đúng tên trên máy tính luôn thắng một phiên gần đúng trên đèn ("test":
  "Test runner" của Mac, không phải "Text formatter" của đèn). Câu trả lời
  có `"near":true`, và giọng nói nêu tên phiên tìm được để người dùng biết
  là phiên nào. Khi giao việc gõ vào một phiên, chỉ tên đúng mới được nhận,
  vì câu đọc lại dùng lời của người dùng nên tiếng "có" không bắt được một
  lựa chọn gần đúng (P2 của Reviewer-lead trên #6422).
  Mỗi từ nói ra phải khớp với một từ của tên, theo đúng thứ tự: hoặc là phần
  đầu của từ đó, hoặc là cả từ nhưng lệch một chút (thiếu, thừa hay sai một
  chữ, hoặc hai chữ cạnh nhau bị đảo). Từ 4–7 chữ được lệch tối đa 1 chữ, từ
  từ 8 chữ trở lên được lệch tối đa 2. Từ dưới 4 chữ thì không được lệch.
  Phiên có ít chữ lệch nhất được chọn, hai phiên gần ngang nhau, trên một
  máy hay hai máy, là `session_ambiguous`. "api" vẫn không bao giờ khớp "rapid-prototype".
- **Schema.** Khi bật công tắc, `start_agent_task` không có `required`: chỉ có
  session là đủ, không có cả hai thì trả "Say what the work is.". Mô tả của
  `session` thêm "Sent with no task, it opens it.".
  `check_agent_task` thêm `session` và một câu. Khi tắt, cả hai giữ nguyên từng
  chữ, kể cả `required: ["task"]`.
- **Test:** `tests/harness.rs` `a_name_said_nearly_right_finds_the_one_closest_session`,
  `a_name_as_it_is_on_a_computer_beats_a_near_one_on_the_lamp`,
  `a_near_name_never_reopens_a_closed_session`,
  `a_named_session_is_looked_at_to_open_or_say_how_it_is_with_nothing_typed`;
  `tests/runtime.rs` `a_look_at_a_named_session_answers_at_once_and_is_asked_again_for_what_it_found`;
  `agent_tasks::tests::with_reply_named_a_session_alone_opens_it_and_needs_no_task`,
  `agent_tasks::tests::with_reply_named_a_check_names_a_session_and_is_told_its_own_state`,
  `agent_tasks::tests::a_named_session_is_opened_or_told_only_to_the_lamps_owner`,
  `agent_tasks::tests::a_look_stops_asking_again_once_its_turn_is_over`.
- **Chưa chạy trên đèn:** cả luồng với giọng phát từ Mac, và người dùng phải chờ
  bao lâu khi các phiên được liệt kê. Ngày 10/11 trên lamp-c0a4, lần tìm phiên
  khi giao việc mất 15,2 s vì có hai máy offline. Một lần tìm phiên ở đây cũng
  đọc đúng những danh sách đó.
- **Các phiên do đèn khởi động, giữ theo tên** (#6274, quyết định của
  builder đầu tiên ngày 11/11; đi cùng `serve --reply-named`, không có công
  tắc riêng). `harness ps --all` chỉ liệt kê phiên đang mở hoặc đã dừng trong
  7 ngày qua, nên "mở phiên Christmas dates" cho một phiên cũ hơn do đèn
  khởi động thì không tìm thấy (`session_not_found`). lamp-tasks giữ một chỉ
  mục các phiên nó đã khởi động (`new`, id nằm ở `agent.id`), đã gõ vào (giao
  việc vào phiên có tên, phiên router chọn) hoặc đã mở (xem có `join`), trong
  file `sessions-started.json` của thư mục tasks (`--dir`,
  `SESSION_INDEX_FILE`, `crate::session_index`): theo từng máy, mỗi phiên chỉ
  có `id`, `name` và `last_used` (unix ms), không gì khác, không bao giờ có
  câu hỏi, câu trả lời, `about` hay thư mục của nó. Tên của phiên mới khởi
  động là tên do đèn đặt, nên chưa giữ tên nào cho đến khi một danh sách cho
  thấy tiêu đề của nó: mỗi lần lamp-tasks đọc `harness ps` (các danh sách khi
  tìm theo tên, lần đọc tiến độ) và mỗi lần `show` dưới đây đều cập nhật tên
  hiện ra cho phiên có trong chỉ mục. Tối đa 200 (`MOST_INDEXED`) cho mọi máy
  cộng lại; thêm một phiên thì phiên lâu không dùng nhất bị bỏ. File được ghi
  trọn cạnh file cũ rồi đổi tên đè lên, quyền 0600, không bao giờ ghi qua
  link; file không đọc được thì coi như trống.
  - Dùng khi tìm theo tên (`find_named`), chỉ khi không dòng nào trong danh
    sách của các máy đã đọc khớp theo tên, thư mục hay việc đã làm: các phiên
    trong chỉ mục trên những máy đó mà danh sách không có được so bằng đúng
    các quy tắc tên ấy (`by_name`: tên đúng như nói, rồi mỗi từ nói ra là
    phần đầu một từ của tên, theo thứ tự), máy của đèn trước. Mỗi phiên khớp
    được đọc theo id, `harness show --machine <m> <id> --json` (tối đa 4,
    cùng lúc, trong phần thời gian còn lại trừ phần dành cho reply), để biết
    trạng thái và tên. Phiên Harness nói không còn nữa (`UNKNOWN_HARNESS`,
    `GONE`) bị bỏ khỏi chỉ mục và việc tìm tiếp tục; phiên không đọc được
    thì bỏ qua (`sessions_failed` nếu không gì khác khớp). Phiên còn đó theo
    đúng quy tắc của phiên có trong danh sách: rảnh thì vào, bận thì
    `session_busy`, đã dừng thì chỉ được mở lại theo id bằng `harness resume`
    khi bật `LAMP_TASKS_RESUME_CLOSED` và chỉ khi gọi đúng tên (nếu không thì
    `session_stopped`), sau đó việc giao được gõ vào; hai phiên khớp là
    `session_ambiguous`, không bao giờ đoán. Khi xem (không phải khi giao việc
    gõ vào phiên), tên nói gần đúng được thử với các tên trong chỉ mục như với
    danh sách (`nearest_rows`): phiên của chỉ mục chỉ được chọn khi gần hơn
    mọi phiên trong danh sách, gần ngang nhau là `session_ambiguous`, và phiên
    tìm ra kiểu này không bao giờ được mở lại. Một lần tìm không thấy gì trong
    danh sách tốn thêm một lần `show` (khoảng 2,3 s trên lamp-c0a4, 10/11) cho
    mỗi phiên khớp trong chỉ mục, đọc cùng lúc; không phiên nào khớp thì không
    tốn thêm gì.
  - Xóa: nút reset của đèn xóa `/var/lib/lampos-tasks/store` (kế hoạch reset
    của sản phẩm, `crates/setup/fixtures/reset-plan-product.json`), chính thư
    mục chứa file. Lệnh "quên hết" của chủ đèn (được guard của route thứ hai
    đánh dấu, bộ nhớ của họ đã được xóa, giống nhật ký được giữ) hỏi lamp-tasks
    trên socket của giọng nói `{"op":"forget_sessions"}`
    (`agent_tasks::forget_sessions_with_everything`); lamp-tasks xóa file và
    trả `{"forgotten":true|false}`. Câu trả lời giọng nói nói với người dùng
    vẫn như cũ. Một lần hand-over hoặc xem session còn đang chạy lúc lệnh
    quên đến có thể ghi lại đúng session đó sau lệnh quên: đó là một lần
    dùng sau khi quên, như mọi lần dùng sau đó.
  - Khi đăng xuất: khi `harness auth status` đọc ra là đã đăng xuất
    (`Cli::signed_in_here`), index bỏ các session của máy trong tài khoản, vì
    lần đăng nhập sau có thể là tài khoản khác. Index giữ các session trên id
    riêng của máy này, là id Harness dùng để chạy việc khi chưa đăng nhập
    (`session_index::forget_machines`). Khi đã đăng nhập hoặc offline, index
    giữ hết.
  - Khi gỡ ghép đôi: mỗi lần đọc danh sách máy của tài khoản
    (`Cli::machines`), index bỏ các session của máy không còn trong danh sách,
    vì máy đó đã bị gỡ ghép đôi trong app. Các máy còn trong danh sách và id
    riêng của máy này vẫn được giữ.
  - Test: `tests/harness.rs`
    `signed_out_the_account_machines_sessions_are_let_go`,
    `an_unpaired_machines_sessions_are_let_go`;
    `tests/session_index.rs` `forgetting_machines_keeps_only_those_kept`;
    `tests/harness.rs`
    `an_older_session_not_listed_is_found_by_its_name_through_the_index`,
    `a_listed_session_is_found_as_before_and_the_index_only_adds`,
    `an_indexed_session_that_is_gone_is_dropped`,
    `an_ambiguous_index_match_is_asked_about`,
    `the_index_never_makes_a_near_name_reopen_a_closed_session`,
    `the_lamp_keeps_the_sessions_it_starts_and_their_names_as_listed`,
    `the_index_lives_in_the_tasks_folder_the_reset_wipes`;
    `tests/session_index.rs` `the_index_holds_only_id_name_and_last_use`,
    `the_index_keeps_the_most_lately_used`, `forgetting_removes_the_index`;
    `tests/runtime.rs`
    `forget_sessions_removes_the_index_of_the_sessions_the_lamp_started`;
    `agent_tasks::tests::the_owners_forget_everything_forgets_the_sessions_the_lamp_started`.
  - Chưa chạy trên đèn: mở lại theo id một phiên cũ hơn 7 ngày (Harness
    0.6.12 nói `resume <id>` và `show <id>` tới được phiên đó), và thời gian
    thêm của một lần `show`.

## Đóng harness của chính đèn khi việc xong, chờ kể tin tối đa một phút (#6530)

Với `serve --close-told` (bật trong unit của sản phẩm), lamp-tasks đóng
harness mà đèn mở khi câu trả lời đã được kể. Việc giao cho chính đèn
(target `lamp`: Hermes hay OpenClaw trên đèn, không phải phiên nối trên máy
tính) cũng được đóng khi tin đã kể. Nếu chưa kể, nó được đóng
`UNTOLD_CLOSE_MS` (60 s) sau khi câu trả lời mới nhất được giữ, từ agent
`idle` có câu trả lời, vẫn là lời nhờ của đèn (`closes_untold`). Trên 52e6
(12226), một lượt Hermes xong trong 10 s bị cắt khi đang kể tin
(`user_interrupted`), nên không bao giờ được tính là đã kể, và phiên chat ở
lại hơn 7 phút trong `epoll_wait`.

- **Một phút đó giữ chỗ cho câu nói tiếp:** người dùng nói tiếp ("thêm táo
  vào đó") vẫn nối vào cùng phiên trong lúc ấy (Twelve đọc trên #6536). Câu
  nói tiếp đến sau khi đã đóng thì mở phiên mới.
- **Tin không cần phiên:** voice kể câu trả lời đã giữ trong store, nên việc
  đóng không cắt được tin. Một lần đóng kiểu này không được tính là đã kể
  trong record (`Engine::recent`).
- **Thời gian:** watch xem mỗi 30 s và xem lại sau 10 s để xác nhận. Việc
  chưa kể được đóng khoảng 70 đến 100 s sau lượt cuối, việc đã kể thì đóng
  ngay sau khi kể.
- **Harness trên máy tính** vẫn chờ kể tin xong mới đóng, vì người đó có thể
  đang ngồi ở màn hình.
- **`Cli::close_now`** (`harness close ... --now`, dừng phiên kể cả khi đang
  làm) chỉ dành cho lần chạy skill do chính lamp-tasks mở
  (`HarnessRunner`, #6510 Part 1 bước 2), lúc hết hạn hay khi xong. Việc
  đóng ở trên không bao giờ dùng `--now`.
- **Test:** `the_lamps_own_agent_is_closed_once_done_its_news_untold`. Đang
  làm, hay idle chưa có câu trả lời: để mở. Xong mà chưa kể: để mở dưới một
  phút, rồi đóng. Trên máy tính: chờ kể.

## Đóng harness khi tin bắt đầu được kể, nghe trọn hay không (#6530)

Voice gửi `told` cho lamp-tasks (socket tin cậy) ngay khi tin của việc bắt
đầu được kể: khi những lời đầu của mô hình tới loa (`TaskNews::begun`), và
một lần nữa khi lượt kết thúc nghe trọn, cho các key thêm vào trong lượt.
Mỗi key chỉ gửi một lần. Lượt bị cắt (Stop, người dùng nói chen, tắt mic)
hay phát có khoảng trống vẫn gửi, miễn lời đã bắt đầu phát; lượt không phát
lời nào (không có âm thanh, hoặc chỉ có thông báo lỗi) thì không gửi, và
harness để mở.

Trong `tasks_now`, `told` của một việc đã xong nghĩa là tin của nó **đã bắt
đầu** được kể, nghe trọn hay không (first builder, 23:14 trên #6059); việc
nghe trọn chỉ quyết định có kể lại hay không (`LAMP_TASK_NEWS_RETELL`).

- **Nghe trọn không còn quyết định việc đóng:** trên một đèn ngày 12/11, việc
  18 idle lúc 21:21:26, tin phát 21:21:32 đến 35 có khoảng trống
  (`audio_written_with_playback_gaps_unscored`, "was not heard whole: its
  harness stays open" lúc 21:21:53). Harness chỉ được đóng lúc 21:22:37,
  71 s sau khi idle. Giờ `told` được gửi khi tin bắt đầu, và việc đóng (đọc
  lại, rồi `harness close`) theo sau. Nghe trọn chỉ còn quyết định tin có
  được kể lại hay không (`LAMP_TASK_NEWS_RETELL`, #4342); việc kể lại đọc
  câu trả lời đã giữ trong store, nên không cần phiên còn mở. Journal ghi
  "was not heard whole (…): begun, so its harness closes" hoặc "was cut
  short (…): begun, so its harness closes".
- **Chỉ việc đã xong mới đóng:** `close_due` chỉ đóng agent `idle` có câu
  trả lời. Việc đang chờ người dùng, đã dừng hay lỗi vẫn để mở như trước.
- **Test:** `news_not_heard_whole_is_never_handed_on`,
  `news_cut_short_is_never_handed_on`,
  `news_heard_whole_is_handed_on_as_the_turn_ends` (coordinator) và
  `news_begun_closes_its_harness_at_once_and_is_still_told_once_more`
  (`task_news`).

## Việc giao là toàn bộ lời nhờ (#6527)

Trên 52e6 ngày 12/11 (row-6361c), câu "have your agent list five fruits that
are good when you're sick and write them to fruits dot md" tới Hermes chỉ còn
"Create a file called fruits.md": mất danh sách và con số, tệp không có loại
quả nào. Với `LAMP_TASKS_ONE_YES`, tham số `task` của `start_agent_task` giờ
đọc "Every part of what the person asked, in their own words: what to do, and
any count, list, topic, file name or place they said. Never shorten it; the
agent works out the rest." (thay cho "A short ask is enough: the agent works
out the details", câu mô hình hiểu là được rút gọn). `start_agent_task` còn
kiểm tra bằng mã (`agent_tasks::whole_ask`), có hay không có công tắc, trước
khi hỏi phần tasks:

- **Lời đã nghe.** Chỉ câu của chính request (`thread::heard_in_own_line`:
  câu được giữ cho nó, hoặc câu trong lượt trao đổi của nó), không bao giờ là
  lời mới nhất nghe được từ lần trả lời trước, vì đó có thể là câu của người
  khác (#6537). Tin
  nhắn gõ, câu do đèn tự viết, hay request không nghe được gì thì không kiểm
  tra; một câu "yes" đơn thuần cũng không, và một câu có hơn một phần tư từ
  nội dung không phải ASCII thuần (lời nhờ tiếng Việt được giao bằng tiếng
  Anh) cũng không.
- **Từ nội dung** (`ask_words`): các chuỗi chữ và số viết thường, bỏ một tập
  nhỏ từ nhờ và từ nối (`NOT_ASK`: "the", "to", "your", "agent", "have",
  "please", …, và các dấu nói "dot", "slash", "underscore", "dash"), các từ
  nơi chốn "lamp", "computer", "mac", "macbook", "laptop", và mọi dạng tên
  agent giọng nói biết (`KINDS`), so cả cụm nên "open" trong "open the file"
  vẫn giữ. Số đọc bằng chữ là chữ số ("five" là "5", `NUMBER_WORDS`), nên
  "fruits dot md" và "fruits.md" là cùng từ; các từ khác so theo gốc. Chỉ
  tính các từ sau tên agent cuối cùng (hoặc chữ "agent"): phần trước đó
  thường là lời nói bâng quơ ("ugh, work was rough today, anyway have Claude
  fix the login bug"), và lời bâng quơ không bao giờ được đẩy vào task gửi
  ra khỏi đèn (#6537, bài đọc quyền riêng tư).
- **Quy tắc.** Khi nghe được ít nhất `WHOLE_ASK_FEWEST` (4) từ nội dung, một
  task giữ dưới `WHOLE_ASK_SHARE` (một nửa) số từ đó bị từ chối: "Nothing
  was prepared: the task looks shorter than what the person asked of the
  agent. If part of the task was left out (a count, a list or a file name),
  call start_agent_task again now with the whole task; leave out anything
  said that is not part of it." Các từ bị bỏ không bao giờ được nêu lại. Mỗi request chỉ từ chối một lần:
  lần gọi thứ hai trong cùng request được cho qua, nên người dùng không bao
  giờ bị kẹt.
