# castlist が外に出す文（日本語）。鍵は `<所>-<中身>`。英語は ../en/main.ftl（同じ鍵と、同じ値の名を持つ。試験が見る）。
# 日本語の字は、roster と担当が読む。字を替える時は、CHANGELOG に書く（roster は、いつも日本語を頼む）。

## 守りの層（src/guard.rs）

guard-host = Host は 127.0.0.1 か localhost だけを受ける
guard-origin = browser の中のページからは書けない
guard-timeout-seconds = 時間がかかりすぎた（{ $n } 秒）。少し待ってから送り直す
guard-timeout-ms = 時間がかかりすぎた（{ $n } ms）。少し待ってから送り直す
guard-too-large = 本文は { $mib } MiB まで
guard-unreadable-body = 本文を読めない

## どの操作にも共通の誤り（src/errors.rs）

internal = castlist の中で失敗した（記録を見る）
busy = 混んでいる。少し待ってから送り直す

## アクター（src/actor.rs・lineage.rs・actor_settings.rs・register.rs）

actor-not-found = アクター { $id } は無い
register-needs-binding = bindings に 1 つ以上（初めの物が鍵）
setting-time-range = time は 0001 年から 9999 年までの時刻

## 引数の時刻と絞り（src/lib.rs）

time-range = { $name } は 0000 年から 9999 年までの時刻
only-true = { $name } は true だけ（絞らないなら付けない）
sent-at-out-of-range = sentAt が扱える時刻の外

## アクターの検索（src/list_actors.rs）

actors-need-filter = 絞る条件を 1 つ以上付ける（label・binding・activeFrom・activeTo・active=true・model・effort・launchedBy・skewed のどれか）
actors-at-needs-binding = at は binding と一緒に付ける（稼働した期間で探すなら activeFrom・activeTo）
actors-token-other-order = nextToken は、同じ sort と order の答えの物を渡す

## 頁の印（src/token.rs）。検索の名は、言語で語順が違うので、検索ごとに文を分ける

token-broken = nextToken が読めない（前の答えの nextToken を、そのまま渡す）
token-other-actors = nextToken が読めない（アクターの検索の nextToken ではない）
token-other-events = nextToken が読めない（出来事の検索の nextToken ではない）
token-other-settings = nextToken が読めない（model と effort の検索の nextToken ではない）
token-other-usage = nextToken が読めない（使用量の検索の nextToken ではない）

## 出来事・線・model と effort・使用量を足す（src/event.rs・db.rs・usage.rs・list_usage.rs）

event-detail-object = detail は JSON の物（{"{"}…{"}"}）
ref-not-found = { $field } のアクター（{ $ref }）が、出し手 { $issuer } の鍵の付随する id に無い
conflict-event = 同じ id（{ $id }）の出来事が、違う中身で既にある
conflict-link = 同じ id（{ $id }）の線が、違う中身で既にある
conflict-setting = 同じ id（{ $id }）の model と effort が、違う中身で既にある
conflict-usage = 同じ id（{ $id }）のトークンの使用量が、違う中身で既にある
link-same-actor = from と to が同じアクター（{ $actor }）
link-already-launched = { $actor } は、もう別の線で起こされている
usage-span-too-long = from から to までは 31 日まで
usage-from-after-to = fromAt（{ $fromAt }）は、toAt（{ $toAt }）より後にしない
usage-not-same-hour = fromAt（{ $fromAt }）と toAt（{ $toAt }）は、同じ UTC の時（年月日と時が同じ）の内にする
usage-bucket-twice = buckets に、同じ bucket（{ $bucket }）が 2 つある
usage-bucket-sum = buckets の { $name } の和（{ $sum }）が、行の { $name }（{ $row }）と合わない

## 起こす binary（src/main.rs）。頼みが無いので、環境の言語（LC_ALL → LC_MESSAGES → LANG）で出す

usage = 使い方: castlist-server（環境変数 CASTLIST_DB・CASTLIST_LISTEN で起きる）
