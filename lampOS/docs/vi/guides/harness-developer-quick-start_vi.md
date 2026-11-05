# Lập trình đèn từ máy tính: bắt đầu nhanh cho nhà phát triển

Trang này tóm tắt [harness-developer-quick-start.md](../../guides/harness-developer-quick-start.md), bằng tiếng Anh; nếu hai trang khác nhau, bản tiếng Anh đúng. **Bản nháp cho #5413**: chưa đăng cho đến khi trưởng nhóm SDK kiểm tra các dòng ghi TO CHECK.

## Bạn nhận được gì

- **Ghép đôi một máy tính với đèn.** Chủ đèn ghép một lần trong ứng dụng Autonomous (*Add a developer computer*), chọn quyền cho máy đó, và có thể thu hồi bất cứ lúc nào. Người luôn thắng: tay đặt lên đầu đèn, "stop", công tắc riêng tư hay một cuộc trò chuyện đều lấy lại quyền điều khiển.
- **SDK** Python và JavaScript (`lampos`): sự kiện, trạng thái và hành động, mỗi thứ cần một quyền chủ đèn đã cấp.
- **Đèn giả** trên máy của bạn, để thử mà không cần đèn.
- Ngoài ra: `lamp-mcp` cho tác tử và kỹ năng đẩy lên đèn.

Harness và SDK là hai cánh cửa khác nhau: SDK nói chuyện với đèn từ máy tính; Harness (của nhóm Harness) chuyển việc nói bằng giọng sang các trợ lý AI trên máy tính.

## Hôm nay chạy được gì

SDK và đèn giả đã merge. `lamp-control` (phía đèn) đã merge nhưng **chưa là một phần của sản phẩm** (PR #4526): chỉ chạy khi `LAMP_CONTROL=on`. TO CHECK (trưởng nhóm SDK, di động): màn hình *Add a developer computer* có trong bản ứng dụng khách hàng nhận không.

## Chạy thử (khoảng 5 phút)

```sh
python3 -m venv ~/lampos-venv && . ~/lampos-venv/bin/activate
pip install ./lampOS/sdk/python
# terminal 1
LAMPOS_HOME=/tmp/lampos-try lamp fake
# terminal 2: lưu docs/guides/examples/first_light.py thành first_light.py
LAMPOS_HOME=/tmp/lampos-try python first_light.py
```

Terminal 2 in `connected to lamp-fake with [...]` và trạng thái đèn; terminal 1 in bốn yêu cầu: `get`, `light`, `say`, `gesture`. Đã chạy với đèn giả (bản dựng 39fddfe02); **chưa chạy trên đèn thật**.

## Dùng đèn thật

Trong ứng dụng, chọn *Add a developer computer* và chọn quyền; ứng dụng hiện mã 8 chữ số, pin của đèn (64 ký tự hex) và đếm ngược 5 phút. Rồi:

```sh
lamp pair 12345678 --pin <64 ký tự ứng dụng hiện>
lamp hello
python first_light.py
```
