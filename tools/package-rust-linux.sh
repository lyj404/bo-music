#!/bin/sh
set -eu

if [ "$#" -lt 2 ] || [ "$#" -gt 3 ]; then
    echo "Usage: sh tools/package-rust-linux.sh BINARY OUTPUT_DIRECTORY [VERSION]" >&2
    exit 2
fi

binary=$1
output=$2
version=${3:-0.1.0}
if ! printf '%s\n' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'; then
    echo "Version must use MAJOR.MINOR.PATCH" >&2
    exit 2
fi
case "$(uname -m)" in
    x86_64) arch=x64 ;;
    aarch64) arch=arm64 ;;
    *) echo "Unsupported architecture" >&2; exit 2 ;;
esac

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
name="BowenRust_${version}_linux_${arch}"
mkdir -p -- "$output"
for path in "$output/$name" "$output/$name.tar.gz" "$output/${name}_SHA256SUMS.txt"; do
    if [ -e "$path" ]; then
        echo "Output already exists at $path" >&2
        exit 1
    fi
done

mkdir -- "$output/$name"
install -m 755 -- "$binary" "$output/$name/bowen-gpui"
install -m 644 -- "$root/assets/Ripple.png" "$output/$name/bowen-rust.png"
install -m 644 -- "$root/LICENSE" \
    "$root/installer/bowen-rust.desktop" "$output/$name/"
tar -czf "$output/$name.tar.gz" -C "$output" "$name"
(cd -- "$output" && sha256sum "$name.tar.gz" > "${name}_SHA256SUMS.txt")
