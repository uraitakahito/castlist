#!/usr/bin/env bash
# release の tag が、出す版と合うかを見る。合わなければ exit 1（release の workflow は何も作らない）。
#   1. tag が `v<版>` で、<版> が Cargo.toml の初めの `version = "…"` の行の値と同じ
#   2. CHANGELOG.md に、その版の節（`## <版>` の行）がある
# 試験は ci/test-check-release-version.sh。
# 使い方: ci/check-release-version.sh <tag> [repo の folder（既定は repo の一番上）]
set -euo pipefail

tag=${1-}
here=$(cd "$(dirname "$0")" && pwd)
root=${2:-$(dirname "$here")}

ng() {
  echo "NG: $*" >&2
  exit 1
}

[ -f "$root/Cargo.toml" ] || ng "Cargo.toml が無い: $root"
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml" | head -1)
[ -n "$version" ] || ng "Cargo.toml に \`version = \"…\"\` の行が無い"
[ "$tag" = "v$version" ] || ng "tag「${tag}」が Cargo.toml の版 ${version} と合わない（tag は v${version}）"
[ -f "$root/CHANGELOG.md" ] || ng "CHANGELOG.md が無い"
grep -qxF "## $version" "$root/CHANGELOG.md" || ng "CHANGELOG.md に ${version} の節（## ${version}）が無い"
echo "tag ${tag} は Cargo.toml の版と CHANGELOG.md の節に合う"
