#!/usr/bin/env bash
# image が、空の image の中で起き、/healthz に答え、外の Host と browser からの書き込みを断り、
# 画面（一覧・タイムライン・相関図）と CSS と script が CSP の決まりどおりに出て、止める合図で自分から終わり、
# 起こし直しても答えるかを見る。
# 使い方: ci/smoke-image.sh <image>（repo の checkout の中で打つ。配る cytoscape を static/vendor/ の物と突き合わせる）
# container の名は smoke-castlist にする（castlist の設定の「PM だけ」の match（--name castlist など）に当てないため）。
set -euo pipefail

image=${1:?使い方: ci/smoke-image.sh <image>}
here=$(cd "$(dirname "$0")" && pwd)
name=smoke-castlist
url=http://127.0.0.1:18802
tmp=${RUNNER_TEMP:-/tmp}

up() {
  for _ in $(seq 1 40); do
    if curl -fsS "$url/healthz"; then return; fi
    sleep 0.5
  done
  return 1
}
trap 'docker logs "$name" || true; docker rm -f "$name" >/dev/null 2>&1 || true; docker volume rm "$name-data" >/dev/null 2>&1 || true' EXIT

docker run -d --name "$name" -p 127.0.0.1:18802:8080 -v "$name-data:/data" "$image"
up | tee "$tmp/healthz.json"
grep -q '"status":"ok"' "$tmp/healthz.json"
code() { curl -s -o /dev/null -w '%{http_code}' "$@"; }
test "$(code -H 'Host: evil.example' "$url/healthz")" = 403
test "$(code -X POST -H 'Origin: https://evil.example' --data '{}' "$url/v1/actors")" = 403
# 画面: 一覧に CSP が付き（src/lib.rs の CSP と同じ字）、inline の script と style が無く、castlist の CSS を読む。
csp="default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'"
curl -fsS -D "$tmp/head.txt" -o "$tmp/index.html" "$url/"
tr -d '\r' <"$tmp/head.txt" | grep -qixF "content-security-policy: $csp"
if grep -qiE '<script|<style|style=' "$tmp/index.html"; then
  echo "NG: 一覧に inline の script か style がある" >&2
  exit 1
fi
grep -qF '<link rel="stylesheet" href="/static/site.css' "$tmp/index.html"
# 系図（前の /graph/tree）は消した。開くと 404。
test "$(code "$url/graph/tree")" = 404
# タイムライン: 同じ CSP が付き、inline の script と style が無い（空の DB なので、条件に合うアクターがいないと書く）。
curl -fsS -D "$tmp/timeline-head.txt" -o "$tmp/timeline.html" "$url/graph/timeline"
tr -d '\r' <"$tmp/timeline-head.txt" | grep -qixF "content-security-policy: $csp"
if grep -qiE '<script|<style|style=' "$tmp/timeline.html"; then
  echo "NG: タイムラインに inline の script か style がある" >&2
  exit 1
fi
grep -qF '条件に合うアクターはいません' "$tmp/timeline.html"
curl -fsS -D "$tmp/css-head.txt" -o "$tmp/site.css" "$url/static/site.css"
tr -d '\r' <"$tmp/css-head.txt" | grep -qi '^content-type: text/css'
test -s "$tmp/site.css"
# 相関図: 同じ CSP が付き、inline の style が無く、script は castlist が配る 2 つ（cytoscape と network.js）を src で読むだけ。
curl -fsS -D "$tmp/network-head.txt" -o "$tmp/network.html" "$url/graph/network"
tr -d '\r' <"$tmp/network-head.txt" | grep -qixF "content-security-policy: $csp"
if grep -qiE '<style|style=' "$tmp/network.html"; then
  echo "NG: 相関図に inline の style がある" >&2
  exit 1
fi
scripts=$(grep -oiE '<script[^>]*>' "$tmp/network.html" | sed -E 's/\?v=[0-9a-f]+"/"/' | LC_ALL=C sort)
want_scripts='<script src="/static/network.js">
<script src="/static/vendor/cytoscape-3.35.0.min.js">'
if [ "$scripts" != "$want_scripts" ]; then
  echo "NG: 相関図の script が、castlist の配る 2 つでない: $scripts" >&2
  exit 1
fi
grep -qF 'data-src="/v1/graph"' "$tmp/network.html"
# 相関図の script（network.js）と cytoscape: JavaScript の型で、中身がある。cytoscape は static/vendor/ の物と同じ字。
curl -fsS -D "$tmp/network-js-head.txt" -o "$tmp/network.js" "$url/static/network.js"
tr -d '\r' <"$tmp/network-js-head.txt" | grep -qi '^content-type: application/javascript'
tr -d '\r' <"$tmp/network-js-head.txt" | grep -qix 'x-content-type-options: nosniff'
test -s "$tmp/network.js"
curl -fsS -D "$tmp/cytoscape-head.txt" -o "$tmp/cytoscape.js" "$url/static/vendor/cytoscape-3.35.0.min.js"
tr -d '\r' <"$tmp/cytoscape-head.txt" | grep -qi '^content-type: application/javascript'
tr -d '\r' <"$tmp/cytoscape-head.txt" | grep -qix 'x-content-type-options: nosniff'
cmp -s "$tmp/cytoscape.js" "$here/../static/vendor/cytoscape-3.35.0.min.js" || {
  echo "NG: 配る cytoscape が static/vendor/cytoscape-3.35.0.min.js と違う" >&2
  exit 1
}
# 止める合図（SIGTERM）で、自分から終わるか（PID 1 は受け口が無いと合図を無視する）。
docker stop "$name"
test "$(docker inspect -f '{{.State.ExitCode}}' "$name")" = 0
docker start "$name"
up
# 終わり方の印（volume の logs/）: 止める合図で止めた後の起動は「きれいに止まった」と出す。
docker logs "$name" 2>&1 | grep -q 'previous: stopped cleanly ('
# 合図なしで切った（SIGKILL）後の起動は「停止の印なし」と、最後に生きていた時刻を出す。
docker kill "$name"
docker start "$name"
up
docker logs "$name" 2>&1 | grep -q 'previous: no stop mark (.*; last alive 20'
# 残した記録は、止めずに読める。起こし直す前の問い合わせの行も、時刻つきで残っている。
curl -fsS "$url/v1/logs" | grep -q ' start version='
curl -fsS "$url/v1/logs" | grep -q 'uri=/healthz'
echo "image は動く: $image"
