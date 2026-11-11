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

## Việc của một kỹ năng trên Hermes của đèn (#6306, PR 3 của #6071, tắt trừ khi `LAMP_SKILL_TASKS=on`)

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
  chỉ dựa vào lời trong prompt. Lần chạy chưa bị chặn mạng (xem "Chưa có").
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
  `HERMES_HOME` và `SHELL` (như lamp-tasks.service cho) cùng
  `LAMP_CONTROL_SOCKET` và `LAMP_CONTROL_TOKEN` của lần chạy; không gì khác
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
  socket bằng `--tasks-socket` hoặc `LAMP_SKILLS_TASKS_SOCKET` (#6346);
  chưa unit nào truyền nó.

**Chưa có (nói rõ để không hiểu code là nhiều hơn):** `ControlTokens` nói
theo socket quản trị của lamp-control (#6408, đã gộp), nhưng lamp-control
chưa giữ lời đồng ý nào của kỹ năng, nên hiện mọi lần xin token ở đó đều bị
từ chối.
Chưa unit sản phẩm nào đặt `LAMP_SKILL_TASKS=on` cho lamp-tasks, và chưa
unit nào truyền socket cho lamp-skills, nên trên đèn chưa có việc của kỹ
năng nào chạy. hermes-ns cũng chỉ cho lần chạy thấy `control.sock` của
lamp-control khi có `LAMP_SKILL_TASKS=on` trong unit của chính lamp-update,
và cũng chưa unit sản phẩm nào đặt nó. Kết quả của lần chạy chưa thành Task news, và thư mục của kỹ năng được
nêu trong lời của lần chạy, chưa nằm trong `AGENTS.md` của thư mục làm việc.
Việc "không mạng nào
khác" của lần chạy (một lệnh gọi ra ngoài trực tiếp bị từ chối) là quyết
định thiết kế của người xây dựng đầu tiên, vì chính các lệnh gọi model của
Hermes cần một đường ra; việc đó và giới hạn
của lamp-tasks.service (`MemoryHigh`, `CPUQuota`) chờ số đo RAM và CPU của
Hermes khi chạy một việc trên lamp-52e6 (đèn 6 GB: mọi giới hạn RAM được
tính theo loại đèn này), so với mức sàn khi cài Hermes
(≥1.350 MB còn trống), trong lúc giọng nói trả lời một câu. Kiểm thử:
`skill_task::tests` (công tắc, vòng đời token, lần chạy lỗi, không có token,
mọi dòng bị từ chối; socket trả lời một dòng, lần chạy thứ hai bị từ chối
`busy`, tắt công tắc thì không mở gì, người gọi không phải lamp-skills không
được trả lời; lần chạy bị kill ở hạn chót, mã thoát của nó, và token nằm
trong môi trường, không bao giờ trong đối số), chạy trên máy tính bất kỳ,
không cần Hermes.

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
đặt. OpenClaw là engine mặc định của đèn (không có ngưỡng bộ nhớ: chủ sản
phẩm, #6226, mọi đèn đều có 6 GB): nó thay Hermes làm target riêng của đèn,
đúng vai trò của Hermes (`tasks.md`, "The lamp itself, with Hermes"), và
Hermes vẫn chọn được. `lamp_order` (`crates/tasks/src/runtime.rs`) chỉ bỏ
OpenClaw ra khi Harness trên đèn cũ hơn 0.6.11.

- **Engine nào:** `LAMP_LAMP_ENGINE` là `openclaw` hoặc `hermes`, không đặt
  thì `openclaw`, mặc định (không có ngưỡng bộ nhớ, #6226). Target
  của đèn vẫn là `lamp`, với engine đó; khi engine được chọn chưa cài mà
  engine kia đã cài, dùng engine kia.
- **Khởi động** như Harness khởi động OpenClaw:
  `openclaw tui --session harness-<agentId> --message "<prompt>"`, không có
  chế độ quyền và không định tuyến model (OpenClaw tự chọn model, theo phần
  cài đặt bên dưới).
- **Voice** (`agent_tasks`, `task_news`): đọc engine của target của đèn từ
  `{"op":"targets"}` và gọi đúng tên, nên một việc giao không nêu máy nào được
  nói là "OpenClaw on the lamp" và mô tả công cụ nói "OpenClaw here on the
  lamp"; nếu là `hermes` thì "Hermes" như trước. Khi chưa đọc targets, voice
  nói OpenClaw, mặc định. `who`
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
  `LAMP_LAMP_ENGINE`: OpenClaw trước nếu không đặt; bỏ OpenClaw khi Harness
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
