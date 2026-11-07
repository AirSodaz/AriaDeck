#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: package-macos.sh VERSION ARCH" >&2
  exit 2
fi

version="$1"
arch="$2"
case "$arch" in
  arm64|x86_64) ;;
  *) echo "unsupported macOS architecture: $arch" >&2; exit 2 ;;
esac

root="$(cd "$(dirname "$0")/.." && pwd)"
binary="$root/target/release/ariadeck-desktop"
[[ -f "$binary" ]] || { echo "release binary is missing: $binary" >&2; exit 1; }
actual_arch="$(lipo -archs "$binary")"
[[ "$actual_arch" == "$arch" ]] || { echo "expected $arch binary, got $actual_arch" >&2; exit 1; }

dist="$root/dist"
mkdir -p "$dist"
output="$dist/AriaDeck-$version-macos-$arch.dmg"
[[ ! -e "$output" ]] || { echo "refusing to overwrite existing artifact: $output" >&2; exit 1; }
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
app="$work/AriaDeck.app"
bundle="$app/Contents"
mkdir -p "$bundle/MacOS" "$bundle/Resources"
install -m 0755 "$binary" "$bundle/MacOS/AriaDeck"
install -m 0644 "$root/LICENSE" "$root/THIRD_PARTY_NOTICES.md" "$bundle/Resources/"

cat > "$bundle/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>AriaDeck</string>
  <key>CFBundleIdentifier</key><string>com.ariadeck.desktop</string>
  <key>CFBundleName</key><string>AriaDeck</string>
  <key>CFBundleDisplayName</key><string>AriaDeck</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

plutil -lint "$bundle/Info.plist"
mkdir "$work/image"
ditto "$app" "$work/image/AriaDeck.app"
ln -s /Applications "$work/image/Applications"
hdiutil create -quiet -volname AriaDeck -srcfolder "$work/image" -format UDZO "$work/AriaDeck-$version-macos-$arch.dmg"
hdiutil verify "$work/AriaDeck-$version-macos-$arch.dmg"
mount="$work/mount"
mkdir "$mount"
hdiutil attach -quiet -nobrowse -readonly -mountpoint "$mount" "$work/AriaDeck-$version-macos-$arch.dmg"
trap 'hdiutil detach -quiet "$mount" || true; rm -rf "$work"' EXIT
verified_app="$mount/AriaDeck.app/Contents"
for path in Info.plist MacOS/AriaDeck Resources/LICENSE Resources/THIRD_PARTY_NOTICES.md; do
  [[ -f "$verified_app/$path" ]] || { echo "app bundle is missing $path" >&2; exit 1; }
done
plutil -lint "$verified_app/Info.plist"
[[ "$(lipo -archs "$verified_app/MacOS/AriaDeck")" == "$arch" ]] || { echo "packaged binary architecture mismatch" >&2; exit 1; }
mv "$work/AriaDeck-$version-macos-$arch.dmg" "$output"
echo "Created and verified $output"
