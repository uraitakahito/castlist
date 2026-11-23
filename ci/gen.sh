#!/usr/bin/env bash
# 定義（model/）から、server の骨（crates/castlist-sdk/）を作り直す。生成した物は repo に入れ、手で直さない。
#   ci/gen.sh           作り直して crates/castlist-sdk/ を置き換える
#   ci/gen.sh --check   作り直した物と、入れてある物が同じかを見る（CI が流す）。違えば exit 1
#   ci/gen.sh --diff    定義を、定義の入った最新の release の tag（v*）と比べ、壊す変わり（DANGER 以上）があれば exit 1（CI が流す）。
#                       定義の入った tag がまだ無ければ、比べずに通す
# 要る物: curl・unzip・shasum か sha256sum。Smithy の CLI は JRE を同梱しているので、Java は要らない。
# 生成器の版は model/smithy-build.json に書く。公開から 2 日たっていない Maven の物は使わない（crate と pnpm と同じ決まり）。
# macOS の bash（3.2）でも動く書き方にする。
set -euo pipefail

SMITHY_VERSION=1.75.0
# release の digest（https://github.com/smithy-lang/smithy/releases/tag/1.75.0）。版を上げる時は一緒に替える。
sha_for() {
  case "$1" in
    darwin-aarch64) echo c6d32931b8eaf5e93c159c870387b4e6d8c97363c8c6bb09061291c93cdc9181 ;;
    darwin-x86_64) echo a89ca22c941f92476fef804ac54e4d3886204da1f0554a18abd8c66073b7519e ;;
    linux-aarch64) echo 8561f72386464ac8cfb0ccd3e864a59199e134f26d8ebf6bd264eebe1aa58098 ;;
    linux-x86_64) echo c8d448426061586a0afb93830151838cb92a73e45d9d3733c3367ca905eb32c1 ;;
    *) return 1 ;;
  esac
}
MIN_AGE_SECONDS=$((2 * 24 * 60 * 60))

root=$(cd "$(dirname "$0")/.." && pwd)
out="$root/crates/castlist-sdk"
check=false
diff_mode=false
case "${1:-}" in
  --check) check=true ;;
  --diff) diff_mode=true ;;
  "") ;;
  *) echo "使い方: ci/gen.sh [--check|--diff]" >&2; exit 2 ;;
esac

ng() {
  echo "NG: $*" >&2
  exit 1
}
sha256() { if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi; }

# 1. Smithy の CLI を取る（版と digest を確かめ、cache に置く）。
os=$(uname -s | tr '[:upper:]' '[:lower:]')
arch=$(uname -m)
[ "$arch" = arm64 ] && arch=aarch64
platform="$os-$arch"
want=$(sha_for "$platform") || ng "Smithy の CLI の無い機械: $platform"
cache="${XDG_CACHE_HOME:-$HOME/.cache}/castlist-smithy/$SMITHY_VERSION-$platform"
smithy="$cache/smithy-cli-$platform/bin/smithy"
if [ ! -x "$smithy" ]; then
  mkdir -p "$cache"
  zip="$cache/cli.zip"
  curl -fsSL -o "$zip" "https://github.com/smithy-lang/smithy/releases/download/$SMITHY_VERSION/smithy-cli-$platform.zip"
  got=$(sha256 "$zip")
  [ "$got" = "$want" ] || ng "Smithy の CLI の digest が違う（$got。$want のはず）"
  unzip -q -o "$zip" -d "$cache"
  rm -f "$zip"
fi

# 2. 生成器（Maven の物）が、公開から 2 日たっているか。
now=$(date -u +%s)
deps=$(sed -n 's/^ *"\([a-zA-Z0-9._-]*:[a-zA-Z0-9._-]*:[0-9][a-zA-Z0-9._-]*\)",\{0,1\}$/\1/p' "$root/model/smithy-build.json")
[ -n "$deps" ] || ng "model/smithy-build.json に Maven の依存が無い"
for dep in $deps; do
  group=${dep%%:*}
  rest=${dep#*:}
  artifact=${rest%%:*}
  version=${rest#*:}
  pom="https://repo1.maven.org/maven2/$(echo "$group" | tr . /)/$artifact/$version/$artifact-$version.pom"
  modified=$(curl -fsSI "$pom" | tr -d '\r' | sed -n 's/^[Ll]ast-[Mm]odified: //p')
  [ -n "$modified" ] || ng "$dep の公開の時刻を読めない（$pom）"
  if date -u -d "$modified" +%s >/dev/null 2>&1; then
    published=$(date -u -d "$modified" +%s)
  else
    published=$(LC_ALL=C date -u -j -f '%a, %d %b %Y %H:%M:%S GMT' "$modified" +%s)
  fi
  [ $((now - published)) -ge $MIN_AGE_SECONDS ] || ng "$dep は公開から 2 日たっていない（$modified）"
done

# 3. 壊す変わりを見る（--diff）。
if $diff_mode; then
  base=""
  for tag in $(git -C "$root" tag --list 'v*' --sort=-version:refname); do
    if git -C "$root" cat-file -e "$tag:model/smithy-build.json" 2>/dev/null; then
      base=$tag
      break
    fi
  done
  if [ -z "$base" ]; then
    echo "定義の入った release の tag がまだ無いので、比べない"
    exit 0
  fi
  (cd "$root/model" && "$smithy" diff --mode git --old "$base" --severity DANGER) || ng "定義が $base から壊す形に変わった（上の DANGER・ERROR）"
  echo "定義は $base から壊す形に変わっていない"
  exit 0
fi

# 4. 作る。
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cp -R "$root/model/." "$work/"
rm -rf "$work/build"
(cd "$work" && "$smithy" build --quiet >"$work/build.log" 2>&1) || {
  cat "$work/build.log" >&2
  ng "smithy build が落ちた"
}
made="$work/build/smithy/source/rust-server-codegen"
[ -f "$made/Cargo.toml" ] || ng "server の骨が作られていない"

if $check; then
  if ! diff -r "$made" "$out" >"$work/diff.txt"; then
    head -50 "$work/diff.txt" >&2
    ng "crates/castlist-sdk/ が定義と合わない。ci/gen.sh を流して、作り直した物を commit する"
  fi
  echo "crates/castlist-sdk/ は定義（model/）と合っている"
else
  rm -rf "$out"
  mkdir -p "$(dirname "$out")"
  cp -R "$made" "$out"
  echo "作り直した: crates/castlist-sdk/"
fi
