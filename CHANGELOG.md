# 変えたこと

版ごとに、API（[docs/src/api.md](docs/src/api.md) の `/api/`。定義は `model/`）と DB の移行（`crates/server/migrations/`）が変わったかを書く。0.4.x までの節の API は `/v1/`、移行は repo の頭の `migrations/` の物（もう無い）。release の workflow は、tag の版の節（`## <版>`）が無いと何も出さない（`ci/check-release-version.sh`）。

## 2.1.1

字だけの版（t-5dhg）。API・DB・動きは 2.1.0 と同じ。

- 呼び方: データの名「トークンの数と費え」を「トークンの使用量」に替えた（Product Manager の答え。callboard の画面の名「費え」も「使用量」に替える）。定義（`model/`）のコメントと、作り直した `crates/castlist-sdk/`、本の api・db・development、README、コードのコメント、409 の誤りの文の字（`同じ id（…）のトークンの使用量が、違う中身で既にある`）。定義は wire の形が同じ（`ci/gen.sh --diff` が通る）。`common.smithy` は変えていない。
- 出した移行の file（`0002_usage.sql`）のコメントは、替えない決まりなので前の字のまま。
- DB の移行の SQL は足していない（2.1.0 の volume でそのまま起き、2.1.0 の image へそのまま戻せる）。
- `ci/check-wording.sh` が「費え」も止める（README・本。前の節の記録が残る CHANGELOG.md は見ない）。

## 2.1.0

アクターごとの「トークンの数と費え」を残す版（t-j4pp）。足しただけで、前からある操作と欄は変えていない（定義は v2.0.0 から壊す形に変わっていない: `ci/gen.sh --diff`。`common.smithy` は変えていない）。

- API: 操作 `PutUsage`（`PUT /api/usage/{usageId}`）を足した。1 行は、アクター 1 人の、時計の 1 時間（UTC の時）の内の、同じ種類（model・手伝いの種類・speed・service tier）の回のまとめ（回の数・トークンの数・web の検索と取得の回数・額・いちばん長い文脈・文脈の長さの段ごとの数）。同じ id で同じ本文は 200、違う本文は 409。区間が同じ UTC の時の内に無い・同じ段が 2 つ・段の和が行と合わない、は 400。
- API: 操作 `ListUsage`（`GET /api/usage`）を足した。`from`・`to`（必ず要る。`from <= toAt < to`。31 日まで）・`label`・`actorId` で絞り、行と、その頁に出てきたアクター（id・名・ラベル）を返す。集計はしない（まとめるのは呼ぶ側）。
- **読む側の決まり**: 同じアクター・同じ時・同じ種類の行は 2 つ以上在りうる（送り手が、増えた分を新しい行で送る）。数は足し、`maxContext` は大きい方を取る（[API](docs/src/api.md)）。
- **DB の移行の SQL を足した**（`0002_usage.sql`。表 `usage` を 1 つ足すだけで、今の表には触らない）。**この版を当てた volume は、前の版（2.0.0）の image では開けない**（戻すなら、上げる前のバックアップから: [動かす](docs/src/run.md) の「版を上げる」）。
- 圧縮の出来事（`compacted`）の `detail` に、送り手が圧縮の前後の長さを載せるようになる（roster の側の変更。castlist は、今までどおり `detail` を好きな JSON の物として受ける）。
- smoke: 数の行を書いて読み返すことを足した。

## 2.0.0

**画面を外した版（壊す変わり）。**アクターの一覧・プロフィール・タイムライン・相関図は、castlist の API を通して [callboard](https://github.com/uraitakahito/callboard)（0.2.0 から）が出す（AI-32）。API と DB は変えていない。

- **前の画面の URL は、どれも 404 になる**（閉じる。callboard へは送らない。Product Manager の決め）: `GET /`（一覧）・`GET /actors/{id}`（プロフィール）・`GET /graph/timeline`・`GET /graph/network`・`GET /static/…`。同じ画面は、callboard の `http://127.0.0.1:8803/actors/`・`/actors/{id}`・`/graph/timeline`・`/graph/network` にある（プロフィールと図の path は同じで、番地だけが替わる。一覧は `/` から `/actors/` に替わる）。
- **上げる前に**: callboard を 0.2.0 からにして、castlist の 4 つの画面が callboard で開くことを見る。castlist の画面の URL を指している所（roster の `castlist` の命令が出すプロフィールの URL・pagetray の `PAGETRAY_CASTLIST_LINK_URL`）を、callboard の番地に替える。
- API: 操作と欄は 1.1.0 と同じ（定義は v1.1.0 から壊す形に変わっていない: `ci/gen.sh --diff`）。一覧の行の `url`（`/actors/{id}`）は、そのまま（callboard の画面の path。castlist の番地に付けても開かない）。定義は、この欄の注を直しただけで、wire の形は替わらない。
- DB の移行の SQL は足していない（1.1.0 の volume でそのまま起きる。戻す時も、1.1.0 の image で同じ volume を開ける）。
- 答えの頭の印: HTML の答えが無くなったので、Content-Security-Policy は付けない。`X-Content-Type-Options: nosniff` と `Referrer-Policy: no-referrer` は、今までどおり、どの答えにも付く。
- 無くした物: 画面の code（`templates/`・`static/`・`crates/server/src/ui.rs`・`ui/params.rs`・並べる計算の `timeline.rs`）・画面の試験（`crates/server/tests/ui.rs`・`e2e/`）・取り込んだ cytoscape と、その突き合わせ（`ci/check-vendor.sh`）・画面の本（`docs/src/ui.md`。callboard の本の「画面」に移った）。image の binary から、template・CSS・script が消える。
- CI: job `e2e` を無くした（ruleset で必須にしている時は、外す）。`ci/check-wording.sh` の、画面の字の決まりは callboard に移した。smoke は、前の画面の道が 404 なことを見る。

## 1.1.0

画面を別の server（callboard）に移すために、API に足した版（AI-32）。足しただけで、前からある操作と欄は変えていない（定義は v1.0.0 から壊す形に変わっていない: `ci/gen.sh --diff`）。DB の移行の SQL は足していない（1.0.0 の volume でそのまま起きる。戻す時も、1.0.0 の image で同じ volume を開ける）。

- API: 一覧の行（`ListActors` の `actors[]` と `GetLineage` の `nodes[]`）に、欄を 2 つ足した。`presence`（稼働を替える出来事の最新の種類。`GetActor` の `state.presence` と同じ）と `skewed`（時刻ずれの行を持つか。絞りの `skewed` と同じ意味）。どちらも、必ずあるとは決めていない欄（前の版の答えには無い）。
- API: 操作 `GetTimeline`（`GET /api/timeline`）を足した。タイムラインを描く元の値（`GetGraph` と同じ組の点と線に、その点の出来事と、model と effort の行の全部を足す）。並べる計算は、呼ぶ側がする。
- 画面は変えていない（一覧・プロフィール・タイムライン・相関図は、今までどおり castlist が出す）。
- 定義の注: `ListActorSettings` の注から、0.4.x の server を指す字（「今の server と同じ」）を消した（wire の形は替わらない。定義の file の digest は替わる）。
- 1.0.0 の節の直し: CI と release の smoke の打ち方は `ci/smoke-image.sh <image>`（引数は image だけ）。1.0.0 の節は `ci/smoke-image.sh <image> api` と書いているが、後ろの引数は、0.4.x の server の code を消した時に無くした。
- smoke: `GET /api/timeline` が、書いた出来事を返すかを足した。

## 1.0.0

AI-36 の作り直しの新しい server（`crates/server`）を、ふつうの release にした版。この版から、image（`Containerfile`）に入るのは新しい server で、今の server（0.4.x）の image は出さない。API と DB の移行の SQL は 1.0.0-rc.1 と同じ（0.4.x からは、どちらも替わる。1.0.0-rc.1 の節）。

- **切り替え（0.4.x から上げる時）: volume は新しい物（`castlist-data-v1`）にし、先に migrate で移す。** 今の volume（`castlist-data`）では起こさない（新しい server は今の server の DB を開かずに止める）。今の server の `GET /v1/backup` で写しを取り、この版の image に `migrate --from <写し> --to /data/castlist.db` を付けて新しい volume に移し、移し終えてから新しい volume で起こす。今の volume は、戻す時のために残す。手順は [release を出す](https://uraitakahito.github.io/castlist/release.html) の「切り替え」、起こす行は [動かす](https://uraitakahito.github.io/castlist/run.html)。
- API: 1.0.0-rc.1 と同じ（`/api/…`。形は [新しい API の形](https://uraitakahito.github.io/castlist/api.html)（1.0.0 から API の仕様に入れた））。定義（`model/`）は、例の引数の値の `:` を `%3A` で書くようにしただけで、wire の形は替わらない。`HEAD` の答えの `Content-Length` を、`GET` と同じにした。
- image と release: `Containerfile` が新しい server を作る（binary は今と同じ `/castlist-server`。環境変数・port も同じ）。CI の job `image` と release の確かめは、新しい API の形の smoke（`ci/smoke-image.sh <image> api`）を流す。release の workflow の rc の分け（`-rc.` の tag だけ `Containerfile.next`）は無くした（`-rc.` の tag を pre-release にする所は残す）。`Containerfile.next` と今の server の code は段 9 で消す。
- smoke の予行（1.0.0-rc.1 の後に足した）: image の smoke の終わりに、今の版（v0.4.0。digest で固める）に今の API でアクター・ラベル・線・model と effort（登録で頼んだ物と、見た物）・出来事を置いて写しを取り、この image の migrate で新しい volume に移し、照らしの行がみな ok で、新しい API で同じ物が読めるかを見る。予行の途中の container は消さずに止め、落ちた時に log を出す。
- docs: [動かす](https://uraitakahito.github.io/castlist/run.html) の起こす行を、切り替えの後の形（`v1.0.0`・volume `castlist-data-v1`・`/api/backup`・`/api/logs`）にした。[release を出す](https://uraitakahito.github.io/castlist/release.html) の rc の節を切り替えの手順にし、今の server（0.4.x）を直して出す道を消した（1.0.0 の後は出さない。戻す時は v0.4.0 の image と今の volume で起こす）。

## 1.0.0-rc.1

AI-36 の作り直しの初めの候補（pre-release）。この版の image は、新しい server（`crates/server`。Smithy の定義 `model/` から作った骨の上に処理を入れた物）を `Containerfile.next` で作った物で、今の server（0.4.0）とは別の物。動いている castlist（`castlist.box`）は替えない。切り替え（AI-36 の段 8）までは、試しと予行にだけ使う。

- **API がみな替わった**（今の server との互換は無い。Product Manager の決め: AI-36）: 道は `/api/…`（`/healthz` だけは外）、protocol は restJson1、欄の名は camelCase、誤りは定義に書いた型（`X-Amzn-Errortype` と `{"message":…}`）、ページ送りは `nextToken`・`maxResults`、時刻は UTC だけ。形は [新しい API の形](https://uraitakahito.github.io/castlist/api.html)（1.0.0 から API の仕様に入れた）。
- **DB の移行の SQL が替わった**: 新しい server は、新しい migrations（`crates/server/migrations/`）で新しい DB を作る。**今の volume（`castlist-data`）では起こさない**（新しい server は今の server の DB を開かずに止める）。今のデータは、今の server の `GET /v1/backup` の写しから `castlist-next migrate --from … --to …` で新しい volume に移す（id・順・時刻の字・本文の写しを守り、数と中身の digest と外のキーを照らす）。手順は [release を出す](https://uraitakahito.github.io/castlist/release.html) の「rc を動かす時と切り替え」。
- 画面（一覧・プロフィール・タイムライン・相関図）は今と同じ。相関図は `/api/graph` を読む。
- image の中身・環境変数・port は今と同じ（binary は `/castlist-server`）。起こし方は、volume のほかは今と同じ（volume は新しい物にする）。

## 0.4.0

落ちた session の終わりを、送り手（tpm）が推して後から送る「推しの幕引き」を描き分ける（t-3ec4。PM の答え: 案 B）。API（`/v1/`）は答えに欄を 1 つ足しただけで、移行の SQL は替わらない（0.3.0 と同じ volume で起き、0.3.0 へもそのまま戻せる）。起こし方・環境変数・image の中身も変わらない。

- API: `GET /v1/actors/{id}` と `GET /v1/graph` の `state`、`GET /v1/actors` の行に `inferred` を足した。今の状態を決めた出来事が、推しの幕引き（`closed` で `detail.inferred` が JSON の true）なら `true`（今の帯を終わらせた出来事で決める。推しの幕引きの後に遅れて来た退場では `true` のまま）。出来事の受け取り方は替わらない（`detail` は前から好きな JSON の物を受ける）。
- 画面: タイムラインは、推しの幕引きで終わる帯の、最後の便りから終わりまでを点線の縁と薄い塗りで描き、マウスを重ねると推した訳（居場所が消えた・居場所に別の session が入った）を出す。凡例に「点線の帯」を足した。一覧とプロフィールの状態は「停止（推し）」、相関図の点の説明も「停止（推し）」。
- docs: 推しの幕引きの `detail` の形（`inferred`・`reason`）を API の章に書いた。概要の page から、API と画面の中身を並べ「動かす」「release を出す」を指す文を消した（#33。サイドメニューが同じ道案内をする）。
- 試験: 推しの帯の並べ方（単体）・`state.inferred`（API）・画面の字（ui）・相関図の説明（e2e）を足した。e2e の見本の other.1 は、推しの幕引きで終わる。

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
