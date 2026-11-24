# Voicin

Nhập bằng giọng nói vào vị trí con trỏ ở bất kỳ ứng dụng nào trên Windows.

## Cách dùng

1. Chạy `target\debug\voicin.exe` (hoặc `cargo run`).
2. Nhấn phím kích hoạt **một lần** để BẬT nghe. Thả phím ra vẫn tiếp tục nghe; nói đến đâu chữ hiện đến đó.
3. Nhấn phím kích hoạt lần nữa để TẮT. Mỗi lần nhấn chỉ đổi trạng thái một lần (giữ phím không lặp lại).
4. Ngôn ngữ được tự nhận diện trong danh sách ứng viên (`VOICIN_LANGS`).

## Phím kích hoạt

Mặc định là **Ctrl** (scan code 29, mã `0x11`, nhận cả Ctrl trái lẫn phải). Nhấn và thả Ctrl một mình để bật/tắt. Nếu Ctrl được dùng trong tổ hợp (ví dụ Ctrl+C) thì app không bật/tắt.

Có thể đổi bằng biến `VOICIN_KEY` (mã hex, ví dụ `set VOICIN_KEY=0x87` cho F24). Lệnh `voicin.exe --keys` hiện mã phím khi bạn nhấn.

## Biến môi trường

| Biến | Mặc định | Ý nghĩa |
| --- | --- | --- |
| `VOICIN_KEY` | `0x87` (F24) | Mã phím kích hoạt (hex) |
| `VOICIN_LANGS` | `vi-VN` | Các ngôn ngữ ứng viên (thêm nhiều ngôn ngữ, cách nhau bằng dấu phẩy, thì app tự nhận diện nhưng chậm hơn) |

Thêm ngôn ngữ càng nhiều thì mỗi câu gửi càng nhiều yêu cầu, chậm hơn một chút.

## Cách hoạt động

- `audio.rs`: thu micro (cpal/WASAPI), đổi về mono 16 kHz.
- `main.rs`: cắt âm thanh thành từng câu theo khoảng lặng (~600 ms), nhận dạng từng câu, rồi gõ kết quả.
- `asr.rs`: gọi Google Speech song song cho các ngôn ngữ ứng viên, chọn kết quả đúng chữ viết và tin cậy nhất.
- `keys.rs`: hook bàn phím cấp thấp để bắt phím kích hoạt.
- `typer.rs`: gõ chữ bằng `SendInput` với `KEYEVENTF_UNICODE`, nên không phụ thuộc bộ gõ tiếng Việt.

## Lưu ý

- Dùng endpoint web không chính thức của Google (cùng cách với phần dịch của app `translator`). Có thể đổi hoặc ngừng bất cứ lúc nào.
- Cần kết nối Internet.
- Câu được gửi đi khi có khoảng lặng, nên chữ hiện theo từng câu chứ không phải từng từ.
- Hiện là app console; chưa có icon khay hệ thống hay chạy nền tự động khi khởi động.
