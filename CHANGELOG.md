# 変えたこと

版ごとに、API（[docs/src/api.md](docs/src/api.md) の `/v1/`）と DB の移行（`migrations/`）が変わったかを書く。release の workflow は、tag の版の節（`## <版>`）が無いと何も出さない（`ci/check-release-version.sh`）。

## 0.3.0

不正終了の原因を、後から調べられるようにした。系図の画面を消した。API（`/v1/`）は道を 1 つ足しただけで、移行の SQL は替わらない（0.2.0 と同じ volume で起き、0.2.0 へもそのまま戻せる）。起こし方・環境変数・image の中身も変わらない。

- 記録: 自分の終わり方と、時刻つきの記録を、volume の `/data/logs/` に残す（[動かす](https://uraitakahito.github.io/castlist/run.html) の「落ちた後に調べる」）。`container logs` は container を起こし直すと消えるが、こちらは残る。
  - 起きるたびに、前回の終わり方を 1 行で出す（`previous: …`）: きれいに止まった・止める合図は受けたが終わり切らなかった・panic（場所と文）・誤りで終わった（訳）・停止の印なし（最後に生きていた時刻）。
  - 記録の file（`server.log`）には、今までの行に加えて、問い合わせごとの 1 行（method・path・status・かかった時間）を書く。
  - **ディスクの上限**: 記録は 1 MiB × 4 file で回し、終わり方の印は 64 KiB まで。合わせて 4 MiB と 64 KiB を超えない。書けない時も castlist は動く（行を捨てる）。
- API: `GET /v1/logs` を足した。印と記録を古い順の字で返す（止めずに読める）。ほかの道は替わらない。
- **log の行の形が変わった**: `container logs` に出るどの行も、頭に時刻（UTC）と重さ（`INFO` など）が付く（`2026-01-02T03:04:05.000000Z  INFO castlist_server: castlist 0.3.0 listen=… db=…`）。行の中の文（`castlist <版> listen=… db=…`・`castlist stopped`）は変わらない。初めの行は、前回の終わり方になった。問い合わせごとの行は `container logs` には出ない（量を増やさないため）。誤りの行は、stderr でなく stdout に出る。止める合図を受けた時に `signal SIGTERM`（か `SIGINT`）の行を出す。
- 画面: 系図の画面（`/graph/tree`）を消した。前の URL は 404。一覧・タイムライン・相関図の頭の「系図へ」も無い（Product Manager の決めごと: 系図は使わない）。API と移行は替わらない。
- 画面: 期間の link の行の見出し「期間をすぐ選ぶ（…）」を「期間（今の条件のまま、期間だけを替えます）」に替えた。
- docs: 図の呼び名から記号（図 B・図 C）を外し、名だけ（タイムライン・相関図）にした。
- docs と誤りの字: DB の「写し」を「バックアップ」、「写しから戻す」を「バックアップからリストアする」にそろえた（`GET /v1/backup` の誤りの字も）。run.md の見出し「断るのは、機械の外からだけ」を「同じ network の container からは届く」にした。roster の `CASTLIST_VERSION` を上げる段を [release を出す](https://uraitakahito.github.io/castlist/release.html) に書いた。
- 中: 記録の 1 行を同期で書く `src/runlog.rs` だけ、塞ぐ呼び出しの断り（clippy）を外した（`panic = "abort"` で、落ちる直前の行を残すため）。
- 試験: 試験ごとに空の folder を作り、その中に DB を置く（記録が DB の隣の `logs/` にできるため）。本物の binary を止め方ごとに起こし直す試験（`tests/runlog.rs`）と、image の smoke での確かめを足した。

## 0.2.0

送り手の時刻の名を `at` から `sent_at` にした（PM の答え。`at` だけでは誰の時刻か分からないため）。API（`/v1/`）と移行の SQL が替わる。

- 移行: `0003_sent_at.sql` が、`actors`・`bindings`・`links`・`events`・`settings` の列 `at` を `sent_at` にする（列の名を替えるだけで、行は書き替えない）。**当てた後は 0.1.x では開けない。前の版へ戻すなら、上げる前に取った写しから戻す。**
- API: 書く 4 つの道（`POST /v1/actors`・`PUT /v1/events/{id}`・`PUT /v1/links/{id}`・`PUT /v1/settings/{id}`）の本文は `sent_at`。移る間は 0.1.x の名 `at` も受ける（両方を付ければ 422）。答えの行は、どれも `sent_at`（送り手の時刻の `at` の鍵は出ない。`?at=` の答えだけは、頭の `at` に問いの時点を返す）。`at` が読めない時の字は「sent_at（0.1.x の名 at） は RFC 3339 の時刻」。同じ id を前の名と新しい名で送り直しても、同じ物として 200。
- API: 問い合わせの `?at=`（`GET /v1/actors?binding=…&at=`・`GET /v1/actors/{id}/settings?at=`）の名は替えない。castlist の時計（`received_at`）での時点だと docs に書いた。
- docs: DB の「表」の章に ER 図（表と外のキー）を足し、移行した DB と合うかを試験が見る（PM の答え: 案 B。SVG を db.md に直に書き、JavaScript は使わない）。
- docs: DB の「列」の章（列の型・NULL・既定・決まり・中身の形）を足し、移行した DB と合うかを試験が見る。送り手の時刻を何の時刻か説明した。
- 送り手（tpm）は、castlist を上げた後で `sent_at` を送るように替える（それまでは `at` のままで通る）。

## 0.1.3

画面と試験だけを替えた。API（`/v1/`）と移行の SQL は替わらない（0.1.2 と同じ volume で起き、0.1.2 へもそのまま戻せる）。

- 画面: 「AI」の印を消し、人（Product Manager）にだけ「人」を付ける（一覧・プロフィール・系図・タイムライン・相関図の説明）。種類の絞りは、人を探す道として残す。
- 画面: 一覧・系図・タイムライン・相関図の絞り込みの枠の見出しの括弧書きを消した（訳は docs の画面の章に残す）。
- 画面: 期間の欄の読めない値の 400 の画面は、欄の見出しと直し方を書く（「「稼働した期間（から）」の日時を読めません。日時の欄で選ぶか、2026-11-22T13:00 の形で入れてください。」）。API の訳は替わらない。
- 画面: タイムラインの帯の字を、矢印の線（点線・実線）が通る所を避けて書く。空いた所に収まらなければ書かない（値は帯にマウスを重ねると出る）。
- 中: 使われていない `Db::actor_at`・`Db::actors_with_label` を消した（索引は移行にあるので残る）。
- 試験: CI の負荷の試験（job `load`）の一時の DB を memory の上（`/dev/shm`）に置く。CI の機械の disk の詰まりで、稀に 503 で落ちていた。基準は替えない。

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
