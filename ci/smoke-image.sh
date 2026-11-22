#!/usr/bin/env bash
# image が、空の image の中で起き、/healthz に答え、外の Host と browser からの書き込みを断り、
# 画面（一覧・系図・時間の帯）と CSS が CSP の決まりどおりに出て、止める合図で自分から終わり、起こし直しても答えるかを見る。
# 使い方: ci/smoke-image.sh <image>
# container の名は smoke-castlist にする（castlist の設定の「PM だけ」の match（--name castlist など）に当てないため）。
set -euo pipefail

image=${1:?使い方: ci/smoke-image.sh <image>}
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
# 系図: 同じ CSP が付き、inline の script と style が無い（空の DB なので、当たる者がいないと書く）。
curl -fsS -D "$tmp/tree-head.txt" -o "$tmp/tree.html" "$url/graph/tree"
tr -d '\r' <"$tmp/tree-head.txt" | grep -qixF "content-security-policy: $csp"
if grep -qiE '<script|<style|style=' "$tmp/tree.html"; then
  echo "NG: 系図に inline の script か style がある" >&2
  exit 1
fi
grep -qF '当たるアクターはいない' "$tmp/tree.html"
# 時間の帯: 同じ CSP が付き、inline の script と style が無い（空の DB なので、当たる者がいないと書く）。
curl -fsS -D "$tmp/timeline-head.txt" -o "$tmp/timeline.html" "$url/graph/timeline"
tr -d '\r' <"$tmp/timeline-head.txt" | grep -qixF "content-security-policy: $csp"
if grep -qiE '<script|<style|style=' "$tmp/timeline.html"; then
  echo "NG: 時間の帯に inline の script か style がある" >&2
  exit 1
fi
grep -qF '当たるアクターはいない' "$tmp/timeline.html"
curl -fsS -D "$tmp/css-head.txt" -o "$tmp/site.css" "$url/static/site.css"
tr -d '\r' <"$tmp/css-head.txt" | grep -qi '^content-type: text/css'
test -s "$tmp/site.css"
# 止める合図（SIGTERM）で、自分から終わるか（PID 1 は受け口が無いと合図を無視する）。
docker stop "$name"
test "$(docker inspect -f '{{.State.ExitCode}}' "$name")" = 0
docker start "$name"
up
echo "image は動く: $image"
