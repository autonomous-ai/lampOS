# Đo xem câu nói tiếp có phải của người vừa gọi đèn hay không

Cảm biến 6 được thử riêng trên bản `0dbbfc95bb`. Sau tiếng gọi “Hey Lamp”, bản này nhận câu nói tiếp bắt đầu trong cửa sổ 20 giây. Khi đèn trả lời, cửa sổ được gia hạn. Cảm biến chỉ ghi kết quả đo và mặc định tắt. Việc nhận câu, gia hạn cửa sổ và xử lý yêu cầu xác nhận do bản cơ sở quyết định. [Hướng dẫn chạy và nguồn mô hình](../verification/caller-frame-8849/README.md) · [Theo dõi kết quả](https://github.com/autonomous-ai/lampOS/issues/8849)

## Phép đo hiện tại bằng SpeechBrain ECAPA-TDNN

Tiến trình đo dùng duy nhất bộ trọng số nhận dạng người nói ECAPA-TDNN mà SpeechBrain phát hành theo giấy phép Apache-2.0. Trên âm thanh 16 kHz, tiến trình tính công suất sau khi bỏ thành phần một chiều trên từng khung 25 ms, cách nhau 10 ms. Cần ít nhất tám khung vượt ngưỡng năng lượng thích nghi. Từ lời gọi “Hey Lamp”, mô hình lấy một vectơ 192 chiều làm mẫu. Với câu nói tiếp, các cửa sổ dài tối đa 1,28 giây trượt mỗi 0,64 giây, cửa sổ cuối bám sát cuối đoạn. Mỗi khung có tiếng nhận trung bình điểm cosine của các cửa sổ phủ tâm khung. Các khung có tiếng trong câu ngắn vừa một cửa sổ nhận cùng một điểm. Phép đo chạy sau khi thu xong câu. [Mô hình và giấy phép](https://huggingface.co/speechbrain/spkrec-ecapa-voxceleb/blob/0f99f2d0ebe89ac095bcc5903c4dd8f72b367286/README.md)

Điểm của câu là trung vị các điểm khung có tiếng. Điểm lớn hơn **0,25** được xếp là cùng người. Kết quả chưa xác định vẫn nằm trong mẫu số. Điểm trong nhật ký bằng `(cosine + 1) / 2`. Cùng ngưỡng này áp dụng cho cả 1.108 cặp từ nhà cung cấp và 44 cặp từ mic dbda. Với mỗi bộ, cần giữ đúng ít nhất 95% câu của người gọi và loại đúng ít nhất 90% câu của người khác. [Quy tắc đã chốt](https://github.com/autonomous-ai/lampOS/issues/8849#issuecomment-7235630577)

Từ thư mục `lampOS/`, chạy `ruby tools/caller-frame-bench/prepare-ecapa.rb embedding_model.ckpt ecapa-voxceleb.onnx` với tệp trọng số gốc. Công cụ kiểm tra SHA-256 của nguồn `0575cb64845e6b9a10db9bcb74d5ac32b326b8dc90352671d345e2ee3d0126a2`. Tiến trình Rust xác minh tệp ONNX `d8c32b0dbc17e31c7b13c9d7910f0adaca2ea0b0b3351da05041c9168e741994`. Trỏ `LAMP_CALLER_FRAME_MODEL` đến đường dẫn tuyệt đối của thư mục chứa tệp. Công tắc riêng `LAMP_CALLER_FRAME_VAD=on` chỉ bật phép đo trên tuyến cuộc gọi đơn giản. Bảy tín hiệu tổng hợp dài từ 0,04 đến 3 giây cho sai lệch tối đa dưới 0,000002 ở từng thành phần vectơ so với bản gốc. [Nguồn trọng số đã cố định](https://huggingface.co/speechbrain/spkrec-ecapa-voxceleb/tree/0f99f2d0ebe89ac095bcc5903c4dd8f72b367286)

Ở ngưỡng đã chốt, ECAPA giữ 130/554 câu của người gọi (23,5%) và loại 524/554 câu của người khác (94,6%) trên bộ nhà cung cấp. Trên mic của đèn dbda, các số tương ứng là 10/16 (62,5%) và 19/28 (67,9%). Ở môi trường yên tĩnh, câu ngắn trên mic đạt 7/9 và 12/18, câu dài đạt 3/7 và 7/10. Hai ô có tiếng ồn trên mic chưa đo được do khoảng trống trong bản thu gốc. Bộ chấm chung đối chiếu đủ 1.152 quyết định. Cảm biến chưa đạt yêu cầu để đưa vào bộ quyết định chung. [Bảng số liệu theo từng điều kiện](../verification/caller-frame-8849/bench.md)

Trên bo ARM64 của lamp-dbda, các dịch vụ của đèn vẫn hoạt động bình thường. Với 24 lượt sau khi mô hình sẵn sàng, thời gian từ lúc bắt đầu gửi âm thanh đến khi nhận quyết định là 398 ms ở p50 và 2.082 ms ở p95. Với 12 câu ngắn 1–3 từ, p50/p95 là 375/415 ms. Với 12 câu dài 6–7 từ, hai số này là 601/2.122 ms. Mỗi lượt tính lại cả tiếng gọi lẫn câu nói tiếp từ âm thanh có sẵn trong bộ nhớ. Phép đo không bao gồm thời gian thu âm, nhận dạng lời nói và phát câu trả lời. P95 trên toàn bộ mẫu vượt mục tiêu 500 ms. [Biên bản đo trên đèn](https://github.com/autonomous-ai/lampOS/issues/8849#issuecomment-7236523867)

Nhật ký của tiến trình ghi tên mô hình và thang điểm. Kho dữ liệu riêng của máy thu giữ điểm cosine gốc, điểm từng khung và thời gian đo của từng câu. [Lệnh chạy cho cả hai bộ](../../tools/caller-frame-bench/strong-replay.md)

Qua ba lần khởi động tiến trình mới trên đèn, trung vị từ khi khởi động đến lúc sẵn sàng là 2.031 ms. Câu đầu gồm một từ mất thêm 302 ms tính từ khi bắt đầu gửi yêu cầu. Trung vị từ khi khởi động đến quyết định đầu tiên là 2.334 ms. Cả sáu nhóm số liệu đều được kiểm tra lại từ 27 dòng thời gian gốc. [Báo cáo EVAL](https://github.com/autonomous-ai/lampOS/issues/8849#issuecomment-7236567550) · [Dữ liệu độ trễ trên lamp-dbda](../verification/caller-frame-8849/bench.md#latency-on-lamp-dbda)

## Kết quả trước đây của CAM++ và Personal VAD ST

CAM++ so sánh hai dấu giọng của cả câu. Mỗi đoạn thu phải có ít nhất 1,5 giây ở 16 kHz. Mô hình chỉ dùng tối đa 1,8 giây đầu. Điểm cosine từ 0,70 trở lên được xếp là cùng người, dưới 0,30 là người khác. Nếu thiếu âm thanh hoặc điểm nằm giữa hai ngưỡng, kết quả là chưa xác định.

Cảm biến dùng bản tái hiện mã nguồn mở của kiến trúc Personal VAD 1.0 ST. Nhóm công bố mô hình này huấn luyện phép chiếu giọng nói trên Multilingual LibriSpeech và LibriSpeech tiếng Anh. Mô hình Personal VAD dùng hai từ “Hey Lamp” của người mở cuộc trò chuyện làm mẫu. Với mỗi khung âm thanh dài 25 ms, cách nhau 10 ms, mô hình tính xác suất cho giọng của người gọi, khoảng lặng và giọng của người khác. Chỉ tính các khung có tiếng nói được phân loại rõ. Cần ít nhất tám khung như vậy. Nếu ít nhất 60% thuộc người gọi thì kết quả là cùng người. Nếu tỷ lệ này từ 40% trở xuống thì là người khác. Các trường hợp còn lại được ghi là chưa xác định. Đoạn mẫu dài tối đa 3 giây và câu nói tiếp dài tối đa 12 giây. Nếu tiếng gọi đi kèm câu hỏi, hệ thống cần mốc thời gian của các từ để tách riêng “Hey Lamp”. Chạm đầu đèn hoặc mở lượt từ máy tính không tạo mẫu giọng.

Trên tuyến thoại thứ hai, phép đo cần `LAMP_SIMPLE_CALL=on`, `LAMP_CALLER_FRAME_VAD=on` và `LAMP_CALLER_FRAME_MODEL` trỏ tới thư mục mô hình cục bộ. Tiến trình giám sát chỉ chuyển công tắc và đường dẫn mô hình cho worker của tuyến thoại thứ hai. Các unit của sản phẩm để cảm biến tắt. Báo cáo tách câu ngắn 1–3 từ và câu dài trong môi trường yên tĩnh hoặc có tiếng ồn. Kết quả chưa xác định vẫn nằm trong mẫu số. Ngưỡng chất lượng hiện tại tương ứng với ít nhất 527/554 và 499/554 trên bộ nhà cung cấp, 16/16 và 26/28 trên bộ mic. Thời gian suy luận khi phát lại trên máy tính và độ trễ đo trực tiếp ở đèn được ghi riêng. Tệp âm thanh của người thật và nhãn riêng tư được lưu cục bộ cùng README và mã SHA-256.

## Kết quả trên tập giọng giữ lại của ElevenLabs và Gemini

Trong 1.108 cặp, mô hình theo khung giữ đúng **457/554 câu của người gọi (82,5%)** và loại đúng **88/554 giọng khác (15,9%)**. CAM++ với điều kiện độ dài đang dùng giữ đúng 0/554 và loại đúng 84/554 (15,2%). Khi có nhiễu +5 dB, mô hình theo khung loại đúng **0/277 giọng khác**. Cảm biến tiếp tục tắt và chưa đủ điều kiện đưa vào bộ xét chung. [EVAL và mã băm](https://github.com/autonomous-ai/lampOS/issues/8849#issuecomment-7224796496)

| Câu nói tiếp và môi trường | CAM++ đúng, cùng người / người khác | Theo khung, giữ đúng | Theo khung, loại đúng |
|---|---|---:|---:|
| 1–3 từ, yên tĩnh | 0/121 · 12/121 | 71/121 (58,7%) | 34/121 (28,1%) |
| 1–3 từ, +5 dB | 0/121 · 12/121 | 120/121 (99,2%) | 0/121 (0%) |
| Từ 4 từ, yên tĩnh | 0/156 · 31/156 | 111/156 (71,2%) | 54/156 (34,6%) |
| Từ 4 từ, +5 dB | 0/156 · 29/156 | 155/156 (99,4%) | 0/156 (0%) |

Kho gốc có 44.981 tệp WAV 16 kHz một kênh. Kế hoạch còn 19 câu Gemini chưa tạo được. Phép đo chính dùng 68 tiếng gọi “Hey Lamp” riêng biệt và 277 câu nói tiếp, gồm 60 câu một từ, tám câu hai từ và 53 câu ba từ. Mỗi câu được ghép với mẫu của đúng người và của người khác, trong môi trường yên tĩnh và có nhiễu. Công cụ của dự án mô phỏng phòng và micrô. Đã đối chiếu cả hai đầu mỗi cặp với bảng kê gốc. Tất cả giọng huấn luyện và 4.500 tệp OpenAI chỉ được lưu trong kho gốc. [Nguồn và mã băm bộ dữ liệu](https://github.com/autonomous-ai/lampOS/issues/8907#issuecomment-7224058425)

Cả tám giọng ElevenLabs và năm trong sáu giọng Gemini có tiếng gọi “Hey Lamp” đứng riêng. Với giọng còn lại, phép đo bổ sung dùng 19 câu nói tiếp để so với người khác. Mô hình theo khung loại đúng 10/76 cặp, trong đó 0/38 cặp có nhiễu. Hai đoạn chỉ chứa lời gọi, được kiểm tra độc lập và cắt từ câu gọi dài của giọng này, cho kết quả giữ đúng 35/38 câu của cùng người. CAM++ loại đúng 12/76 và giữ đúng 0/38 ở hai phép đo bổ sung. Khi dùng đoạn mở đầu dài hơn của cùng giọng và cùng phong cách ở một phép đối chứng riêng, CAM++ giữ đúng 0/554 và loại đúng 252/554 (45,5%).

Trên máy Mac, mô hình theo khung mất 3,062 ms ở cặp đầu, p50/p95 là 2,915/7,918 ms ở 1.107 cặp sau. CAM++ đủ độ dài để chạy ở 210 cặp, mất 79,338 ms ở cặp đầu và 76,652/86,900 ms ở p50/p95 sau đó. Thời gian nạp tệp mô hình tương ứng là 2,158 ms và 866,426 ms. Phép đo bắt đầu khi cả hai đoạn PCM đã có trong bộ nhớ. Bộ chấm chung đã đối chiếu đủ 2.216 kết quả của hai phương pháp. Bản ghi micrô vật lý của lượt D do trưởng nhóm giữ trên máy thu. [Hướng dẫn đối chiếu lượt D](https://github.com/autonomous-ai/lampOS/issues/8849#issuecomment-7224605726)

## Phát qua micrô của lamp-dbda

Trưởng nhóm phát các giọng tổng hợp đã có nhãn qua loa vào micrô của đèn và chấm bản thu 16 kHz trên Mac 2. Mô hình theo khung giữ đúng **16/16** câu của người gọi và loại đúng **1/28 (3,6%)** cặp ghép với giọng khác. Trong 27 cặp còn lại, 25 cặp bị nhận nhầm là người gọi và hai cặp cho kết quả chưa xác định. CAM++ cho kết quả chưa xác định ở cả 44 cặp vì đoạn mẫu chỉ dài 0,55–0,81 giây. [Kết quả trên micrô và mã băm](https://github.com/autonomous-ai/lampOS/issues/8849#issuecomment-7226611814)

| Câu nói tiếp và môi trường | Theo khung, giữ đúng | Theo khung, loại đúng giọng ghép khác | CAM++ đúng, cùng người / người khác |
|---|---:|---:|---|
| 1–3 từ, yên tĩnh | 9/9 | 1/18 (5,6%) | 0/9 · 0/18 |
| Từ 4 từ, yên tĩnh | 7/7 | 0/10 | 0/7 · 0/10 |
| 1–3 từ, có tiếng nền | Chưa đo | Chưa đo | Chưa đo |
| Từ 4 từ, có tiếng nền | Chưa đo | Chưa đo | Chưa đo |

Máy thu dừng từ 00:42:56 đến 01:36:09 GMT+7. Cả ba câu tự nhiên của giọng khác và hai câu có tiếng nền đều rơi vào khoảng này. Sổ theo dõi giữ đủ **53 cặp thiếu bản thu**, gồm 28 câu nói tiếp và 25 cặp ghép giọng khác. Có 31/91 tiếng gọi chưa xác minh được ranh giới hai từ. Mười hai câu tiếp không có mẫu mở đầu của chính lượt đó được ghi riêng. Khi đối chiếu với mẫu xác minh được từ lượt gọi khác của cùng giọng, mô hình giữ đúng 8/9 câu ngắn và 3/3 câu dài.

Trên máy Mac dùng để chấm, mô hình theo khung mất 6,6 ms ở cặp đầu và 5,6/10,5 ms ở p50/p95 của 43 cặp sau. Thời gian nạp mô hình theo khung là 3,3 ms, còn CAM++ là 3,9 giây. Khi bỏ qua điều kiện độ dài, CAM++ có p50/p95 là 246/372 ms. Thời điểm sẵn sàng trực tiếp trên đèn chưa được đo. Trưởng nhóm giữ toàn bộ 6.211 tệp trên Mac 2.

Ở phép đối chứng riêng trên dữ liệu nhà cung cấp, chỉ 300 ms nhiễu cộng thêm được đưa vào mô hình. Với mẫu giọng có nhiễu, 550/554 lượt được xếp là cùng người. Khi dùng mẫu giọng yên tĩnh với chính các đoạn nhiễu ấy, chỉ 19/554 lượt được xếp là cùng người. Cả 1.108 lượt dùng mẫu PCM bằng 0 cho kết quả chưa xác định. Các phép đối chứng này không được tính vào bảng độ chính xác.

## Kết quả phát triển trên giọng macOS và LibriSpeech

Trên 288 cặp giọng tổng hợp của macOS, bộ phân loại theo khung giữ đúng 134/144 câu của người gọi (93,1%) và loại đúng 40/144 câu của người khác (27,8%). Trên 288 cặp từ 12 người đọc LibriSpeech, hai con số là 124/144 (86,1%) và 42/144 (29,2%). Trong nhiễu +5 dB, cả hai bộ đều nhận nhầm toàn bộ giọng người khác. Với cả 144 câu có nhiễu, đổi đoạn mẫu của người gọi sang đoạn mẫu của người khác vẫn cho cùng tỷ lệ khung giọng đích là 1,00. Trong môi trường yên tĩnh, mẫu đúng có điểm cao hơn trên 56/72 câu tổng hợp và 46/72 câu LibriSpeech. Cảm biến chưa đạt ngưỡng để đưa vào bộ xét chung. [Kết quả theo từng điều kiện và độ trễ](../verification/caller-frame-8849/bench.md)

Tiến trình mô hình có thể nạp trọng số ngay khi nhận được mẫu tiếng gọi hợp lệ. Trên đèn, cần đo tỷ lệ kết quả có trong 100 ms sau lúc người dùng nói xong và trước quyết định đầu tiên. Mục tiêu cho p95 thời gian suy luận sau khi đã nạp mô hình là 500 ms. Quyết định nhận câu không chờ mô hình.

Lần chạy lại từ mã nguồn đã chốt cho kết quả phân loại giống hệt. Trên máy Mac, sau khi đã nạp mô hình, p50/p95 là 2,834/4,155 ms với giọng tổng hợp và 3,124/6,987 ms với giọng đọc LibriSpeech. Dữ liệu huấn luyện tiếng Anh của mô hình lấy từ train-clean-100. Mười hai người đọc dùng để đo lấy từ test-clean. [Báo cáo EVAL và mã băm](https://github.com/autonomous-ai/lampOS/issues/8849#issuecomment-7222294409)

Bộ chấm chung #8885 đã đối chiếu đủ 1.152 kết quả của hai phương pháp trên hai bộ dữ liệu. Mỗi bộ gồm 288 cặp, dùng lại 144 tệp câu nói tiếp và 24 tệp mẫu giọng theo từng điều kiện. Chế độ phát lại này chỉ chấm dữ liệu thử nghiệm. Chưa có cặp nào trong bộ thu thực tế đạt điều kiện xác minh. [Quy ước chấm cặp giọng](https://github.com/autonomous-ai/lampOS/blob/bfb0a95a9da96740f77923b76fd8547fecbb7ffd/lampOS/tools/sensors/development-pairs.md) · [Báo cáo EVAL và mã băm của bộ chấm chung](https://github.com/autonomous-ai/lampOS/issues/8849#issuecomment-7222967382)

Một phép đối chứng giữ nguyên 288 cặp LibriSpeech và kéo dài đoạn mẫu đến hết câu nói gốc nếu người đọc còn nói tiếp sau hai từ đầu. Tám trong 12 đoạn mẫu có thêm lời nói. Cả hai đầu vào đủ 1,5 giây ở 118/288 cặp. Với CAM++, số câu giữ đúng vẫn là 0/144, còn số giọng khác loại đúng tăng lên 56/144 (38,9%). Mô hình loại nhầm 14 câu của cùng người và cho kết quả chưa xác định ở 130 câu của cùng người và 88 câu của người khác. Thời gian p95 trên máy Mac sau khi đã nạp mô hình là 80,413 ms cho cả hai dấu giọng và phép so sánh. Phép đo chính vẫn lấy hai từ đầu làm mẫu giọng.

Để kiểm tra tác động của nhiễu, chúng tôi giữ nguyên mẫu giọng của người gọi và chỉ đưa 300 ms đầu của câu nói tiếp vào mô hình. Mỗi đoạn yên tĩnh có đúng 4.800 mẫu PCM bằng 0. Mỗi đoạn có nhiễu trùng từng mẫu với tệp nhiễu đã lưu, trước khi người nói cất tiếng. Mô hình cho kết quả chưa xác định ở cả 288 lượt im lặng. Với 144 đoạn chỉ có nhiễu, mô hình xếp cả 288 lượt là cùng người, dù dùng mẫu của người gọi hay người khác. Tỷ lệ khung giọng đích bằng 1,00 ở mọi lượt có nhiễu. Các lượt đối chứng này được ghi riêng và không tính vào tỷ lệ giữ đúng hoặc loại đúng.

Bộ dữ liệu phát hành xác nhận ElevenLabs có các câu thì thầm thực sự. Khi ghép cặp, công cụ giữ nguyên nhãn phong cách của ElevenLabs và Gemini trong bảng kê. Chỉ các giọng thuộc tập kiểm tra của hai nhà cung cấp này được dùng để đo. [Thông tin phát hành](https://github.com/autonomous-ai/lampOS/issues/8907#issuecomment-7224058425)
