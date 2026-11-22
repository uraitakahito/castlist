#!/usr/bin/env bash
# 公開する字（README・CHANGELOG・本の docs/src・画面の templates）に、読む人の機械や、ある人の手元を前提にした言い方が無いかを見る。
# castlist はだれの機械でも動かせる物として公開するので、「この Mac」のように 1 台を指す字や、ある人の home の path を書かない。
# 鍵と値の組（API の labels）は「ラベル」と呼ぶ。前の呼び方の「札」は書かない（Product Manager の答え）。
# 画面の字も、Product Manager が答えた字にそろえ、前の字は書かない: アクターの状態は「稼働中・停止」（前は「居るか」「いる・いない」「居た間」）、
# 図は「系図・タイムライン・相関図」で 3 つのまとめは「図 A・B・C」（前は「時間の帯」「関係の網」「相関図 A・B・C」）、
# 送り手の時計のずれは「時刻ずれ」（前は「時計のずれた行」）、絞りの枠は「絞り込み」（前は「絞る（…）」「当たるアクターはいない」）。
# 出来事の名（登場・退場・交代など）と API の名は、どれにも当たらない。
# 見る形: 語の Mac（macOS は当てない）・/Users/<名>・札・上の前の字
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
if grep -nE '(^|[^A-Za-z])Mac([^A-Za-z]|$)|/Users/[A-Za-z]|札' $files >&2; then
  echo "NG: 公開する字に、1 台の機械やある人の手元を指す言い方か、前の呼び方「札」がある（上の行）。「castlist を動かす機械」「host」のように、どの機械でも通じる言い方にし、鍵と値の組は「ラベル」と書く" >&2
  exit 1
fi
# shellcheck disable=SC2086
if grep -nE '居るか|居た間|>いる<|>いない<|時間の帯|関係の網|相関図 ?[A-C]|時計のずれた行|絞る（|当たるアクターはいない' $files >&2; then
  echo "NG: 公開する字に、前の画面の字がある（上の行）。状態は「稼働中・停止」「稼働した期間」、図は「系図・タイムライン・相関図」（まとめは「図 A・B・C」）、ずれは「時刻ずれ」、絞りは「絞り込み」「条件に合うアクターはいません」と書く" >&2
  exit 1
fi
echo "ok: 公開する字に、1 台の機械やある人の手元を指す言い方と「札」と、前の画面の字は無い"
