#!/usr/bin/env bash
# ci/check-release-version.sh の試験。一時の folder に Cargo.toml と CHANGELOG.md を置き、通る形と落ちる形で打つ。
# 期待と違えば、その場合を出して exit 1。CI の job `test` が流す。
# 使い方: ci/test-check-release-version.sh
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
script="$here/check-release-version.sh"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
fail=0

# repo の形を作る: $1 は folder、$2 は Cargo.toml の中身、$3 は CHANGELOG.md の中身（空なら置かない）。
repo() {
  mkdir -p "$1"
  printf '%s\n' "$2" >"$1/Cargo.toml"
  if [ -n "$3" ]; then printf '%s\n' "$3" >"$1/CHANGELOG.md"; fi
}
# $1 は場合の名、$2 は期待する exit（0 か 1）、残りは script の引数。
# 落ちる場合は、exit 1 に加えて、script 自身の「NG: 」の行を求める（script が無くて落ちたのを、通ったと見ない）。
expect() {
  local name=$1 want=$2 got=0
  shift 2
  "$script" "$@" >"$work/out.txt" 2>&1 || got=$?
  if [ "$got" = 1 ] && ! grep -q '^NG: ' "$work/out.txt"; then got="1（NG: の行が無い）"; fi
  if [ "$got" = "$want" ]; then
    echo "ok: ${name}（exit ${got}）"
  else
    echo "NG: ${name}: exit ${got}（期待は ${want}）" >&2
    sed 's/^/  /' "$work/out.txt" >&2
    fail=1
  fi
}

cargo='[package]
name = "castlist"
version = "0.1.0"

[dependencies]
serde = { version = "1" }'
changelog='# 変えたこと

## 0.1.0

- 初めの版。'

repo "$work/good" "$cargo" "$changelog"
expect "tag と版が同じ" 0 v0.1.0 "$work/good"
expect "tag の版が違う" 1 v0.1.1 "$work/good"
expect "tag の頭に v が無い" 1 0.1.0 "$work/good"
expect "tag の後ろに字が足してある" 1 v0.1.0-rc1 "$work/good"
expect "tag が空" 1 "" "$work/good"

# 初めの `version = "…"` の行だけを見る（後ろの節の version は見ない）。
repo "$work/later" "$cargo
[package.metadata.x]
version = \"9.9.9\"" "$changelog"
expect "後ろの節の version は見ない" 0 v0.1.0 "$work/later"
expect "後ろの節の version に合わせても通らない" 1 v9.9.9 "$work/later"

repo "$work/nolog" "$cargo" ""
expect "CHANGELOG.md が無い" 1 v0.1.0 "$work/nolog"
repo "$work/nosection" "$cargo" '# 変えたこと

## 0.0.9

- 前の版。'
expect "CHANGELOG.md にその版の節が無い" 1 v0.1.0 "$work/nosection"
repo "$work/prefix" "$cargo" '# 変えたこと

## 0.1.0-rc1

- 前の版。'
expect "節の名の頭だけが同じ" 1 v0.1.0 "$work/prefix"

repo "$work/noversion" '[package]
name = "castlist"' "$changelog"
expect "Cargo.toml に版が無い" 1 v0.1.0 "$work/noversion"

if [ "$fail" -ne 0 ]; then
  echo "check-release-version.sh の試験に落ちた" >&2
  exit 1
fi
echo "check-release-version.sh の試験: 通った"
