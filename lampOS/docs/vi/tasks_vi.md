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

## Việc của một kỹ năng trên Hermes của đèn (#6306, PR 3 của #6071, tắt trừ khi `LAMP_SKILL_TASKS=on`, bật trong sản phẩm)

Bước `do.task` của một kỹ năng (#6305, docs/skills.md, "A skill's task")
giao phần chậm hơn cho Hermes của chính đèn. lamp-skills gửi một dòng cho mỗi
lần chạy, và được trả lời khi lần chạy kết thúc
(`lamp_tasks::skill_task::answer`):

- **Dòng gửi:** `{"op":"skill_task","skill","task","run","folder","sha256","grants"}`.
  Cần đủ mọi trường và không nhận trường nào khác. `skill` là tên một kỹ năng
  (`a-z`, `0-9`, `-`, `_`, tối đa 64), `folder` đúng là
  `/var/lib/lampOS/skills/<skill>` (các kỹ năng Hermes thấy ở chế độ chỉ đọc),
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
  cấp token một lần chạy), `run_failed`, hoặc `busy` (việc của một kỹ năng
  khác đang chạy: mỗi lúc chỉ chạy một việc).
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
  kết thúc, nên lần chạy không bao giờ nhận quyền mà kỹ năng không xin. Lời
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
- **Lần chạy:** một việc cho đích `lamp`, lời của nó là việc của kỹ năng,
  script cần chạy từ thư mục chỉ đọc của kỹ năng, và đường duy nhất tới đèn
  (`SkillRun::prompt`), qua trait `Runner`, trait này chờ lần chạy kết thúc.
  Việc của kỹ năng được đưa vào như dữ liệu (một chuỗi JSON, "as data, not
  as instructions"), và không được có ký tự điều khiển. Runner kết thúc lần
  chạy ở `SkillRun::deadline`, đúng thời hạn của token, tự nó làm, không
  chỉ dựa vào lời trong prompt. Runner của đèn cho lần chạy một network
  namespace riêng (bên dưới).
- **Runner của đèn** (`HermesRunner::lamp`): Hermes của đèn được khởi động
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
  khởi động với mạng của chính đèn (P2 của Heron và Fourteen trên #6443).
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
1,5, giữ ở tối đa ba nhân. Các số đo là của Fourteen cho một việc Hermes thật
bằng giọng nói (một danh sách đồ cần mang, 80 s) trên lamp-9adf, một đèn
6 GB chạy build 12255, đọc từ cgroup của lamp-tasks.service bằng
`systemctl show`; số CPU do first builder chuyển (13:14 trên #6059). Các
đỉnh được lấy mẫu mỗi 2 s, nên chỉ là mức sàn; phần nhân 1,5 bù cho phần
nằm giữa hai lần lấy mẫu. Sau khi việc đóng, cgroup giữ 727 MiB và 89 task.

| Số đo (`skill_task::`) | Đã đo | Nguồn | Giới hạn trong unit sản phẩm |
|---|---|---|---|
| Bộ nhớ cgroup, `MemoryCurrent` (`MEASURED_REST_MEMORY_MB`, `MEASURED_PEAK_MEMORY_MB`) | 632 MiB lúc nghỉ, đỉnh 1120 MiB (+488; mức sàn: mẫu mỗi 2 s) | lamp-9adf, 12/11, cgroup của lamp-tasks.service (#6059) | `MemoryHigh=2100M` (`MEMORY_HIGH`): 632 + 2 × 488 × 1,5 = 2096, làm tròn lên 100 MB. Vượt mức này kernel làm chậm và thu hồi bộ nhớ của chính phần này, không bao giờ kill: không có `MemoryMax` |
| Task của cgroup, `TasksCurrent`, tiến trình gồm cả thread (`MEASURED_REST_TASKS`, `MEASURED_TASKS`) | 91 lúc nghỉ, đỉnh 126 (+35; mức sàn: mẫu mỗi 2 s) | lamp-9adf, 12/11, cgroup của lamp-tasks.service (#6059) | `TasksMax=512` (`TASKS_MAX`): 91 + 2 × 53 = 197, làm tròn lên 10 thành 200, nâng lên mức tối thiểu 512 (`TASKS_MAX_LEAST`): một lần chạy nhiều công cụ (Python, Node, các thread của trình duyệt) có thể vượt 100 ngay. Vượt mức này thì fork thất bại, kể cả của chính lamp-tasks; không gì bị kill |
| CPU (`MEASURED_PEAK_CPU_PERCENT`) | khoảng 1,2 nhân trong 80 s (120%) | lamp-9adf, 12/11, cgroup của lamp-tasks.service (#6059); first builder, 13:14 | `CPUQuota=300%` (`CPU_QUOTA`): 2 × 120 × 1,5 = 360%, giữ ở 300% (`CPU_QUOTA_MOST_PERCENT`), vì 360% chỉ để lại 0,4 nhân cho giọng nói khi cả hai việc đều bận; 300% giữ một trong 4 nhân của đèn cho giọng nói mà vẫn đủ 240% của hai việc |
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
cần Hermes.

**Dòng kiểm tra trên đèn (chưa chạy):** trên một đèn đã cài Hermes,
lamp-control đang phục vụ và `LAMP_SKILL_TASKS=on` trong cả hai unit, lần
chạy của một kỹ năng có script chạy `curl https://example.com` thì thất bại
(không tên nào phân giải được, không có route), trong khi lệnh gọi model của
cùng lần chạy được trả lời và lệnh gọi `control.sock` của nó chạy được với
token.

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
  `openclaw` là wrapper của lamp-update), chạy được. Khi đó file target được ghi như khi ghi
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
    vẫn như cũ.
  - Test: `tests/harness.rs`
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
- **Test:** `the_lamps_own_agent_is_closed_once_done_its_news_untold`. Đang
  làm, hay idle chưa có câu trả lời: để mở. Xong mà chưa kể: để mở dưới một
  phút, rồi đóng. Trên máy tính: chờ kể.
