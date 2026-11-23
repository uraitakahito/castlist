#!/usr/bin/env bash
# Rust の source に、行を書き替える・消す SQL が無いかを見る（台帳は足すだけ。DB の trigger も断る）。
# 見る形（大文字と小文字は問わない・改行をまたいでも見る）:
#   update <表> set ・ delete from ・ insert or replace ・ replace into ・ do update
# 1 つでも当たれば、file と行を出して exit 1。
# 使い方: ci/check-sql.sh [path…]（既定は src と crates/server/src。今の server と、作り直している新しい server）
set -euo pipefail

if [ "$#" -eq 0 ]; then
  here=$(cd "$(dirname "$0")" && pwd)
  root=$(dirname "$here")
  set -- "$root/src" "$root/crates/server/src"
fi

files=$(find "$@" -type f -name '*.rs' | LC_ALL=C sort)
if [ -z "$files" ]; then
  echo "NG: 見る Rust の file が無い: $*" >&2
  exit 1
fi

# perl で file を丸ごと読み、改行をまたいで探す（macOS と Linux のどちらでも同じに動く）。
# shellcheck disable=SC2086
if perl -0777 -ne '
  while (/\b(update\s+[\w."`]+\s+set|delete\s+from|insert\s+or\s+replace|replace\s+into|do\s+update)\b/gi) {
    my $line = 1 + (substr($_, 0, $-[0]) =~ tr/\n//);
    (my $hit = $1) =~ s/\s+/ /g;
    print "$ARGV:$line: $hit\n";
    $found = 1;
  }
  END { exit($found ? 0 : 1) }
' $files >&2; then
  echo "NG: 行を書き替える・消す SQL がある（台帳は足すだけ。docs/src/db.md）" >&2
  exit 1
fi
echo "書き替える・消す SQL は無い: $*"
