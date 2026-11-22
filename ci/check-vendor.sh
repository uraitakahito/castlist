#!/usr/bin/env bash
# 取り込んだ外の JavaScript（static/vendor/）が、npm に出ている物と同じかを見る。1 つでも違えば exit 1。
#   1. static/vendor/SOURCES.md の記録（package・version・tarball・integrity・path・sha256）を読む
#   2. npm の registry の記録の tarball と integrity が、SOURCES.md と同じ
#   3. tarball を落とし、その sha512 が integrity と同じ
#   4. tarball の中の path の file の sha256 が、SOURCES.md の sha256 と同じ
#   5. commit した file の sha256 も同じ。license の file も、tarball の中の物と同じ
# macOS の bash（3.2）でも動く書き方にする。curl と jq と openssl が要る。
# 使い方: ci/check-vendor.sh [commit した file（既定は SOURCES.md の file）]
#   引数は、見張りが違いを本当に当てるかを見る時に、書き替えた写しを渡すため。
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(dirname "$here")
sources="$root/static/vendor/SOURCES.md"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
fail=0
ng() {
  echo "NG: $*" >&2
  fail=1
}

[ -f "$sources" ] || { echo "NG: 記録が無い: $sources" >&2; exit 1; }

# SOURCES.md の表の `| 名 | \`値\` |` の行から、値を読む。
field() {
  local value
  value=$(sed -n "s/^| $1 | \`\([^\`]*\)\` |\$/\1/p" "$sources")
  [ -n "$value" ] || { echo "NG: SOURCES.md に $1 が無い" >&2; exit 1; }
  printf '%s\n' "$value"
}
sha256() {
  if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

package=$(field package)
version=$(field version)
tarball=$(field tarball)
integrity=$(field integrity)
path=$(field path)
file=$(field file)
want=$(field sha256)
license_path=$(field license_path)
license_file=$(field license_file)
committed=${1:-$root/$file}

# --- 2. registry の記録 ---
curl -fsSL -o "$work/meta.json" "https://registry.npmjs.org/$package/$version"
got_tarball=$(jq -r '.dist.tarball' "$work/meta.json")
got_integrity=$(jq -r '.dist.integrity' "$work/meta.json")
[ "$got_tarball" = "$tarball" ] || ng "registry の tarball が ${got_tarball}（記録は ${tarball}）"
[ "$got_integrity" = "$integrity" ] || ng "registry の integrity が ${got_integrity}（記録は ${integrity}）"

# --- 3. tarball ---
curl -fsSL -o "$work/package.tgz" "$tarball"
got_sha512="sha512-$(openssl dgst -sha512 -binary "$work/package.tgz" | openssl base64 -A)"
[ "$got_sha512" = "$integrity" ] || ng "tarball の sha512 が ${got_sha512}（記録は ${integrity}）"

# --- 4. tarball の中の file ---
mkdir "$work/x"
tar -xzf "$work/package.tgz" -C "$work/x" "$path" "$license_path"
got=$(sha256 "$work/x/$path")
[ "$got" = "$want" ] || ng "tarball の $path の sha256 が ${got}（記録は ${want}）"

# --- 5. commit した file ---
if [ -f "$committed" ]; then
  got=$(sha256 "$committed")
  [ "$got" = "$want" ] || ng "$committed の sha256 が ${got}（記録は ${want}）"
else
  ng "commit した file が無い: $committed"
fi
if [ -f "$root/$license_file" ]; then
  cmp -s "$work/x/$license_path" "$root/$license_file" || ng "$license_file が tarball の $license_path と違う"
else
  ng "license の file が無い: $root/$license_file"
fi

if [ "$fail" -ne 0 ]; then
  echo "取り込んだ file の確かめに落ちた: $package $version" >&2
  exit 1
fi
echo "取り込んだ file は npm の物と同じ: $package ${version}（$path → ${file}、sha256 ${want}）"
