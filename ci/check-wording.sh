#!/usr/bin/env bash
# 公開する字（README・CHANGELOG・本の docs/src）に、読む人の機械や、ある人の手元を前提にした言い方が無いかを見る。
# castlist はだれの機械でも動かせる物として公開するので、「この Mac」のように 1 台を指す字や、ある人の home の path を書かない。
# 鍵と値の組（API の labels）は「ラベル」と呼ぶ。前の呼び方の「札」は書かない（Product Manager の答え）。
# 画面の字の決まり（稼働中・停止など）は、画面と一緒に callboard に移した（callboard の ci/check-wording.sh）。
# データの名は「トークンの使用量」。前の呼び方の「費え」は書かない（Product Manager の答え。t-5dhg）。
# 「費え」は、出した版の記録（CHANGELOG.md の前の節と、替えたことを書く節）に残るので、CHANGELOG.md では見ない。
# 見る形: 語の Mac（macOS は当てない）・/Users/<名>・札・費え（CHANGELOG.md を除く）
# 1 つでも当たれば、file と行を出して exit 1。
# 使い方: ci/check-wording.sh [path…]（既定は README.md・CHANGELOG.md・docs/src）
set -euo pipefail

if [ "$#" -eq 0 ]; then
  root=$(cd "$(dirname "$0")/.." && pwd)
  set -- "$root/README.md" "$root/CHANGELOG.md" "$root/docs/src"
fi

files=$(find "$@" -type f | LC_ALL=C sort)
if [ -z "$files" ]; then
  echo "NG: 見る file が無い: $*" >&2
  exit 1
fi

# shellcheck disable=SC2086
if grep -nE '(^|[^A-Za-z])Mac([^A-Za-z]|$)|/Users/[A-Za-z]|札' $files >&2; then
  echo "NG: 公開する字に、1 台の機械やある人の手元を指す言い方か、前の呼び方「札」がある（上の行）。「castlist を動かす機械」「host」のように、どの機械でも通じる言い方にし、鍵と値の組は「ラベル」と書く" >&2
  exit 1
fi
rest=$(printf '%s\n' "$files" | grep -vE '(^|/)CHANGELOG\.md$' || true)
# shellcheck disable=SC2086
if [ -n "$rest" ] && grep -Hn '費え' $rest >&2; then
  echo "NG: 公開する字に、前の呼び方「費え」がある（上の行）。「トークンの使用量」と書く" >&2
  exit 1
fi
echo "ok: 公開する字に、1 台の機械やある人の手元を指す言い方と「札」「費え」は無い"
