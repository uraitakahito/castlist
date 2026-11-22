#!/usr/bin/env bash
# 公開する字（README・CHANGELOG・本の docs/src・画面の templates）に、読む人の機械や、ある人の手元を前提にした言い方が無いかを見る。
# castlist はだれの機械でも動かせる物として公開するので、「この Mac」のように 1 台を指す字や、ある人の home の path を書かない。
# 見る形: 語の Mac（macOS は当てない）・/Users/<名>
# 1 つでも当たれば、file と行を出して exit 1。
# 使い方: ci/check-wording.sh [path…]（既定は README.md・CHANGELOG.md・docs/src・templates）
set -euo pipefail

if [ "$#" -eq 0 ]; then
  root=$(cd "$(dirname "$0")/.." && pwd)
  set -- "$root/README.md" "$root/CHANGELOG.md" "$root/docs/src" "$root/templates"
fi

files=$(find "$@" -type f | LC_ALL=C sort)
if [ -z "$files" ]; then
  echo "NG: 見る file が無い: $*" >&2
  exit 1
fi

# shellcheck disable=SC2086
if grep -nE '(^|[^A-Za-z])Mac([^A-Za-z]|$)|/Users/[A-Za-z]' $files >&2; then
  echo "NG: 公開する字に、1 台の機械やある人の手元を指す言い方がある（上の行）。「castlist を動かす機械」「host」のように、どの機械でも通じる言い方にする" >&2
  exit 1
fi
echo "ok: 公開する字に、1 台の機械やある人の手元を指す言い方は無い"
