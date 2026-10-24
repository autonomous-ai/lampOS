# Kiểm tra định dạng Rust trong CI

Hai quy trình `lampOS clippy` và `lampOS on ARM64` cài bộ công cụ Rust 1.96.0
được cố định cùng với `rustfmt`. Sau bước thiết lập Rust, mỗi tác vụ kiểm tra
toàn bộ workspace từ thư mục `lampOS/`:

```sh
cargo +1.96.0 fmt --all --check
```

Bước bắt buộc này chạy trước Clippy hoặc bước biên dịch. Lỗi định dạng làm
tác vụ thất bại; bước này không có điều kiện chạy hay tùy chọn bỏ qua lỗi và
không ghi lại mã nguồn. Các shard của lượt chạy ARM64 đầy đủ cũng kiểm tra
như vậy.

Kiểm tra định dạng bổ sung cho biên dịch, lint và kiểm thử. Nó không thay thế
các bước đó và không chứng minh một lượt CI đã thành công. Điều kiện kích
hoạt, tên tác vụ, ma trận, cách dùng bộ nhớ đệm và công bố artifact hiện có
không thay đổi.
