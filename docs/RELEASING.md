# Phát hành gitty

## Tạo bản phát hành

1. Tăng `version` trong `Cargo.toml` (workspace), `apps/desktop/package.json` và `apps/desktop/src-tauri/tauri.conf.json`.
2. Commit, rồi gắn tag và push:
   ```
   git tag v0.2.0 && git push origin v0.2.0
   ```
3. Workflow **Release** build các bản chạy được ngay (không có bộ cài) và đính kèm vào một GitHub release ở dạng **draft**:

   | Nền tảng | App desktop | Bản terminal |
   |---|---|---|
   | macOS Apple Silicon (M1 trở lên) | `Gitty-v0.2.0-macos-arm64.zip` (giải nén ra `Gitty.app`) | `gitty-v0.2.0-macos-arm64.tar.gz` |
   | Windows x64 | `Gitty-v0.2.0-windows-x64.exe` (chạy thẳng) | `gitty-v0.2.0-windows-x64.zip` |

   Mỗi file có kèm `.sha256`. Không build cho Mac Intel và Linux.
4. Kiểm tra lại các file, rồi bấm **Publish** trên GitHub.

## Ký số và notarize trên macOS (không bắt buộc)

Nếu không ký, macOS sẽ chặn khi mở app tải từ mạng. Người dùng xử lý bằng một trong hai cách: chuột phải vào `Gitty.app` → **Open**, hoặc chạy `xattr -dr com.apple.quarantine /Applications/Gitty.app`.

Trên Windows, `Gitty.exe` cần WebView2. Windows 10 và 11 hầu hết đã có sẵn; nếu thiếu, Windows sẽ đề nghị cài. Để ký, cần tài khoản **Apple Developer** (99 USD/năm), rồi thêm các secret sau vào repo (Settings → Secrets → Actions):

| Secret | Lấy ở đâu |
|---|---|
| `APPLE_CERTIFICATE` | Export chứng chỉ "Developer ID Application" ra file `.p12`, rồi `base64 -i cert.p12` |
| `APPLE_CERTIFICATE_PASSWORD` | Mật khẩu đã đặt khi export file `.p12` |
| `APPLE_SIGNING_IDENTITY` | Ví dụ `Developer ID Application: Tên (TEAMID)` (xem bằng `security find-identity -v -p codesigning`) |
| `APPLE_ID` | Email tài khoản Apple Developer |
| `APPLE_PASSWORD` | App-specific password, tạo tại appleid.apple.com |
| `APPLE_TEAM_ID` | Team ID gồm 10 ký tự |

Workflow chỉ ký khi các secret này có giá trị. Nếu thiếu, nó vẫn build bản không ký và hiện cảnh báo.

## Tự động cập nhật (chưa bật)

App desktop có thể tự cập nhật qua `tauri-plugin-updater`. Việc này cần một cặp khoá ký riêng:
```
cd apps/desktop && npx tauri signer generate -w ~/.tauri/gitty.key
```
Sau đó:
- thêm khoá **public** vào `tauri.conf.json` (`plugins.updater.pubkey`), cùng endpoint `https://github.com/lettdev/gitty/releases/latest/download/latest.json`
- thêm secret `TAURI_SIGNING_PRIVATE_KEY` và `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`
- bật `createUpdaterArtifacts` trong `bundle`

Khoá private không bao giờ được commit. Nếu mất khoá, các bản đã cài sẽ không nhận được cập nhật nữa.

## Homebrew, winget và Scoop (bản terminal)

Mẫu nằm trong `packaging/`. Workflow Release tự điền phiên bản và SHA256 (`packaging/fill.sh`) rồi để kết quả ở artifact `package-manifests-<tag>` của lần chạy đó:

```
gh run download <run-id> -n package-manifests-v0.2.3 -D /tmp/manifests
```

Chỉ gửi đi **sau khi đã publish** bản phát hành, vì các trình cài đặt tải file từ link công khai.

- **Homebrew (macOS, Apple silicon):** chép `homebrew/gitty.rb` vào `Formula/gitty.rb` của repo `lettdev/homebrew-tap`. Người dùng cài bằng `brew install lettdev/tap/gitty`. Homebrew tải bằng curl nên macOS không chặn file chưa ký.
- **winget (Windows):** chép thư mục `winget/manifests/...` vào một fork của `microsoft/winget-pkgs`, kiểm tra bằng `winget validate --manifest <thư mục>` rồi mở pull request. Người dùng cài bằng `winget install lettdev.Gitty`.
- **Scoop (Windows):** chép `scoop/gitty.json` vào một bucket Scoop. Phần `autoupdate` giúp Scoop tự nhận bản mới.

