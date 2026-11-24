# Tắt đèn an toàn (#3697, #7943)

[English](../shutdown.md)

Lệnh tắt từ ứng dụng, nút chính và kỹ năng chuẩn bị rút điện đã được xác nhận
đều dùng trình tự tắt do Setup quản lý. Khi `LAMP_SHUTDOWN=on`, một lần nhấn
mới hợp lệ gửi đúng một yêu cầu khi nút chính vẫn được giữ đủ 10 giây. Thả
nút từ 5 giây đến dưới 10 giây cũng gửi yêu cầu. Nút chính không khôi phục
cài đặt gốc; chức năng đó thuộc nút reset riêng.

## Nút chính và các mốc thời gian

Nút chính của cấu hình sản phẩm dùng GPIO99, chân 35 / PD3, mức thấp là đang
nhấn. Nút reset dùng GPIO100, chân 37. Controls phải quan sát mức đã thả để
cho phép một lần nhấn mới; nút bị giữ ngay khi khởi động không được coi là
lần nhấn mới. Thời gian được tính từ mẫu thu nhận đầu tiên của lần nhấn hợp
lệ, khi Controls được bật và đầu vào GPIO hợp lệ.

| Thao tác với nút chính | `LAMP_SHUTDOWN` tắt | `LAMP_SHUTDOWN=on` |
|---|---|---|
| Thả trước 2 giây | Stop | Stop |
| Thả từ 2 giây đến dưới 5 giây | Tắt tiếng bằng phần mềm | Tắt tiếng bằng phần mềm |
| Thả từ 5 giây đến dưới 10 giây | Tắt tiếng bằng phần mềm | Một yêu cầu tắt đèn, không tắt tiếng riêng |
| Vẫn giữ ở mốc 10 giây | Chưa thực hiện khi còn giữ; tắt tiếng khi thả | Một yêu cầu tắt đèn tại mẫu GPIO đang nhấn, mới và hợp lệ đầu tiên ở hoặc sau mốc 10 giây |
| Giữ lâu hơn hoặc thả sau khi đã yêu cầu | Tắt tiếng khi thả | Không gửi lần hai; giữ tiếp không hủy yêu cầu đã gửi |

Sau khi một lần nhấn mới hợp lệ đạt 5 giây, nút chính không còn thao tác hủy:
thả trước 10 giây sẽ yêu cầu tắt đèn, còn giữ đến 10 giây sẽ yêu cầu ngay khi
vẫn giữ. Giữ quá 15 giây không còn hủy yêu cầu. Vì vậy, một vật đè nút sau
khởi động, khi nút đã được cho phép, có thể tắt đèn ở 10 giây, một lần cho
lần nhấn đó. Nếu vật vẫn đè khi khởi động lại, quy tắc từ chối nút bị giữ từ
lúc khởi động ngăn yêu cầu mới cho đến khi nút được thả để cho phép lại.

Một nhịp bộ định thời không thể kéo dài tuổi của mẫu cũ để kích hoạt tắt đèn.
Controls kiểm tra đồng hồ, phiên, số thứ tự, độ mới và toàn bộ mẫu GPIO trước
khi hành động. Hàng đợi yêu cầu có giới hạn 64 mục (`MAX_PENDING_EVENTS`).
Lỗi GPIO, đầu vào không hợp lệ hoặc GPIO quá cũ xóa hàng đợi và buộc nút phải
được thả để cho phép lần nhấn tiếp theo. Lỗi chỉ ở dải cảm ứng tùy chọn không
làm mất khả năng xử lý GPIO đang hoạt động tốt. Controls bị tắt hoặc chạy
chế độ shadow không gửi yêu cầu.

Trạng thái đã yêu cầu của một lần nhấn được giữ cả sau khi lấy yêu cầu ra
khỏi hàng đợi. Lỗi gửi lên bus, Setup từ chối, giữ tiếp hoặc thả nút đều
không làm lần nhấn đó thử lại. Muốn thử lại phải thả và nhấn mới hợp lệ.

Một lần nhấn reset hợp lệ, đã được cho phép sau khi quan sát mức thả, được
ưu tiên khi trùng với nút chính. Nó chặn toàn bộ lần nhấn nút chính đó và
xóa cả yêu cầu còn trong hàng đợi, chưa được gửi. Thả reset trước không cho
phép lại lần giữ nút chính đang chồng lấp. Controls ghi nhật ký hủy một lần
cho mỗi lần nhấn và bỏ `held_us` của lần nhấn đó; hổ phách tắt và không bật
lại sau khi thả reset. Đầu vào reset bị kẹt ở mức nhấn
từ lúc khởi động và bị từ chối vì chưa được cho phép không vô hiệu hóa nút
chính đang hoạt động tốt. Reset xuất hiện sau đó không thu hồi được yêu cầu
đã gửi lên bus.

## Thông điệp và đèn cảnh báo

Thao tác khi còn giữ gửi thông điệp phiên bản 1 hiện có
`lamp/shutdown_request`, nguồn `button`, do Controls phát. Setup kiểm tra
đúng bên phát, đồng hồ khởi động và thời hạn 2 giây. Không phát `TouchEvent`
giả cho nút chưa thả, cũng không tạo thời điểm thả giả. Khi thực sự thả,
sự kiện `TouchIntent::Shutdown` có thể vẫn xuất hiện trong cửa sổ thả 5–15
giây cũ; cấu trúc trên bus không đổi và runtime không chuyển sự kiện đó
thành yêu cầu tắt trùng lặp. Khi công tắc tắt đèn bật, bộ xử lý nút chính cũ
không gửi yêu cầu tắt tiếng cho lần giữ từ 5 giây trở lên.

Đầu ra nội bộ và replay `TouchOutput` có thêm trường tùy chọn
`shutdown_requests`, mặc định là danh sách rỗng và không xuất hiện khi rỗng.
Runtime gửi trực tiếp các yêu cầu này. Đây không phải trường mới của
`controls/touch` hay một loại thông điệp bus mới.

Từ 5 giây, vòng đèn hiển thị màu hổ phách liên tục (`SetupLook::Failed`,
không kênh màu nào vượt 24) khi nút chính còn được giữ, kể cả sau 10 hoặc
15 giây. Cảnh báo dùng `held_us` còn mới từ `controls/touch_state` và vẫn
chịu quyền điều khiển đầu ra hiện có của vòng đèn; cảnh báo reset màu đỏ được
ưu tiên. Lần nhấn nút chính đã bị reset hủy không còn cảnh báo theo thời gian
giữ. Khi đóng thanh trượt riêng tư vật lý, không có hổ phách: khung đèn lúc
nghỉ yêu cầu micrô được cho phép. Thả nút, trạng thái thiếu hoặc quá cũ, hay
mất quyền điều khiển đầu ra sẽ bỏ cảnh báo tương ứng.

Màu hổ phách chỉ báo đang giữ nút, không xác nhận Setup đã nhận tắt đèn.
Màu xanh lá không phải tín hiệu xác nhận nút: chỉ sau khi trình tự tắt xác
nhận đã nhả mô-men an toàn mới được hiện màu xanh cho phép rút điện.

## Công tắc và khả năng tiếp nhận

Thư viện mặc định tắt nếu giá trị không đúng `on`, nhưng cấu hình sản phẩm
đã bật `LAMP_SHUTDOWN` trong cả bốn unit đọc nó: `lamp-cloud`,
`lamp-controls`, `lamp-voice` và `lamp-setup-reset` (#4867, stable 9528).
#7943 không đổi công tắc mặc định, unit triển khai hay ánh xạ chân. Những
ghi nhận sản phẩm còn tắt công tắc là lịch sử.

Controls và watcher của Setup trong sản phẩm thông thường yêu cầu hoàn tất
thiết lập. `lamp-setup-reset.service` còn cần
`/etc/lampOS/setup/reset-plan.json`; thiếu watcher thì yêu cầu từ nút không
có bên đặc quyền tiếp nhận. Khả năng dùng nút trong thiết lập ban đầu là
trường hợp riêng. Thay đổi này không viết lại cấu hình chân hoặc unit đã giữ
lại hay di chuyển từ cấu hình cũ.

Controls không chạy lệnh root. Watcher khởi chạy `lamp-setup shutdown`, đưa
`shutdown-now` sang unit riêng `lamp-setup-shutdown`, ngoài `lampos.target`.
Khi unit này còn được nạp, watcher không khởi chạy bản thứ hai. Runner kết
thúc hoặc từ chối được thu gom bằng `--collect`, nên một lần nhấn mới độc
lập có thể yêu cầu lại.

## Trình tự tắt dùng chung

Ứng dụng, nút chính và kỹ năng chuẩn bị rút điện đã xác nhận dùng cùng trình
tự Setup. Yêu cầu từ ứng dụng kiểm tra khóa cập nhật ở helper trước khi trả
lời; yêu cầu từ nút đến kiểm tra khóa của runner. Runner không thay đổi
trong #7943:

1. Lấy khóa của `lamp-update` trước mọi tác động tắt đèn, chờ tối đa 5 giây
   nếu đang bận. Nếu khóa vẫn bị giữ hoặc không thể kiểm tra, từ chối mà
   không thực hiện tác động tắt. Nếu cập nhật kết thúc trong lúc chờ, runner
   có thể tiếp tục. Khi lấy được khóa, giữ đến hết trình tự.
2. Nói câu báo đi ngủ từ bộ nhớ đệm, rồi thử văn bản nếu cần, trong tổng
   thời hạn 10 giây. Không nói được thì ghi nhận và vẫn tiếp tục.
3. Yêu cầu pause nếu có keeper; chờ thân đèn đứng yên 2 giây, với giới hạn
   chờ 15 giây. Đèn sản phẩm không có keeper vẫn gập khi thân đèn dừng.
4. Dừng các dịch vụ và xác nhận trạng thái; Voice cùng đường kích hoạt
   setup-done dừng trước. Thân đèn gập khi dừng và vẫn giữ mô-men.
5. Đợi 400 ms sau khi dừng rồi chạy phép đo nhả mô-men. Chỉ nhả khi đèn đã
   hạ xuống, đầu trong 20 count so với home và tay trong 20 count so với
   vị trí gập; đèn dạng ngồi không có tay để kiểm tra. Ghi và đọc lại mô-men
   từng khớp. Đèn còn dựng lên giữ mô-men để không đổ.
6. Chỉ khi kết quả xác nhận nhả thành công mới bật vòng xanh lá an toàn để
   rút điện. Nhả thất bại hoặc đèn còn dựng không được bật xanh.
7. Chạy `sync`, rồi `systemctl poweroff`; nếu cần, buộc tắt sau khi đồng bộ.

Sau khi đã qua khóa cập nhật, một bước thất bại được ghi lại và trình tự
vẫn tiếp tục về tắt hệ điều hành. Không đổi thứ tự dừng dịch vụ và nhả mô-men:
phải dừng các bên có thể ghi lệnh trước để tránh bật lại mô-men. Chế độ
`--dry-run` vẫn nói, pause, dừng, nhả và bật đèn, chỉ bỏ poweroff rồi khởi
động lại các unit trước đó đang chạy; nó không phải phép kiểm tra không có
tác động. Chi tiết từng bước nằm ở [tài liệu tiếng Anh](../shutdown.md#the-runner).

## Bằng chứng lịch sử và kiểm tra còn thiếu

Ngày 2 tháng 11, #3697 ghi nhận một lần nhấn **đã thả** sau 8.668 ms bị từ
chối trong lúc cập nhật, rồi tắt thành công bằng nút và ứng dụng, khởi động
lại sạch trên lamp-52e6, stable 9424. #4867 dùng kiểm tra đường thả nút đó
khi bật công tắc sản phẩm trong stable 9528. Chưa ghi hình được cảnh báo
hổ phách. Các kết quả lịch sử này không chứng minh đường giữ đủ 10 giây mới
hoặc trạng thái của đèn xuất xưởng hiện tại.

Các kiểm thử trên máy phát triển bao phủ ngưỡng khi còn giữ, chỉ phát một
yêu cầu cho mỗi lần nhấn, từ chối mẫu cũ/lỗi và reset chồng lấp. Kiểm thử độ
trễ tùy chọn `primary_held_shutdown_threshold_to_bus_host_latency` đo từ mẫu
mới tại ngưỡng đã xếp hàng, qua bước xử lý dịch vụ thật, nhận bus và phân tích
JSON, gồm lần đầu và 200 lần lặp. Mục tiêu là p95 ≤50 ms và lớn nhất ≤100 ms.
Phép đo không gồm thu nhận vật lý hoặc lập lịch vòng dịch vụ; kết quả từng
lần chạy nằm trong bằng chứng review của thay đổi.

Mục tiêu trên thiết bị, từ thu nhận mẫu tại ngưỡng cảnh báo đến ánh hổ phách
nhìn thấy được, là ≤200 ms và **chưa được đo**. Không tính thời gian cố ý giữ
10 giây vào các mục tiêu này; chúng cũng không chứng minh độ trễ từ yêu cầu
đến runner hoặc tiếng nói đầu tiên. Chờ khóa cập nhật, thời hạn tiếng nói,
nghỉ và dừng dịch vụ vẫn là các phần riêng của quá trình tắt.

Kiểm tra thiết bị được cho phép còn phải ghi nhận chuyển mức GPIO99 thật,
hổ phách khi còn giữ, một yêu cầu khoảng 10 giây trước khi thả và không có
yêu cầu thứ hai từ cùng lần nhấn sau khi thả hoặc bị từ chối. Phải kiểm tra
các thao tác ngắn, reset chồng lấp, hổ phách đã hủy không bật lại sau khi thả
reset, và lần nhấn mới sau đó. Kiểm tra khi đóng thanh trượt riêng tư (không
có hổ phách). Khi khóa cập nhật vẫn bị giữ, hổ phách có thể còn sáng lúc giữ
nút nhưng runner phải từ chối mà không gây tác động tắt đèn. Đo từ mốc 10
giây đến tiếng nói hoặc chuyển động đầu tiên; vòng đèn không đổi ở 10 giây,
nên đó là dấu hiệu đầu tiên cho người dùng biết quá trình tắt đã bắt đầu.
Cần xác nhận thứ
tự nghỉ/dừng/nhả mô-men, chỉ xanh sau khi nhả thành công, và lần khởi động
tiếp theo không sửa hệ thống tệp. Việc LED hoặc servo còn điện sau khi bo
mạch tắt phải được quan sát trực tiếp. Chưa khẳng định kết quả thiết bị,
âm thanh, phản hồi vật lý hoặc phát hành cho thay đổi này.

Liên quan: [Controls](../controls.md), [công tắc](../switches.md),
[chuẩn bị rút điện](../unplug.md).
