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

## Phase 2: Insight + thao tác nâng cao ✅

- [x] Nhận diện project (Unity, Godot, Unreal, Node, Rust, Go, Python, Flutter, Swift, .NET, Android/JVM, CMake)
- [x] Dung lượng working tree theo thư mục, `.git`, kho LFS
- [x] File lớn nhất trong lịch sử, kèm commit đã thêm nó (cache theo refs)
- [x] Cố vấn LFS (track và convert, migrate có cảnh báo), kiểm tra `.gitignore` theo loại project, kiểm tra cài đặt Unity
- [x] Tóm tắt thay đổi scene/prefab Unity
- [x] Stash: tạo, apply, pop, drop (undo được), xem file trong stash
- [x] Interactive rebase có xem trước, cảnh báo commit đã push, continue/abort/skip
- [x] Giải conflict theo vùng (ours/theirs/both/base), cả file, hiện bản gốc, mở editor
- [x] Workspace nhiều repo
- [x] File hay sửa (90 ngày) và người sửa nhiều nhất theo thư mục
- [ ] Đo hiệu năng trên repo game khoảng 3 GB (mục tiêu: lần đầu dưới 10 giây, lần sau dưới 1 giây)

## Phase 3: App desktop + phát hành

**Mốc 1 ✅**
- [x] `apps/desktop` (Tauri 2 + React + TypeScript), gọi thẳng `gitty-core`
- [x] Changes: stage theo file, hunk, dòng (chọn bằng chuột); discard; commit và amend; undo
- [x] Diff ảnh: đặt cạnh nhau, swipe, onion skin
- [x] History: graph SVG, chi tiết commit, sửa message
- [x] Insight: treemap, biểu đồ thành phần, Health có nút Fix, file lớn nhất trong lịch sử
- [x] Command palette `⌘K`; sidebar có repo, branch, stash; fetch/pull/push

**Mốc 2**
- [ ] Kéo thả để rebase, giải conflict trong app desktop
- [ ] `cargo-dist`: Homebrew, Scoop/winget cho bản TUI
- [ ] `.dmg` có ký số và notarize, `.msi`, tự động cập nhật
- [ ] CI build app desktop trên 3 hệ điều hành
