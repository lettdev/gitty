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

## Năm màn hình

| Phím | Màn | Làm được gì |
|---|---|---|
| `1` | **Changes** | Stage/unstage theo file, hunk hoặc từng dòng; commit, amend, discard; giải conflict 3 cột |
| `2` | **History** | Commit graph của mọi branch, chi tiết commit, diff từng file, interactive rebase (`r`) |
| `3` | **Branches** | Switch, tạo, đổi tên, xoá branch; xem log của từng branch; quản lý stash |
| `4` | **Insight** | Loại project, dung lượng theo thư mục, file lớn nhất trong lịch sử, LFS, `.gitignore`, file hay sửa |
| `5` | **Repos** | Mọi repo trong thư mục làm việc: branch, số thay đổi, ahead/behind; `enter` để mở |

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
| `s` | Stash mọi thay đổi (`tab` để chọn có gồm file mới không) |
| `r` | Ở History: interactive rebase từ commit đang chọn |
| `e` | Ở History: sửa message của commit đang chọn (commit nào cũng được) |
| `i` | Ở Changes: ignore file đang chọn, mọi file cùng đuôi, hoặc cả thư mục |
| `I` | Xem và sửa mọi rule ignore; `/` để hỏi "vì sao file này bị ignore?" |
| `m` | Khi đang merge/rebase: continue, abort hoặc skip |
| `:` | Chạy lệnh git bất kỳ ngay trong terminal |
| `?` | Xem toàn bộ phím |
| `q` | Thoát |

Dòng trạng thái dưới cùng luôn hiện lệnh git tương ứng với thao tác vừa làm.

## Insight

- **Nhận diện project:** Unity, Godot, Unreal, Node (framework và package manager), Rust, Go, Python, Flutter/Dart, Swift/Xcode, .NET, Android/JVM, CMake. Kèm phiên bản engine, render pipeline, số script và tỉ lệ ngôn ngữ.
- **Thành phần:** dung lượng theo loại file (code, ảnh/texture, audio, video, model 3D, scene/asset engine, font, tài liệu, binary), có thanh tỉ lệ và các đuôi file chiếm nhiều nhất.
- **Dung lượng:** working tree theo thư mục (`enter` để vào, `esc` để lên), kích thước `.git` và kho LFS.
- **Lịch sử:** file lớn nhất từng được commit, kể cả file đã xoá, kèm commit đã thêm nó (`enter` để nhảy tới). Kết quả quét được cache trong `.git/gitty/cache`, chỉ quét lại khi refs thay đổi; `R` để quét lại ngay.
- **Health**, chọn mục rồi `enter` để sửa:
  - file lớn hoặc file art/audio chưa vào LFS: track và convert bản hiện tại (undo được)
  - thư mục engine tự sinh (`Library/`, `UserSettings/`, `.godot/`, `Intermediate/`, `node_modules/`, `target/`…) bị commit hoặc chưa được ignore: thêm vào `.gitignore` và ngừng track (undo được)
  - `.DS_Store`, `Thumbs.db` bị commit
  - Unity: chế độ serialize binary, `.meta` file bị ẩn
  - file lớn trong lịch sử chưa vào LFS: chạy `git lfs migrate` trong terminal, sau khi cảnh báo rõ là sẽ viết lại lịch sử
- **Hot files:** file được sửa nhiều nhất trong 90 ngày, kèm người sửa nhiều nhất theo từng thư mục.

## Unity

- Diff của `.unity`, `.prefab`, `.asset`, `.mat`… có thêm khung **Unity changes** tóm tắt theo object, ví dụ `InventoryChest › Transform.m_LocalPosition: {x: 12.5…} → {x: 14…}` hoặc `+ Lamp (GameObject) added`.

## Conflict và rebase

- Chọn file trong mục **Conflicts**, bấm `enter` để mở trình giải conflict với các cột ours, gốc và theirs. Từng vùng chọn `o` ours, `t` theirs, `b` cả hai, `e` bản gốc; cả file chọn `O` hoặc `T`. `B` hiện bản gốc giữa hai bên, `E` mở bằng editor, `space` để đánh dấu đã giải xong. Mọi thao tác đều undo được.
- **Interactive rebase:** ở History bấm `r` trên commit muốn bắt đầu. Mỗi commit chọn `p` pick, `r` reword, `s` squash, `f` fixup hoặc `d` drop; `J`/`K` để đổi thứ tự. Bấm `enter` để xem trước kết quả và cảnh báo nếu có commit đã push. Nếu rebase dừng vì conflict, giải xong rồi bấm `m` để continue. `u` hoàn tác cả lần rebase.

## Hoàn tác

Trước mỗi thao tác, gitty lưu lại HEAD, các branch và staging area vào `.git/gitty/oplog.jsonl`. Nội dung file bị discard được cất thành blob trong `.git`. Nhấn `u` để quay lại.

Các thao tác hoàn tác được gồm: stage, commit, sửa message, branch, checkout, pull, discard, sửa `.gitignore`/LFS, giải conflict, stash và drop stash, rebase.

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
