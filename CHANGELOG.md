# 変えたこと

版ごとに、API（[docs/src/api.md](docs/src/api.md) の `/v1/`）と DB の移行（`migrations/`）が変わったかを書く。release の workflow は、tag の版の節（`## <版>`）が無いと何も出さない（`ci/check-release-version.sh`）。

## 0.1.2

画面だけを替えた。API（`/v1/`）と移行の SQL は替わらない（0.1.1 と同じ volume で起き、0.1.1 へもそのまま戻せる）。

- 画面: 一覧の稼働した期間と、系図・タイムライン・相関図の登録した期間の欄を、browser の日時の欄（`datetime-local`）にした。入れた時刻は日本時間として RFC 3339（`+09:00`）にしてから探す。RFC 3339 の期間を書いた前の link も、そのまま効く。
- 画面: 期間の欄の下に「期間をすぐ選ぶ」の link（直近 1 時間・今日・昨日・直近 7 日。script なし）。ほかの条件はそのままで、期間を RFC 3339（`+09:00`）で URL に置くので、開いた URL を証拠に貼れる。
- 試験: 期間の欄とすぐ選ぶ link を、画面の試験（`tests/ui.rs`）と本物の browser の試験（`e2e/tests/dates.spec.ts`）で見る。

## 0.1.1

API と移行の SQL は替わらない（0.1.0 と同じ volume で起き、0.1.0 へもそのまま戻せる）。

- 画面の字を直した: 状態（稼働中・停止）・図の名（系図・タイムライン・相関図。まとめは図 A・B・C）・時刻ずれあり・ラベル・説明の文は です・ます（Product Manager が決めた字の一覧の通り）。
- docs: container の名を `castlist.box` にした（同じ network の container が DNS の domain `box` で名で引くため。置き場の pagetray は `http://castlist.box:8080` で聞く）。`Host` の検査が断るのは機械の外とほかの browser のページだけで、同じ network の container からは届くことを書いた。1 台の機械を指す言い方を、どの機械でも通じる言い方に直した。
- repo の設定: pm-only の守りの match に、新しい名 `castlist.box` を足した（前の名も残す）。
- CI: 「動かす」の container の命令が名 `castlist.box` を指し、pm-only の守りに当たるかを見る `ci/check-run-docs.sh`。公開する字に、1 台を指す言い方・前の画面の字が無いかを見る `ci/check-wording.sh`。

## 0.1.0

初めの版。API は `/v1/`。移行の SQL は `0001_init.sql`・`0002_search.sql`。

- アクターの台帳。Claude Code の session 1 つ（起動 1 回）と、人（Product Manager）を、それぞれ 1 つのアクターとし、本当の id（UUID v7）と名（`<接頭辞>.<n>`）を castlist が出す。Claude Code の session の id などは「付随する id」として残す。
  - API: 登録（`POST /v1/actors`。鍵の付随する id で 1 つにまとめる）・出来事（`PUT /v1/events/{id}`。登場・退場など 10 の種類）・線（`PUT /v1/links/{id}`。起動した・引き継いだ）・model と effort（`PUT /v1/settings/{id}`）・引き当て（`POST /v1/resolve`）・1 つのアクター（`GET /v1/actors/{id}`）。同じ id で同じ本文は 200、違う本文は 409。
- 足すだけの記録。どの表にも、書き替えと消しを断る trigger がある。Rust の source に書き替える・消す SQL が無いかを、`ci/check-sql.sh` が CI で見る。
- 外のキー（消す・替える時は断る）。起きる時に `PRAGMA foreign_key_check` を流し、崩れた行があれば起きない。
- model と effort の名の一覧（master の表 `models`・`efforts`）。知らない名は足す。`effort` は low・medium・high・xhigh・max を初めから持つ。
- 監査の検索: アクターを探す（`GET /v1/actors`）・誰が起動したか・誰を起動したか（`GET /v1/actors/{id}/lineage`）・時刻 T の model と effort（`GET /v1/actors/{id}/settings`）・model と effort の行（`GET /v1/settings`）・出来事（`GET /v1/events`）。
- 図の点と線（`GET /v1/graph`）と、動かしたまま取れる DB の写し（`GET /v1/backup`）。`GET /healthz` は版と castlist の今の時刻を返す。
- 画面: アクターの一覧（`/`）・プロフィール（`/actors/{id}`）・系図（図 A。`/graph/tree`）・タイムライン（図 B。`/graph/timeline`）・相関図（図 C。`/graph/network`）。相関図だけが script を使い、ほかは script なしで読める。
- 守り:
  - `Host` が `127.0.0.1`・`localhost` でない要求と、`Origin` の付いた書く要求を 403 で断る（castlist を動かす機械の browser で開いた、ほかのページからは書けない）。
  - 画面の応答に CSP（`default-src 'none'; script-src 'self'; style-src 'self'; …`）を付け、inline の script と style を使わない。
  - 相関図が使う cytoscape 3.35.0（MIT）は、CDN から読まず、`static/vendor/` に取り込んで binary に埋め込んで配る。npm の物と同じかを `ci/check-vendor.sh` が CI で見る。
- 動かす物: 空の image に binary 1 つ（`castlist-server`）と空の `/data`。止める合図（SIGTERM）で、走っている物を終えてから閉じる。並列の問い合わせを待たせない（塞ぐ呼び出しと、鍵を握ったままの await を clippy で断る）。負荷の試験（50 の客で 10 秒）を CI で流す。
