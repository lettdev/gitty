# gitty

Git client chạy trong terminal: nhanh như CLI, dễ nhìn như GUI. Chạy được trên macOS, Windows và Linux.

```
gitty            # mở repo chứa thư mục hiện tại
gitty ~/code/x   # mở repo khác
```

## Cài từ source

Cần Rust ≥ 1.85 và `git` có sẵn trong PATH.

```
cargo install --path crates/gitty-tui
```

## Ba màn hình

| Phím | Màn | Làm được gì |
|---|---|---|
| `1` | **Changes** | Stage/unstage theo file, theo hunk hoặc từng dòng; commit, amend, discard |
| `2` | **History** | Commit graph của mọi branch, chi tiết commit, diff từng file |
| `3` | **Branches** | Switch, tạo, đổi tên, xoá branch; xem log của từng branch |

## Phím chính

| Phím | Tác dụng |
|---|---|
| `j` `k` / mũi tên | Di chuyển |
| `enter` / `esc` | Vào pane sâu hơn / quay lại |
| `tab` | Chuyển pane |
| `space` | Stage / unstage file, hunk hoặc các dòng đã chọn |
| `v` | Bắt đầu / kết thúc chọn dòng trong diff |
| `]` `[` | Hunk kế tiếp / trước |
| `a` | Stage tất cả (ở History: bật/tắt hiện mọi branch) |
| `c` / `A` / `C` | Commit / amend / commit bằng `$EDITOR` |
| `d` | Discard (luôn có bản backup, `u` để hoàn tác) |
| `u` | **Hoàn tác** thao tác gitty gần nhất |
| `f` `p` `P` | Fetch / pull / push |
| `:` | Chạy lệnh git bất kỳ ngay trong terminal |
| `?` | Xem toàn bộ phím |
| `q` | Thoát |

Dòng trạng thái dưới cùng luôn hiện lệnh git tương ứng với thao tác vừa làm.

## Hoàn tác

Trước mỗi thao tác, gitty lưu lại HEAD, các branch và staging area vào `.git/gitty/oplog.jsonl`. Nội dung file bị discard được cất thành blob trong `.git`. Nhấn `u` để quay lại.

Undo sẽ **từ chối** nếu repo đã bị thay đổi từ bên ngoài sau thao tác đó (ví dụ bạn sửa lại file vừa discard), để không làm mất công việc của bạn. Push và các lệnh chạy qua `:` không hoàn tác được.

## Cấu hình

Đường dẫn file config:
- macOS / Linux: `~/.config/gitty/config.toml` (hoặc `$XDG_CONFIG_HOME/gitty/config.toml`)
- Windows: `%APPDATA%\gitty\config.toml`

Xem `config.example.toml` để biết mọi tuỳ chọn.

## Tương thích terminal

- **Màu:** tự nhận biết terminal hỗ trợ 24-bit, 256 hay 16 màu. `NO_COLOR` được tôn trọng.
- **Ký hiệu:** `unicode` (mặc định), `nerd` (cần Nerd Font), `ascii`. Ở console Windows cũ (conhost) và console Linux, gitty tự chuyển sang `ascii`.
- **Tên file tiếng Việt:** được chuẩn hoá NFC trước khi hiển thị, nên tên dạng tách dấu (NFD) trên macOS không bị lệch cột.
- **Mật khẩu / passphrase:** fetch, pull, push chạy nền và không bao giờ hiện prompt lên giữa giao diện. Nếu git cần mật khẩu, bấm `!` trong thông báo lỗi để chạy lại lệnh trực tiếp trong terminal.
- **Chuột:** click để chọn, cuộn để di chuyển. Muốn bôi đen chữ thì giữ `Shift` (hoặc `Option` trên Mac), hoặc tắt bằng `mouse = false`.

## Cấu trúc

```
crates/gitty-core   logic repo dùng chung (gọi git CLI, parse, undo log)
crates/gitty-tui    giao diện terminal (Ratatui + crossterm)
mockups/            bản phác giao diện
docs/PLAN.md        lộ trình
```

## Phát triển

```
cargo test        # test core và test giao diện trên repo tạm thật
cargo run -- .    # chạy gitty trên chính repo này
```
