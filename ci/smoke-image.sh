#!/usr/bin/env bash
# image が、空の image の中で起き、/healthz に答え、外の Host と browser からの書き込みを断り、
# 画面（一覧・タイムライン・相関図）と CSS と script が CSP の決まりどおりに出て、止める合図で自分から終わり、
# 起こし直しても答えるかを見る。
# 使い方: ci/smoke-image.sh <image> [v1|api]（repo の checkout の中で打つ。配る cytoscape を static/vendor/ の物と突き合わせる）
#   v1（既定）: 今の server の image（Containerfile）。API は /v1 の下
#   api: 新しい server の image（Containerfile.next。rc の release）。API は /api の下で、断る答えは restJson1 の形
#        （X-Amzn-Errortype）。登録して出来事を足し、読み返し、起こし直した後も同じ物が読めるかも見る。
#        終わりに切り替えの予行もする: 今の版（v0.4.0。digest で固める）の写しを、この image の migrate で新しい volume に移し、
#        新しい API で読む（docs/src/release.md の「rc を動かす時と切り替え」の 2 と 3）
# container の名は smoke-castlist にする（castlist の設定の「PM だけ」の match（--name castlist など）に当てないため）。
# 手元で流す時は、名と port を SMOKE_NAME・SMOKE_PORT で替えられる（既定は smoke-castlist と 18802）。
set -euo pipefail

usage='使い方: ci/smoke-image.sh <image> [v1|api]'
image=${1:?$usage}
mode=${2:-v1}
case "$mode" in
  v1) api=/v1 ;;
  api) api=/api ;;
  *)
    echo "$usage" >&2
    exit 2
    ;;
esac
here=$(cd "$(dirname "$0")" && pwd)
name=${SMOKE_NAME:-smoke-castlist}
port=${SMOKE_PORT:-18802}
url=http://127.0.0.1:$port
tmp=${RUNNER_TEMP:-/tmp}

up() {
  for _ in $(seq 1 40); do
    if curl -fsS "$url/healthz"; then return; fi
    sleep 0.5
  done
  return 1
}
# 後片付け: 切り替えの予行の container（今の版の $name-old・移した先の $name-v1）と volume と、写しの folder も消す。
cleanup() {
  for c in "$name" "$name-old" "$name-v1"; do
    if docker inspect "$c" >/dev/null 2>&1; then docker logs "$c" || true; fi
    docker rm -f "$c" >/dev/null 2>&1 || true
    docker volume rm "$c-data" >/dev/null 2>&1 || true
  done
  if [ -n "${backup_dir:-}" ]; then rm -rf "$backup_dir"; fi
}
trap cleanup EXIT

docker run -d --name "$name" -p "127.0.0.1:$port:8080" -v "$name-data:/data" "$image"
up | tee "$tmp/healthz.json"
grep -q '"status":"ok"' "$tmp/healthz.json"
code() { curl -s -o /dev/null -w '%{http_code}' "$@"; }
test "$(code -H 'Host: evil.example' "$url/healthz")" = 403
test "$(code -X POST -H 'Origin: https://evil.example' --data '{}' "$url$api/actors")" = 403
if [ "$mode" = api ]; then
  # 断る答えは、生成した層の誤りと同じ restJson1 の形（header に型の名・本文は JSON の message）。
  for header in 'Host: evil.example' 'Origin: https://evil.example'; do
    curl -s -D "$tmp/forbidden-head.txt" -o "$tmp/forbidden.json" -X POST -H "$header" --data '{}' "$url/api/actors"
    tr -d '\r' <"$tmp/forbidden-head.txt" | grep -qix 'x-amzn-errortype: forbidden'
    jq -e '.message | length > 0' "$tmp/forbidden.json" >/dev/null
  done
fi
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
grep -qF "data-src=\"$api/graph\"" "$tmp/network.html"
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
# 新しい server: 書いて読み返す（画面の確かめは空の DB を前にするので、その後に書く）。
if [ "$mode" = api ]; then
  # 登録（201）・出来事（201、同じ本文の送り直しは 200）・GetActor に出来事が付く。
  json='Content-Type: application/json'
  sent=2026-01-01T00:00:00Z
  register="{\"issuer\":\"smoke\",\"kind\":\"ai\",\"namePrefix\":\"se\",\"bindings\":[{\"type\":\"claude.session\",\"value\":\"smoke-1\"}],\"sentAt\":\"$sent\"}"
  test "$(curl -s -o "$tmp/actor.json" -w '%{http_code}' -X POST -H "$json" --data "$register" "$url/api/actors")" = 201
  jq -e '.actor.name == "se.1" and .created == true' "$tmp/actor.json" >/dev/null
  actor=$(jq -r '.actor.id' "$tmp/actor.json")
  event="{\"issuer\":\"smoke\",\"by\":\"claude.session:smoke-1\",\"type\":\"entered\",\"detail\":{\"source\":\"smoke\"},\"sentAt\":\"$sent\"}"
  test "$(code -X PUT -H "$json" --data "$event" "$url/api/events/smoke-e-1")" = 201
  test "$(code -X PUT -H "$json" --data "$event" "$url/api/events/smoke-e-1")" = 200
  read_back() {
    curl -fsS -o "$tmp/read.json" "$url/api/actors/$actor"
    jq -e '.name == "se.1" and .state.present == true and [.events[].id] == ["smoke-e-1"] and .events[0].detail.source == "smoke"' \
      "$tmp/read.json" >/dev/null
  }
  read_back
fi
# 止める合図（SIGTERM）で、自分から終わるか（PID 1 は受け口が無いと合図を無視する）。
docker stop "$name"
test "$(docker inspect -f '{{.State.ExitCode}}' "$name")" = 0
docker start "$name"
up
# 終わり方の印（volume の logs/）: 止める合図で止めた後の起動は「きれいに止まった」と出す。
# （grep -q に pipe でつながない: grep が先に終わると、書く側が SIGPIPE で落ち、pipefail で全体が落ちる）
logs=$(mktemp)
docker logs "$name" >"$logs" 2>&1
grep -q 'previous: stopped cleanly (' "$logs"
# 合図なしで切った（SIGKILL）後の起動は「停止の印なし」と、最後に生きていた時刻を出す。
docker kill "$name"
docker start "$name"
up
docker logs "$name" >"$logs" 2>&1
grep -q 'previous: no stop mark (.*; last alive 20' "$logs"
# 残した記録は、止めずに読める。起こし直す前の問い合わせの行も、時刻つきで残っている。
curl -fsS -o "$logs" "$url$api/logs"
grep -q ' start version=' "$logs"
grep -q 'uri=/healthz' "$logs"
rm -f "$logs"
# 新しい server: 起こし直した後も、volume の DB に書いた物が読める。
if [ "$mode" = api ]; then read_back; fi
# 切り替えの予行（新しい server だけ）: 今の版の image に今の API で書き、`GET /v1/backup` の写しを取り、この image の
# migrate で新しい volume に移し、その volume で起こして新しい API で読む。port は同じ物を使うので、先の container は消す。
if [ "$mode" = api ]; then
  docker rm -f "$name" >/dev/null
  # 今の版は v0.4.0（release の本文の digest で固める。tag を後から替えられても同じ物を取る）。
  old_image=ghcr.io/uraitakahito/castlist@sha256:9bc52784175e3ff5cb0ddfa2ff57363c256741d64e77b8a51da1f1b22d11efd3
  docker run -d --name "$name-old" -p "127.0.0.1:$port:8080" -v "$name-old-data:/data" "$old_image"
  up
  # 今の API（/v1・snake_case）で、アクター 2 つ（AI と人）と出来事 3 つを置く。
  sent=2026-01-01T00:00:00Z
  for actor in \
    "{\"issuer\":\"smoke\",\"kind\":\"ai\",\"name_prefix\":\"se\",\"bindings\":[{\"type\":\"claude.session\",\"value\":\"old-1\"}],\"sent_at\":\"$sent\"}" \
    "{\"issuer\":\"smoke\",\"kind\":\"human\",\"name_prefix\":\"pm\",\"bindings\":[{\"type\":\"tpm.person\",\"value\":\"pm\"}],\"sent_at\":\"$sent\"}"; do
    test "$(code -X POST -H "$json" --data "$actor" "$url/v1/actors")" = 201
  done
  for event in 'old-e-1 claude.session:old-1 entered' 'old-e-2 claude.session:old-1 exited' 'old-e-3 tpm.person:pm reported_in'; do
    read -r id by kind <<<"$event"
    body="{\"issuer\":\"smoke\",\"by\":\"$by\",\"type\":\"$kind\",\"detail\":{\"source\":\"smoke\"},\"sent_at\":\"$sent\"}"
    test "$(code -X PUT -H "$json" --data "$body" "$url/v1/events/$id")" = 201
  done
  backup_dir=$(mktemp -d "$tmp/backup.XXXXXX")
  curl -fsS -o "$backup_dir/castlist.db" "$url/v1/backup"
  test -s "$backup_dir/castlist.db"
  docker rm -f "$name-old" >/dev/null
  # release.md の 2 の行と同じ形（新しい volume を /data・写しの folder を /in に読むだけで付ける）。0 で終わらなければ落ちる。
  docker run --rm -v "$name-v1-data:/data" -v "$backup_dir:/in:ro" "$image" \
    migrate --from /in/castlist.db --to /data/castlist.db | tee "$tmp/migrate.txt"
  # 照らしの行はみな ok で、置いた数が移り、終わりの行は「移し終えた」。
  if sed '$d' "$tmp/migrate.txt" | grep -v ' ok$'; then
    echo "NG: migrate の照らしに ok でない行がある" >&2
    exit 1
  fi
  grep -qx 'actors 2 → 2 digest .* ok' "$tmp/migrate.txt"
  grep -qx 'events 3 → 3 digest .* ok' "$tmp/migrate.txt"
  test "$(tail -n 1 "$tmp/migrate.txt")" = '移し終えた: /in/castlist.db → /data/castlist.db'
  # 移した volume で起こし、新しい API で、置いた物が同じ数だけ読める。
  docker run -d --name "$name-v1" -p "127.0.0.1:$port:8080" -v "$name-v1-data:/data" "$image"
  up
  # ListActors は索引で引ける条件が要る（無ければ 400）ので、置いた付随する id で 1 人ずつ引く。
  curl -fsS -o "$tmp/migrated-se.json" "$url/api/actors?binding=claude.session:old-1"
  jq -e '[.actors[].name] == ["se.1"]' "$tmp/migrated-se.json" >/dev/null
  curl -fsS -o "$tmp/migrated-pm.json" "$url/api/actors?binding=tpm.person:pm"
  jq -e '[.actors[].name] == ["pm.1"]' "$tmp/migrated-pm.json" >/dev/null
  curl -fsS -o "$tmp/migrated-events.json" "$url/api/events"
  jq -e '[.events[] | [.id, .type, .actorName]] == [["old-e-1", "entered", "se.1"], ["old-e-2", "exited", "se.1"], ["old-e-3", "reported_in", "pm.1"]]' \
    "$tmp/migrated-events.json" >/dev/null
fi
echo "image は動く（${mode}）: ${image}"
