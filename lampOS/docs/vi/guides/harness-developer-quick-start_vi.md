# Lập trình đèn từ máy tính: bắt đầu nhanh cho nhà phát triển

Trang này tóm tắt [harness-developer-quick-start.md](../../guides/harness-developer-quick-start.md), bằng tiếng Anh; nếu hai trang khác nhau, bản tiếng Anh đúng.

## Bạn nhận được gì

- **Ghép đôi một máy tính với đèn.** Chủ đèn ghép một lần bằng một mã, chọn quyền cho máy đó, và có thể thu hồi bất cứ lúc nào. Màn hình *Add a developer computer* của ứng dụng sẽ tạo mã này nhưng chưa ra mắt; trong lúc đó người giữ đèn tạo mã. Người luôn thắng: tay đặt lên đầu đèn, "stop", công tắc riêng tư hay một cuộc trò chuyện đều lấy lại quyền điều khiển.
- **SDK** Python và JavaScript (`lampos`): sự kiện, trạng thái và hành động, mỗi thứ cần một quyền chủ đèn đã cấp.
- **Đèn giả** trên máy của bạn, để thử mà không cần đèn.
- Ngoài ra: `lamp-mcp` cho tác tử và kỹ năng đẩy lên đèn.

Harness và SDK là hai cánh cửa khác nhau: SDK nói chuyện với đèn từ máy tính; Harness (của nhóm Harness) chuyển việc nói bằng giọng sang các trợ lý AI trên máy tính.

## Hôm nay chạy được gì

SDK và đèn giả đã merge. `lamp-control` (phía đèn) đã merge nhưng **chưa là một phần của sản phẩm**: chỉ chạy khi người giữ đèn bật `LAMP_CONTROL=on`. Màn hình *Add a developer computer* của ứng dụng **chưa ra mắt**.

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

Lấy mã ghép đôi 8 chữ số kèm pin của đèn (64 ký tự hex), hiệu lực 5 phút, với ba quyền `light`, `speak`, `body`. Hôm nay người giữ đèn chạy `lamp-control code --grants body,light,speak` bằng quyền root. Rồi:

```sh
lamp pair 12345678 --pin <64 ký tự của pin>
lamp hello
python first_light.py
```
