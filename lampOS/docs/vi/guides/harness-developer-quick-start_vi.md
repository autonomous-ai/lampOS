# Lập trình đèn từ máy tính: bắt đầu nhanh cho nhà phát triển

Trang này tóm tắt [harness-developer-quick-start.md](../../guides/harness-developer-quick-start.md), bằng tiếng Anh; nếu hai trang khác nhau, bản tiếng Anh đúng.

## Bạn nhận được gì

- **Ghép đôi một máy tính với đèn.** Chủ đèn bật quyền truy cập cho nhà phát triển trong ứng dụng, rồi ghép một lần bằng một mã, chọn quyền cho máy đó, và có thể thu hồi bất cứ lúc nào. Người luôn thắng: tay đặt lên đầu đèn, "stop", công tắc riêng tư hay một cuộc trò chuyện đều lấy lại quyền điều khiển.
- **SDK** Python và JavaScript (`lampos`): sự kiện, trạng thái và hành động, mỗi thứ cần một quyền chủ đèn đã cấp.
- **Đèn giả** trên máy của bạn, để thử mà không cần đèn.
- Ngoài ra: `lamp-mcp` cho tác tử và kỹ năng đẩy lên đèn.

Harness và SDK là hai cánh cửa khác nhau: SDK nói chuyện với đèn từ máy tính; Harness (của nhóm Harness) chuyển việc nói bằng giọng sang các trợ lý AI trên máy tính.

## Hôm nay chạy được gì

SDK và đèn giả đã merge. `lamp-control` (phía đèn) có trên mọi đèn nhưng tắt cho đến khi chủ đèn bật quyền truy cập cho nhà phát triển trong ứng dụng.

## Chạy thử (khoảng 5 phút)

```sh
python3 -m venv ~/lampos-venv && . ~/lampos-venv/bin/activate
pip install ./lampOS/sdk/python
# terminal 1 (kích hoạt môi trường ảo trong MỖI terminal)
. ~/lampos-venv/bin/activate
LAMPOS_HOME=/tmp/lampos-try lamp fake
# terminal 2: lưu docs/guides/examples/first_light.py thành first_light.py
. ~/lampos-venv/bin/activate
LAMPOS_HOME=/tmp/lampos-try python first_light.py
```

Terminal 2 in `connected to lamp-fake with [...]` và trạng thái đèn; terminal 1 in bốn yêu cầu: `get`, `light`, `say`, `gesture` (cùng dòng `said person.arrived` của đèn giả). Đã chạy với đèn giả; **chưa chạy trên đèn thật**.

## Dùng đèn thật

Lấy mã ghép đôi 8 chữ số kèm pin của đèn (64 ký tự hex), hiệu lực 5 phút, với ba quyền `light`, `speak`, `body`. Trong ứng dụng, bật quyền truy cập cho nhà phát triển, mở đèn và chọn *Add a developer computer*. Nếu ứng dụng của bạn chưa có cài đặt này, người giữ đèn có thể tạo mã: chạy `lamp-control code --grants body,light,speak` bằng quyền root trên đèn, lệnh in ra mã và pin. Rồi:

```sh
lamp pair 12345678 --pin <64 ký tự của pin>
lamp hello
python first_light.py
```
