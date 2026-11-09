#!/usr/bin/env bash
# Fill the package-manager templates for one release.
#   packaging/fill.sh <version, e.g. 0.2.3> <dir with the release .sha256 files> <out dir>
# Writes <out>/homebrew/gitty.rb, <out>/scoop/gitty.json and
# <out>/winget/manifests/l/lettdev/Gitty/<version>/*.yaml, ready to submit.
set -euo pipefail
version="${1#v}"
sums="$2"
out="$3"
here="$(cd "$(dirname "$0")" && pwd)"

sha() {
  local f="$sums/$1.sha256"
  [ -f "$f" ] || { echo "missing $f" >&2; exit 1; }
  awk '{print $1}' "$f"
}
mac=$(sha "gitty-v$version-macos-arm64.tar.gz")
win=$(sha "gitty-v$version-windows-x64.zip")
today=$(date -u +%Y-%m-%d)

fill() {
  sed -e "s/VERSION/$version/g" -e "s/SHA256_MACOS_ARM64/$mac/g" \
      -e "s/SHA256_WINDOWS_X64/$win/g" -e "s/RELEASE_DATE/$today/g" "$1" > "$2"
}

mkdir -p "$out/homebrew" "$out/scoop"
fill "$here/homebrew/gitty.rb" "$out/homebrew/gitty.rb"
fill "$here/scoop/gitty.json" "$out/scoop/gitty.json"
w="$out/winget/manifests/l/lettdev/Gitty/$version"
mkdir -p "$w"
for f in "$here"/winget/*.yaml; do
  # the template comments only matter in the repo
  fill "$f" /dev/stdout | grep -v '^# Template:' | grep -v '^# (from gitty' > "$w/$(basename "$f")"
done
if grep -rn 'VERSION\|SHA256_\|RELEASE_DATE' "$out"; then
  echo "unfilled placeholders above" >&2
  exit 1
fi
echo "filled $version into $out"
