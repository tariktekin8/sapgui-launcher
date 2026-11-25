#!/usr/bin/env bash
# Prints latest.json for the updater. Usage: latest-json.sh <version> <folder with the release files>
# Fails if a platform's signature is missing, so no platform silently loses its updates.
set -euo pipefail
version=$1
dir=$2
base="https://github.com/tariktekin8/sapgui-launcher/releases/download/v$version"
app="SAP.GUI.Launcher"

platforms='{}'
add() { # <platform> <file>
  platforms=$(jq --arg p "$1" --arg url "$base/$2" --rawfile sig "$dir/$2.sig" \
    '.[$p] = {signature: ($sig | rtrimstr("\n")), url: $url}' <<<"$platforms")
}

add darwin-aarch64 "${app}_aarch64.app.tar.gz"
add darwin-aarch64-app "${app}_aarch64.app.tar.gz"
add windows-x86_64 "${app}_${version}_x64_en-US.msi"
add windows-x86_64-msi "${app}_${version}_x64_en-US.msi"
add windows-x86_64-nsis "${app}_${version}_x64-setup.exe"
add linux-x86_64 "${app}_${version}_amd64.AppImage"
add linux-x86_64-appimage "${app}_${version}_amd64.AppImage"
add linux-x86_64-deb "${app}_${version}_amd64.deb"
add linux-x86_64-rpm "${app}-${version}-1.x86_64.rpm"

jq -n --arg v "$version" --arg date "$(date -u +%Y-%m-%dT%H:%M:%SZ)" --argjson p "$platforms" \
  '{version: $v, notes: "Install steps are in the README.", pub_date: $date, platforms: $p}'
