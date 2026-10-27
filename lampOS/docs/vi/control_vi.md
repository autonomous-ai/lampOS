# Điều khiển: lập trình đèn từ mã của bạn

Trang này tóm tắt [control.md](../control.md), bằng tiếng Anh; nếu hai trang
khác nhau, bản tiếng Anh đúng. Các thông điệp nằm trong
[control.schema.json](../control.schema.json) (JSON Schema 2020-12), và các SDK
được sinh ra từ đó.

**Chưa được xây dựng.** Hôm nay chưa có phần nào phục vụ giao thức này.
`lamp-control` là phần sẽ làm việc đó, tắt cho đến khi chủ đèn bật quyền truy
cập cho nhà phát triển trong ứng dụng.

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
  giờ** là lời đã nói), `name.heard`, `sound.heard`, `touch`, `button`,
  `time`, `lamp.changed`.
- **Hành động:** `express`, `gesture`, `look`, `move`, `play`, `joints`,
  `light`, `say`, `sound`, `stop`. `look`, `move`, `play` và `joints` cần một
  lease.
- **Quyền:** `body`, `light`, `speak`, `look`, `hear`, `sound`.
- **Không bao giờ qua cửa này:** khung hình camera, âm thanh, bản ghi lời
  nói, dấu khuôn mặt hay tên.

## Đường truyền

- `wss://<đèn>.local:7731/v1`, tìm bằng mDNS `_lampos._tcp`, chứng chỉ tự
  ký được ghim khi ghép đôi.
- Mỗi khung là một đối tượng JSON: yêu cầu `{"id":7,"op":"..."}`, trả lời
  `{"re":7,"ok":true}` hoặc `{"re":7,"ok":false,"error":{...}}`, sự kiện
  `{"ev":"...","at":"..."}`.
- **Ghép đôi** một lần cho mỗi máy: mã dùng một lần từ ứng dụng (hoặc giữ
  nút của đèn 3 giây), rồi `lamp pair` gửi khóa Ed25519 công khai cùng bằng
  chứng. **Hello** trên mỗi kết nối, ký bằng khóa đó.

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
