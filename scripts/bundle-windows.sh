#!/usr/bin/env bash

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

target_dir="${CARGO_TARGET_DIR:-target}"
version="$(cargo metadata --no-deps --format-version 1 | sed -n 's/.*"name":"hakata","version":"\([^"]*\)".*/\1/p')"
target_triple="$(rustc -vV | sed -n 's/^host: //p')"
package="hakata-${version}-${target_triple}"
archive="$target_dir/release/$package.zip"
staging="$(mktemp -d)"
trap 'rm -rf -- "$staging"' EXIT

cargo build --locked --release --bin hakata

package_dir="$staging/$package"
mkdir -p "$package_dir"
cp "$target_dir/release/hakata.exe" "$package_dir/hakata.exe"

mkdir -p "$(dirname "$archive")"
rm -f "$archive"
tar -C "$staging" -acf "$archive" "$package"
printf 'Created %s\n' "$archive"
