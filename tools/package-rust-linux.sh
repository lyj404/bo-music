#!/bin/sh
set -eu

if [ "$#" -lt 2 ] || [ "$#" -gt 3 ]; then
    echo "Usage: sh tools/package-rust-linux.sh BINARY OUTPUT_DIRECTORY [VERSION]" >&2
    exit 2
fi

binary=$1
output=$2
version=${3:-0.1.0}
if ! printf '%s\n' "$version" | grep -Eq '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z]+([.-][0-9A-Za-z]+)*)?$'; then
    echo "Version must use MAJOR.MINOR.PATCH with an optional prerelease suffix" >&2
    exit 2
fi
case "$(uname -m)" in
    x86_64) arch=x64 ;;
    *) echo "Only Linux x86_64 packages are supported" >&2; exit 2 ;;
esac
command -v dpkg-deb >/dev/null 2>&1 || { echo "dpkg-deb is required" >&2; exit 1; }
test -f "$binary" || { echo "Binary not found: $binary" >&2; exit 1; }

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
name="bo-music_${version}_linux_${arch}"
deb_version=$(printf '%s' "$version" | tr '-' '~')
deb="$output/bo-music_${version}_amd64.deb"
mkdir -p -- "$output"
for path in "$output/$name" "$output/$name.tar.gz" "$deb" "$output/${name}_SHA256SUMS.txt"; do
    if [ -e "$path" ]; then
        echo "Output already exists at $path" >&2
        exit 1
    fi
done
stage=$(mktemp -d)
trap 'rm -rf -- "$stage"' EXIT HUP INT TERM
chmod 755 "$stage"

mkdir -- "$output/$name"
install -m 755 -- "$binary" "$output/$name/bo-music"
install -m 644 -- "$root/assets/bo_music.png" "$output/$name/bo_music.png"
install -m 644 -- "$root/LICENSE" \
    "$root/installer/bo_music.desktop" "$output/$name/"
tar -czf "$output/$name.tar.gz" -C "$output" "$name"

install -Dm755 -- "$binary" "$stage/usr/bin/bo-music"
install -Dm644 -- "$root/installer/bo_music.desktop" "$stage/usr/share/applications/bo_music.desktop"
install -Dm644 -- "$root/assets/bo_music.png" "$stage/usr/share/icons/hicolor/256x256/apps/bo_music.png"
install -Dm644 -- "$root/LICENSE" "$stage/usr/share/doc/bo-music/copyright"
mkdir -p "$stage/DEBIAN"
installed_size=$(du -sk "$stage/usr" | cut -f1)
cat > "$stage/DEBIAN/control" <<EOF
Package: bo-music
Version: $deb_version
Section: sound
Priority: optional
Architecture: amd64
Maintainer: bo-music maintainers
Installed-Size: $installed_size
Depends: libc6 (>= 2.39), libgcc-s1, libxcb1, libxkbcommon0, libxkbcommon-x11-0, libx11-6, libfontconfig1, libvulkan1, libwayland-client0, libsecret-1-0, libmpv2, dbus
Recommends: pulseaudio-utils, mesa-vulkan-drivers
Description: Modern Fluent-style music player
 A desktop music player with playlists, lyrics and audio effects.
EOF
dpkg-deb --root-owner-group --build "$stage" "$deb"
(cd -- "$output" && sha256sum "$name.tar.gz" "bo-music_${version}_amd64.deb" > "${name}_SHA256SUMS.txt")
