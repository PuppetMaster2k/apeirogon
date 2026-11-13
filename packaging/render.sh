#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
set -eu

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
    echo "Usage: $0 <vMAJOR.MINOR.PATCH> [output-dir]" >&2
    exit 1
fi
tag=$1
version=$(printf '%s\n' "$tag" | sed -n 's/^v\([0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*\)$/\1/p')
if [ -z "$version" ]; then
    echo "Invalid release tag: $tag" >&2
    exit 1
fi
templates=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
output=${2:-"$templates/rendered"}
downloads=$(mktemp -d)
trap 'rm -rf "$downloads"' EXIT HUP INT TERM
repo=PuppetMaster2k/apeirogon
gh release download "$tag" --repo "$repo" --pattern '*.sha256' --dir "$downloads"

checksum() {
    archive="apeirogon-$tag-$1"
    read -r hash filename extra < "$downloads/$archive.sha256"
    filename=${filename#\*}
    case "$hash" in
        *[!0-9a-fA-F]*|'') echo "Invalid checksum for $archive" >&2; exit 1 ;;
    esac
    if [ "${#hash}" -ne 64 ] || [ "$filename" != "$archive.$2" ] || [ -n "$extra" ]; then
        echo "Invalid checksum entry for $archive" >&2
        exit 1
    fi
    printf '%s' "$hash"
}
mac_sha=$(checksum aarch64-apple-darwin tar.gz)
linux_sha=$(checksum x86_64-unknown-linux-gnu tar.gz)
windows_sha=$(checksum x86_64-pc-windows-msvc zip)
gh api "https://raw.githubusercontent.com/$repo/$tag/packaging/linux/apeirogon.desktop" > "$downloads/apeirogon.desktop"
desktop_sha=$(sha256sum "$downloads/apeirogon.desktop")
desktop_sha=${desktop_sha%% *}
gh api "https://raw.githubusercontent.com/$repo/$tag/crates/a7n-platform/icons/linux/hicolor/scalable/apps/apeirogon.svg" > "$downloads/apeirogon.svg"
icon_sha=$(sha256sum "$downloads/apeirogon.svg")
icon_sha=${icon_sha%% *}

mkdir -p "$output"
for template in homebrew/apeirogon.rb scoop/apeirogon.json aur/PKGBUILD aur/.SRCINFO; do
    sed -e "s/@TAG@/$tag/g" -e "s/@VERSION@/$version/g" \
        -e "s/@MAC_SHA256@/$mac_sha/g" -e "s/@LINUX_SHA256@/$linux_sha/g" \
        -e "s/@WINDOWS_SHA256@/$windows_sha/g" -e "s/@DESKTOP_SHA256@/$desktop_sha/g" \
        -e "s/@ICON_SHA256@/$icon_sha/g" \
        "$templates/$template.in" > "$output/${template##*/}"
done
