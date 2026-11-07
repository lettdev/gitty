# Lộ trình gitty

Nguyên tắc: logic viết một lần trong `gitty-core`, các giao diện chỉ là lớp mỏng phía trên. Không có tính năng AI.

## Phase 1: Nền tảng + TUI dùng hằng ngày ✅ (đang hoàn thiện)

- [x] Cargo workspace `gitty-core` + `gitty-tui`
- [x] Status, diff, log, branch qua git CLI
- [x] Stage/unstage theo file, hunk, dòng; commit, amend, commit bằng `$EDITOR`
- [x] Discard có backup; undo log (`u`) cho stage/commit/branch/checkout/pull/discard
- [x] Commit graph nhiều lane, chi tiết commit, diff từng file
- [x] Branch: switch, track remote, tạo, đổi tên, xoá (hỏi khi chưa merge)
- [x] Fetch/pull/push chạy nền có tiến trình; chạy lại trong terminal khi cần mật khẩu
- [x] Thanh lệnh `:` và dòng trạng thái hiện lệnh git tương ứng
- [x] Nhận biết số màu, 3 bộ ký hiệu, chuẩn hoá NFC, chuột, bracketed paste
- [x] Theo dõi file thay đổi (bỏ qua path trong `.gitignore`), refresh khi terminal được focus lại
- [x] Config `config.toml`: ký hiệu, màu, chuột, phím
- [x] CI trên macOS / Windows / Linux
- [ ] Thử tay trên Terminal.app, iTerm2, Windows Terminal, PowerShell (conhost)
- [ ] Đo hiệu năng trên repo khoảng 50k commit (mục tiêu mở dưới 0.5 giây)

## Phase 2: Insight + thao tác nâng cao

- Nhận diện project (Unity, Godot, Unreal, Node, Rust, Go, Python, Flutter, Xcode…)
- Dung lượng working tree và lịch sử `.git`, file lớn nhất trong history (có cache)
- Cố vấn LFS, kiểm tra `.gitignore`, tóm tắt scene/prefab Unity
- Stash, interactive rebase có xem trước, giải quyết conflict 3 cột
- Workspace nhiều repo, heatmap churn

## Phase 3: App desktop + phát hành

- `apps/desktop` (Tauri 2) dùng lại `gitty-core`
- Treemap, diff ảnh (swipe, onion skin), kéo thả rebase, `⌘K`
- `cargo-dist`: Homebrew, Scoop/winget, `.dmg` có ký số và notarize, `.msi`, tự động cập nhật
