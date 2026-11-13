# Điều khiển: lập trình đèn từ mã của bạn

Trang này tóm tắt [control.md](../control.md), bằng tiếng Anh; nếu hai trang
khác nhau, bản tiếng Anh đúng. Các thông điệp nằm trong
[control.schema.json](../control.schema.json) (JSON Schema 2020-12), và các SDK
được sinh ra từ đó.

**Chưa được xây dựng.** Hôm nay chưa có phần nào phục vụ giao thức này.
`lamp-control` là phần sẽ làm việc đó, tắt cho đến khi chủ đèn bật quyền truy
cập cho nhà phát triển trong ứng dụng.

Khi `LAMP_CONTROL` không bật mà `LAMP_SKILL_TASKS=on`, `lamp-control` vẫn
chạy nhưng không có cửa: không lắng nghe TCP, không TLS, không ghép đôi. Nó
chỉ phục vụ phía chủ đèn (`--admin-socket`: lời đồng ý của voice, việc cấp
token của lamp-tasks) và socket cho skill run. Cả hai đều tắt thì nó báo vậy
và thoát với mã 0.

## Bốn quyết định

1. **Nhà phát triển trước.** API trực tiếp, SDK Python và JavaScript và một
   máy chủ MCP đến trước; chỉnh sửa không cần mã bằng giọng nói hay ứng dụng
   đến sau.
2. **Chỉ trong mạng nhà.** Việc điều khiển không đi qua đám mây.
3. **Mở, trong giới hạn của thân đèn.** Một máy đã ghép đôi có thể cử động
   thân đèn trong các giới hạn mà `lamp-body` giữ cho mọi bên gửi. Máy chưa
   ghép đôi không được gì.
4. **Chia sẻ từ ngày đầu.** Kỹ năng lên Store có chữ ký, và khi cài sẽ thấy
   quyền trước.

## Một bộ từ vựng chung

- **Sự kiện:** `person.arrived`, `person.left`, `person.stood_up`,
  `person.facing`, `phrase.heard` (chỉ cụm từ đã đăng ký khớp, **không bao
  giờ** là lời đã nói; so với chữ của bộ nhận dạng: chữ thường, bỏ dấu nháy,
  từ viết liền hay tách đều khớp ("goodnight" / "good night"), và từ cuối
  nghe được có thể thêm một "s" ("lamps" khớp "lamp"); lamp-control chỉ ghi
  số đếm vào nhật ký, không ghi lời), `name.heard`, `sound.heard`, `touch`, `button`,
  `time`, `lamp.changed`.
- **`touch`:** một cú chạm đầu (`tap`, nhả nhanh) từ `controls/touch` và một
  cú vuốt đầu (`pet`, mỗi lần chạm một sự kiện, từ `controls/pet`) đến chương
  trình của bạn; loại `hold` chưa được tạo ra.
- **Hành động:** `express`, `gesture`, `look`, `move`, `play`, `joints`,
  `light`, `say`, `sound`, `stop`. `look`, `move`, `play` và `joints` cần một
  lease.
- **Quyền:** `body`, `light`, `speak`, `look`, `hear`, `sound`.
- **Không bao giờ qua cửa này:** khung hình camera, âm thanh, bản ghi lời
  nói, dấu khuôn mặt hay tên.

## Đường truyền

- `wss://<đèn>.local:7731/v1`, tìm bằng mDNS `_lampos._tcp`, chứng chỉ tự
  ký của đèn, mà máy kiểm tra theo mã ghim (SHA-256) ứng dụng đưa kèm mã
  ghép đôi, trước khi gửi bất cứ thứ gì.
- Mỗi khung là một đối tượng JSON: yêu cầu `{"id":7,"op":"..."}`, trả lời
  `{"re":7,"ok":true}` hoặc `{"re":7,"ok":false,"error":{...}}`, sự kiện
  `{"ev":"...","at":"..."}`.
- **Ghép đôi** một lần cho mỗi máy: mã dùng một lần từ ứng dụng, kèm mã
  ghim chứng chỉ của đèn; không có ghép đôi nào thiếu mã ghim đó. `lamp pair`
  kiểm tra chứng chỉ trước, rồi gửi khóa Ed25519 công khai cùng bằng chứng
  (HMAC trên mã ghim, khóa và nonce). Bằng chứng sai thì kết nối bị đóng, và
  mỗi giây chỉ một lần thử trên cả đèn. **Hello** trên mỗi kết nối, ký bằng
  khóa đó, một lần cho mỗi kết nối.

## Token cho skill run

Khi cấp token (`mint`, #6304, `task_yes::KeptYes`), `lamp-control` tự băm
thư mục `/var/lib/lampOS/skills/<skill>`, hoặc, với tên không có ở đó, thư
mục của skill được đẩy lên `/var/lib/lampOS/skills-dev/<skill>`
(`task_yes::folder_of`). Chỉ cần có bất cứ thứ gì mang tên đó trong
`skills/` (thư mục, tệp hay liên kết) là skill được đẩy lên không bao giờ
thay chỗ nó; lamp-skills và lamp-tasks giữ cùng quy tắc đó. Một thư mục là
liên kết thì không bao giờ được băm. Token chỉ nêu các quyền khi
`task-yes.json` giữ lời đồng ý của chủ đèn cho đúng mã SHA-256 đó và các
quyền đó. Không có lời đồng ý, hay thư mục đã đổi từ khi đồng ý, thì bị từ
chối và không token nào được cấp. Lời đồng ý được giữ mà không có quyền nào
(một skill có mã không dùng gì của đèn, như skill mã do #6307 tạo) vẫn là
lời đồng ý: token của nó không nêu quyền nào (`"grants": []`), nên lần chạy
chỉ `hello` được, mọi thao tác cần quyền đều bị từ chối `no_grant`.

**Lượt chạy của một lệnh ghi cố định** (`LAMP_SKILL_WRITE_AGENT=on` trong
lamp-control, đúng `on`, không bật trong unit sản phẩm nào; quyết định của
chủ đèn, 13/11): khi cấp token cho một skill không có lời đồng ý task nào mà
bước `[[do]]` duy nhất là một lệnh ghi cố định
(`lamp_skills::service_write::fixed_write_of`, đọc từ thư mục của nó), token
nêu một quyền duy nhất, `-write` của dịch vụ của lệnh ghi
(`task_yes::KeptYes::fixed_write_grant`), và lượt chạy được đánh dấu
(`runs::Run::fixed_write`). Ở đây không đọc lời đồng ý nào: lời đồng ý của
chủ cho lệnh ghi là của lamp-services, khớp đúng nội dung lúc gửi.
`service.write` của lượt chạy đó mang `"fixed_write_run": "<run id>"` bên
cạnh (`services::Services::fixed_write`), nên lamp-services chỉ gửi như lời
đồng ý đó, một lần mỗi lượt chạy, không bao giờ giữ chờ. Tắt thì việc cấp
token cho skill như vậy là "the owner hasn't said yes to this skill's task",
như trước. Test: `a_fixed_writes_run_is_minted_its_one_write_grant`,
`a_fixed_writes_run_names_its_run_to_lamp_services`.

## Ai thắng

Giới hạn an toàn của thân đèn, rồi người (tay trên đầu, công tắc riêng tư,
"stop", tạm dừng), rồi cuộc trò chuyện, rồi ứng dụng của chủ, rồi máy khách,
rồi đời sống riêng của đèn. Cuộc trò chuyện tạm dừng lease của máy khách; một
bàn tay hay "stop" chấm dứt nó.

## Giới hạn

Mọi cử động đi qua các giới hạn của `lamp-body`, không bên gửi nào nâng được;
không có truy cập servo trực tiếp. Tốc độ cho mỗi máy khách: `joints` 20 lần
một giây, `gesture`/`move`/`play`/`look` một lần mỗi 2 giây, `light` 5 lần
một giây, `say`/`sound` 6 lần một phút và 60 lần một giờ, `express` một lần
một phút, tối đa 16 cụm từ, mỗi cụm tối đa 64 ký tự.
