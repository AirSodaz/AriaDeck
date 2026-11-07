#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: package-linux.sh VERSION LINUXDEPLOY_APPIMAGE" >&2
  exit 2
fi

version="$1"
linuxdeploy="$2"
root="$(cd "$(dirname "$0")/.." && pwd)"
dist="$root/dist"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

binary="$root/target/release/ariadeck-desktop"
desktop_file="$root/packaging/linux/ariadeck.desktop"
icon_file="$root/apps/ariadeck-desktop/assets/icon.svg"

for file in "$binary" "$desktop_file" "$icon_file" "$root/LICENSE" "$root/THIRD_PARTY_NOTICES.md" "$linuxdeploy"; do
  if [[ ! -f "$file" ]]; then
    echo "required packaging input is missing: $file" >&2
    exit 1
  fi
done
desktop-file-validate "$desktop_file"

mkdir -p "$dist"
appdir="$work/AriaDeck.AppDir"
mkdir -p \
  "$appdir/usr/bin" \
  "$appdir/usr/share/doc/ariadeck"
install -m 0755 "$binary" "$appdir/usr/bin/ariadeck-desktop"
install -m 0644 "$icon_file" "$work/ariadeck.svg"
install -m 0644 "$root/LICENSE" "$root/THIRD_PARTY_NOTICES.md" "$appdir/usr/share/doc/ariadeck/"

appimage="$dist/AriaDeck-$version-x86_64.AppImage"
(
  cd "$work"
  APPIMAGE_EXTRACT_AND_RUN=1 \
    "$linuxdeploy" --appdir "$appdir" \
      --executable "$appdir/usr/bin/ariadeck-desktop" \
      --desktop-file "$desktop_file" \
      --icon-file "$work/ariadeck.svg" \
      --output appimage
)
generated_appimage="$(find "$work" -maxdepth 1 -type f -name '*.AppImage' -print -quit)"
if [[ -z "$generated_appimage" ]]; then
  echo "linuxdeploy did not produce an AppImage" >&2
  exit 1
fi
mv "$generated_appimage" "$appimage"
chmod 0755 "$appimage"

debroot="$work/deb-root"
mkdir -p \
  "$debroot/DEBIAN" \
  "$debroot/usr/bin" \
  "$debroot/usr/share/applications" \
  "$debroot/usr/share/icons/hicolor/scalable/apps" \
  "$debroot/usr/share/doc/ariadeck"
install -m 0755 "$binary" "$debroot/usr/bin/ariadeck-desktop"
install -m 0644 "$desktop_file" "$debroot/usr/share/applications/ariadeck.desktop"
install -m 0644 "$icon_file" "$debroot/usr/share/icons/hicolor/scalable/apps/ariadeck.svg"
install -m 0644 "$root/LICENSE" "$root/THIRD_PARTY_NOTICES.md" "$debroot/usr/share/doc/ariadeck/"

mkdir -p "$work/debian"
cat > "$work/debian/control" <<EOF
Source: ariadeck
Section: net
Priority: optional
Maintainer: AriaDeck contributors

Package: ariadeck
Architecture: amd64
Description: Native desktop client for aria2
 AriaDeck is a native Rust desktop client that controls aria2 over JSON-RPC.
EOF
depends="$(cd "$work" && dpkg-shlibdeps -O -e"$binary" | sed -n 's/^shlibs:Depends=//p')"
if [[ -z "$depends" ]]; then
  echo "dpkg-shlibdeps did not report runtime dependencies" >&2
  exit 1
fi
cat > "$debroot/DEBIAN/control" <<EOF
Package: ariadeck
Version: $version
Section: net
Priority: optional
Architecture: amd64
Maintainer: AriaDeck contributors
Depends: $depends
Description: Native desktop client for aria2
 AriaDeck is a native Rust desktop client that controls aria2 over JSON-RPC.
EOF

deb="$dist/ariadeck_$version"_amd64.deb
dpkg-deb --build --root-owner-group "$debroot" "$deb"

verify_appdir="$work/appimage-extract"
mkdir -p "$verify_appdir"
(
  cd "$verify_appdir"
  "$appimage" --appimage-extract >/dev/null
)
for path in \
  squashfs-root/AppRun \
  squashfs-root/ariadeck.desktop \
  squashfs-root/.DirIcon \
  squashfs-root/usr/share/icons/hicolor/scalable/apps/ariadeck.svg \
  squashfs-root/usr/bin/ariadeck-desktop \
  squashfs-root/usr/share/doc/ariadeck/LICENSE \
  squashfs-root/usr/share/doc/ariadeck/THIRD_PARTY_NOTICES.md; do
  [[ -e "$verify_appdir/$path" ]] || { echo "AppImage is missing $path" >&2; exit 1; }
done
if ldd "$verify_appdir/squashfs-root/usr/bin/ariadeck-desktop" | grep -q 'not found'; then
  echo "AppImage desktop binary has unresolved shared libraries" >&2
  ldd "$verify_appdir/squashfs-root/usr/bin/ariadeck-desktop" >&2
  exit 1
fi

dpkg-deb --info "$deb" >/dev/null
if [[ -z "$(dpkg-deb --field "$deb" Depends)" ]]; then
  echo "Debian package is missing runtime dependencies" >&2
  exit 1
fi
deb_contents="$(dpkg-deb --contents "$deb")"
for path in \
  ./usr/bin/ariadeck-desktop \
  ./usr/share/applications/ariadeck.desktop \
  ./usr/share/icons/hicolor/scalable/apps/ariadeck.svg \
  ./usr/share/doc/ariadeck/LICENSE \
  ./usr/share/doc/ariadeck/THIRD_PARTY_NOTICES.md; do
  grep -Fq "$path" <<<"$deb_contents" || { echo "Debian package is missing $path" >&2; exit 1; }
done

printf 'Created and verified %s\n%s\n' "$appimage" "$deb"
