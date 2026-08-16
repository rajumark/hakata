#!/usr/bin/env bash

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

target_dir="${CARGO_TARGET_DIR:-target}"
version="$(cargo metadata --no-deps --format-version 1 | sed -n 's/.*"name":"hakata","version":"\([^"]*\)".*/\1/p')"
target_triple="$(rustc -vV | sed -n 's/^host: //p')"
bundle="$target_dir/release/Hakata.app"
contents="$bundle/Contents"
dist="$root/dist"
dmg_name="Hakata-${version}-${target_triple}.dmg"
zip_name="Hakata-${version}-${target_triple}.zip"

cargo build --locked --release --bin hakata

rm -rf "$bundle"
mkdir -p "$contents/MacOS"
cp resources/macos/Info.plist "$contents/Info.plist"
cp "$target_dir/release/hakata" "$contents/MacOS/Hakata"
plutil -replace CFBundleVersion -string "$version" "$contents/Info.plist"
plutil -replace CFBundleShortVersionString -string "$version" "$contents/Info.plist"

if [ -n "${HAKATA_SIGNING_IDENTITY:-}" ]; then
  codesign --force --options runtime --timestamp --sign "$HAKATA_SIGNING_IDENTITY" "$bundle"
else
  codesign --force --sign - "$bundle"
fi
codesign --verify --deep --strict --verbose=2 "$bundle"

mkdir -p "$dist"
rm -f "$dist/$zip_name" "$dist/$dmg_name"
ditto -c -k --keepParent "$bundle" "$dist/$zip_name"
create-dmg \
  --volname "Hakata" \
  --window-pos 200 120 \
  --window-size 660 400 \
  --text-size 13 \
  --icon-size 128 \
  --icon "Hakata.app" 180 178 \
  --hide-extension "Hakata.app" \
  --app-drop-link 480 178 \
  --filesystem APFS \
  --format ULFO \
  --no-internet-enable \
  --overwrite \
  "$dist/$dmg_name" \
  "$bundle"

printf 'Created %s and %s\n' "$dist/$zip_name" "$dist/$dmg_name"
