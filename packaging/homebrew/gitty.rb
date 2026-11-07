# Homebrew formula for the terminal app. Copy into your tap repository
# (github.com/<owner>/homebrew-tap/Formula/gitty.rb) and fill in the
# placeholders from the release assets and their .sha256 files.
class Gitty < Formula
  desc "Friendly git client for the terminal"
  homepage "https://github.com/lettdev/gitty"
  version "VERSION"

  depends_on arch: :arm64
  depends_on :macos

  url "https://github.com/lettdev/gitty/releases/download/vVERSION/gitty-vVERSION-macos-arm64.tar.gz"
  sha256 "SHA256_MACOS_ARM64"

  depends_on "git"

  def install
    bin.install "gitty"
    doc.install "README.md", "config.example.toml"
  end

  test do
    assert_match "gitty", shell_output("#{bin}/gitty --version")
  end
end
