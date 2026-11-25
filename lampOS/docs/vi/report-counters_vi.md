# Bộ đếm trong báo cáo (#6256 F8, `LAMP_REPORT_COUNTERS`, đã xây, tắt trừ khi được bật)

Bản tiếng Việt của mục "Report counters" trong [cloud.md](../cloud.md); nếu
hai trang khác nhau, bản tiếng Anh đúng.

Cho bảng số liệu theo vùng B10 của Backend (đèn ở Trung Quốc so với phần còn
lại): một trường thêm vào dưới `lamp-report/1`, `metrics`, trên báo cáo sức
khoẻ, theo đúng dạng Backend đăng trên #6256 (12/11, cùng khối `mqtt` của
Twelve). Tắt trừ khi `LAMP_REPORT_COUNTERS=on`; khi bật vẫn chỉ đi qua ba
cổng của báo cáo, nên không bao giờ gửi sau khi người dùng nói không. Khi
tắt, báo cáo giống hệt trước từng byte (không có khoá `metrics`). Chỉ có
số: không dữ liệu từng sự kiện, không id, không lời nói, không vùng (cloud
tự chia theo vùng).

```json
"metrics": {
  "voice_turn_ms": {"edges": [500,1000,1500,2000,3000,5000], "counts": [0,4,9,3,1,0,0]},
  "ota_download_s": {"edges": [30,60,120,300,600], "counts": [0,1,0,0,0,0]},
  "wifi_setup": {"attempts": 2, "ok": 1, "fail_password": 1, "fail_no_ip": 0, "fail_other": 0},
  "mqtt": {"transport": "tcp", "reconnects": 0, "tcp_fail": 0, "wss_switches": 0}
}
```

Một giá trị nằm ở ô đầu tiên có mốc lớn hơn nó; vượt mốc cuối thì vào ô
cuối. Khối không có gì thì bỏ ra; `mqtt` luôn có, toàn số 0 khi không có gì
hỏng.

| Khối | Đo bởi | Đến `lamp-cloud` bằng cách nào |
|---|---|---|
| `voice_turn_ms` | `lamp-voice`: từ lúc quyết định người nói đã xong tới âm thanh đầu tiên của câu trả lời, chính là `after_decision_ms` của `answer_first_audio` trong trace (câu đèn tự nói, không có điểm kết thúc lời, không được đếm) | `voice/turn_time` (`lamp_msgs::VoiceTurnTime {after_ms}`, một event), chỉ phát khi giọng có `LAMP_REPORT_COUNTERS=on` (`turn_time_out.rs`) |
| `ota_download_s` | bước `fetching` của `lamp-update`: từ `step_started_at_unix_ms` của nó tới của bước kế tiếp trong cùng một vòng; vòng kết thúc ngay trong `fetching` (tải hỏng) không được đếm | `update/status`, đọc mỗi giây khi có lời đồng ý (`counters::Fetching`) |
| `wifi_setup` | lần join của `lamp-setup` (`wifi::join`): `ok`, `fail_password` (`wrong_password`), `fail_no_ip` (`no_dhcp`), `fail_other` (mọi lý do khác) | tổng cộng dồn trong `/etc/lampOS/setup/wifi-setup.json` (0644, bị xoá khi reset), chỉ ghi khi setup có `LAMP_REPORT_COUNTERS=on` (`wifi_counts.rs`) |
| `mqtt` | kết nối của chính `lamp-cloud` (`online.rs`): `transport` hiện tại (`tcp`, hoặc `wss` khi cầu nối F9 đang sống), `reconnects` (mỗi CONNACK sau cái đầu tiên kể từ khi `lamp-cloud` chạy), `tcp_fail` (kết nối TCP không tới được CONNACK), `wss_switches` (số lần chuyển sang WSS) | trong cùng tiến trình (`Reports::link`) |

- **Tính từ báo cáo trước:** những gì đã đếm mà chưa được broker xác nhận
  được giữ trong state của báo cáo (`state.json`); một báo cáo sức khoẻ mang
  phần đó trừ đi phần mà báo cáo đã trao đi nhưng chưa được xác nhận đang
  mang, và ack của broker chỉ trừ đúng phần báo cáo đó mang. Báo cáo mất
  giữa đường, hoặc khởi động lại, thì số đếm đi lại cùng báo cáo kế. Số đếm
  nằm trong bộ nhớ khi tới, và xuống flash ở lần ghi state kế tiếp (một báo
  cáo được đưa vào hàng đợi hoặc được xác nhận): không số đếm nào tự thêm
  một lần ghi.
- **Các lần join của setup chờ trên đèn** tới khi một báo cáo mang chúng đi,
  nên đèn join được sau vài lần hỏng vẫn kể được; phần `lamp-cloud` đã lấy
  được giữ làm mốc, nên mỗi lần join chỉ gửi một lần.
- **Chỉ khi có lời đồng ý:** một lần chờ của giọng, một lần nối lại hay một
  lần tải xảy ra lúc không có lời đồng ý thì không được đếm (các lần chờ
  của giọng vẫn được lấy khỏi bus rồi bỏ). Một lời "không" xoá các số đếm,
  và các lần join của setup trước đó không bao giờ được gửi.
- **Có giới hạn:** số đếm là một tập số cố định (21 số), dù chuyện gì xảy
  ra; `voice/turn_time` được đọc không chờ, mỗi giây.
