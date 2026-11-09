# Homebrew formula for the terminal app (Apple silicon), for the tap
# github.com/lettdev/homebrew-tap as Formula/gitty.rb. The Release workflow
# fills in the placeholders (packaging/fill.sh); git itself comes with macOS.
class Gitty < Formula
  desc "Friendly git client for the terminal"
  homepage "https://github.com/lettdev/gitty"
  url "https://github.com/lettdev/gitty/releases/download/vVERSION/gitty-vVERSION-macos-arm64.tar.gz"
  sha256 "SHA256_MACOS_ARM64"
  license "MIT"

  depends_on arch: :arm64
  depends_on :macos

  def install
    bin.install "gitty"
    doc.install "README.md", "config.example.toml"
    prefix.install "LICENSE"
  end

  test do
    assert_match "gitty", shell_output("#{bin}/gitty --version")
  end
end
