# 新しい API の形（作り直しの間の設計）

AI-36 の作り直しで、castlist の API を Smithy の定義（`model/`）から作る。この章は、新しい server（`crates/server`）が従う形を先に決めておく物。切り替え（AI-36 の段 8）の後は、[API](api.md) の章をこの形に書き直し、この章は消す。

今の API との互換は守らない（Product Manager の決め: AI-36）。守るのは、データ（台帳の行と id）と、そこから決まる振る舞い（足すだけ・同じ id は 2 度足さない・`received_at` は戻らない・「最新」は seq で決める など）。

## 共通の決まり（pagetray とも同じ）

| 決まり | 形 |
|---|---|
| 道の頭 | API は `/api/` の下（画面の `/actors/{id}` などと重ならないように）。`/healthz` だけは外 |
| protocol | `aws.protocols#restJson1`。誤りの種類は header `X-Amzn-Errortype` と本文の `message` |
| 欄の名 | camelCase（`sentAt`・`receivedAt`・`clockSkew`・`asOf`・`actorId`・`namePrefix`） |
| 時刻 | RFC 3339 の UTC（末尾 `Z`。`ai.common#DateTime`）。Smithy の `date-time` は時差の付いた時刻（`+09:00`）を受けず、400 `SerializationException` で断る。送り手の時刻（`sentAt`）は UTC に直して残す（今の server は字のまま残していた）。答えの `receivedAt` は UTC・ミリ秒まで |
| 誤り | 定義に書いた型だけ: 400 `ValidationException`（欄の path と訳）・404 `NotFound`・409 `Conflict`・500 `InternalError`（server の中の失敗。訳は記録に書き、答えは決まった字。送り直しても直らないので `@retryable` は付けない）・503 `Unavailable`（混んでいる・時間切れ。`@retryable`）。知らない道は生成した層が 404 `UnknownOperationException`（道は在るが method の違う物は 405）。守りの層が断る 403 `Forbidden`・413 `PayloadTooLarge`・503 `Unavailable`（時間切れ）は、service の `errors` に置き、どの操作の誤りにも入る（下の「守りの層」） |
| 書いて足す操作 | 呼ぶ側が id を決める物は `put`（`PUT /api/events/{eventId}` など。`@idempotent`。同じ id・同じ本文は 200、新しく足せば 201、同じ id で本文が違えば 409）。状態の数は `@httpResponseCode` で返し、本文は足した行（同じなら前の行）を GetActor の中の物と同じ形で返す。同じ id かは、本文のアクターを引くより先に見る（引けないアクターでも、同じ id なら 409） |
| 引けないアクター | 本文の `by`・`from`・`to` が出し手の中の鍵で引けない時と、線の `from` と `to` が同じアクターの時は、400 `ValidationException`（訳は `message`。今の server は 422） |
| ページ送り | `@paginated`。`maxResults`（1〜1000。既定 100）と、前の答えの `nextToken` をそのまま渡す |
| 探す答え | どれも `asOf`（castlist の今の時刻）を付ける |
| 決まった字の欄 | `kind` などの決まった字は、enum でなく `@pattern` の字で書く（codegen-server 0.1.40 の作る protocol の試験が、要る enum を空の字から作って panic するため。wire の形は同じ）|
| 知らない欄 | restJson1 は知らない欄を読み飛ばす（今の 422 はやめる）。id を外から紛れ込ませる道は、定義に欄が無いので無い |
| 問い合わせの引数 | 名は camelCase（`activeFrom`・`launchedBy`・`maxResults`・`nextToken`）。値の形（時刻・`鍵:値`・id・決まった字・数の範囲）は生成した層が 400 で断る。restJson1 は知らない引数を読み飛ばし、重ねられない引数を 2 度付ければ初めの物を使う（今の server はどちらも 400。生成した層の前では断れない）。重ねられるのは list の引数（`label`）だけ。時刻の引数は DB に残す字（UTC・ミリ秒まで・24 字。ミリ秒より下は切る）に直して比べる |

## 操作

| 操作 | 道 | 今の道 |
|---|---|---|
| `GetHealth` | `GET /healthz` | `GET /healthz` |
| `RegisterActor` | `POST /api/actors` | `POST /v1/actors` |
| `GetActor` | `GET /api/actors/{actorId}` | `GET /v1/actors/{id}` |
| `ListActors` | `GET /api/actors` | `GET /v1/actors` |
| `GetLineage` | `GET /api/actors/{actorId}/lineage` | `GET /v1/actors/{id}/lineage` |
| `ListActorSettings` | `GET /api/actors/{actorId}/settings` | `GET /v1/actors/{id}/settings`（`at` 無し） |
| `GetActorSettingAt` | `GET /api/actors/{actorId}/settings/at?time=…` | `GET /v1/actors/{id}/settings?at=…`（引数で答えの形が変わっていた） |
| `PutEvent` | `PUT /api/events/{eventId}` | `PUT /v1/events/{id}` |
| `ListEvents` | `GET /api/events` | `GET /v1/events` |
| `PutLink` | `PUT /api/links/{linkId}` | `PUT /v1/links/{id}` |
| `PutSetting` | `PUT /api/settings/{settingId}` | `PUT /v1/settings/{id}` |
| `ListSettings` | `GET /api/settings` | `GET /v1/settings` |
| `ResolveActors` | `POST /api/resolve` | `POST /v1/resolve` |
| `GetGraph` | `GET /api/graph` | `GET /v1/graph` |

手の道（Smithy の外）: 画面（`/`・`/actors/{id}`・`/graph/…`・`/static/…`）・`GET /api/backup`（SQLite の file）・`GET /api/logs`（字）。

- `GET /api/backup`（`crates/server/src/hand.rs`）: 今の server の `GET /v1/backup` と同じ。DB の隣の一時の file（`<DB>.backup-<pid>-<番号>`）に `VACUUM INTO` で揃ったバックアップを作り、開いてから名を消し、`application/vnd.sqlite3`（`Content-Disposition: attachment; filename="castlist.db"`）で読みながら送る。DB 全体を読むので、重い読みの接続で作る（今の server は読む接続。新しい server では、軽い読みの接続を取らないようにした）。誤りは restJson1 の形の 503 `Unavailable`・500 `InternalError`。
- `GET /api/logs`: 今の server の `GET /v1/logs` と同じ字（`text/plain; charset=utf-8`。印と、古い順の記録）。記録の置き場を付けた時（binary）だけ在り、無い時（Router を直に呼ぶ試験）は生成した層の 404。

## 探す操作の決まり

- `ListActors`（`GET /api/actors`）: 条件はどれも AND で、今の server の SQL のまま。索引で引ける条件（`label`・`binding`・`activeFrom`・`activeTo`・`active=true`・`model`・`effort`・`launchedBy`・`skewed`）が 1 つも無ければ 400（`kind` と `active=false` は数えない）。`at` は `binding` と一緒にだけ付ける（無ければ 400）。`skewed` は true だけを受け、false は 400（「時刻ずれの無いアクター」と読まれないように。今の server と同じ）。
- 頁の印（`nextToken`）は、検索の種類・`sort`・`order`・最後の行の並べる値と seq を 16 進にした字。崩れた物・ほかの検索の物・`sort` か `order` の違う物は 400。ほかの条件は結ばない（今の server と同じ）。辿る間に足されたアクターでは穴も重なりも出ないが、並べる値（初めの登場・最後の退場）は出来事で動くので、もう出したアクターが辿る間に退場すると出直すことがある（今の server と同じ。動かない値で分けるなら並べ方の意味が変わるので、別の課題）。
- `launchedBy` と道のアクターの id は `ActorId` の形（UUID）だけを受ける。形の違う id は 400（今の server は、線の上下では 404、`launched_by` では空の答え）。
- `GetLineage`（`GET /api/actors/{actorId}/lineage`）: 点を引く所も、線を辿る所と同じ 1 つの読む transaction で読む（今の server は 2 つに分けていた）。答えの欄は `asOf`・`actorId`・`nodes`（一覧の行 `ActorRow`）・`edges`（GetActor の線と同じ形）。
- 一覧の行（`ActorRow`）は、今の server の一覧の欄を camelCase にした物（`firstEnteredAt`・`lastExitedAt`・`lastExitType`・`present`・`inferred`・`model`・`effort`・`url`）。無い値は欄ごと無い（今の server は null）。
- `ListActorSettings`（`GET /api/actors/{actorId}/settings`）: そのアクターの model と effort の全部を受けた順に返し、頁は分けない（1 つのアクターの行は少ない。今の server と同じ）。行は GetActor の model と effort と同じ形（`Setting`。アクターの名は付けない）。
- `GetActorSettingAt`（`GET /api/actors/{actorId}/settings/at?time=…`）: 今の server は `at` の有る無しで答えの形が変わったので、道を分けた。`time` は要る（無ければ 400）。受けた時刻が `time` まで（含む）の行の最新（seq で）1 つを `setting` に、比べた形（UTC・ミリ秒まで）に直した `time` を `at` に返す。行が無ければ `setting` は欄ごと無い（今の server は null）。`at` は 0001 年からしか書けないので、0000 年の `time` は 400。
- `ListSettings`（`GET /api/settings`）と `ListEvents`（`GET /api/events`）: 条件はどれも AND で、無くてもよい（今の server と同じ）。並びは受けた順（seq）。`mismatch` と `skewed` は true だけを受け、false は 400（`ListActors` の `skewed` と同じ訳）。`mismatch` は、見た行のうち、同じアクターのそれより前（seq で）の最新の頼んだ行と model か effort が違う物（前に頼んだ行が無ければ入らない）。`type` は重ねれば OR で、10 種の外は生成した層が 400。`actor` は `ActorId` の形だけ（今の server は空でない 256 字の何でも）。`reason`・`source` は `detail` の同じ名の欄と等しい物（空でない 256 字まで）。
- `ListSettings` と `ListEvents` の頁の印は、検索の種類と最後の行の seq を 16 進にした字。崩れた物・ほかの検索の物（`ListActors` の物も）は 400。ほかの条件は結ばない（今の server と同じ）。
- 探した行は、出来事と model と effort の欄（mixin の `EventFields`・`SettingFields`）に `actorName` を足した形（`EventRow`・`SettingRow`）。今の server の model と effort の行にあった `model_id`・`effort_id`（中の番号）は出さない。
- 読む操作は、どれも 1 つの読む transaction の中で読む（今の server の `ListSettings`・`ListEvents` は transaction を張らずに 1 つの SQL で読んでいた）。`asOf` は読み始める前に取る。

## 図と画面

- `GetGraph`（`GET /api/graph?label=鍵:値&from=…&to=…`）: 今の server の `GET /v1/graph` の決まりのまま。ラベルと、登録を受けた時刻の窓（両端を含む）でアクターの組を絞り、組に触れる線の全部と、その線の向こうの端のアクター（一歩だけ）も入れる。点も線も受けた順（seq）。重い読みの接続（2 本）の 1 つの読む transaction の中で読み、軽い読みの接続は使わない。点（`GraphNode`）の `state` は GetActor の `state` と同じ形（今の server は、無ければ null の欄）、`currentSettings` は GetActor の物と同じ形（`id`・`clockSkew` も付く。今の server の物には無かった）。引数はどれも 1 つだけ（重ねれば初めの物を使う。今の server は 400）。
- 画面（`/`・`/actors/{id}`・`/graph/timeline`・`/graph/network`・`/static/…`）は、同じ axum の Router に手の道として並べる（`crates/server/src/ui.rs`）。URL の引数（snake_case の `active_from` など）・画面の字・誤りの訳は今の server のまま（画面の引数は、生成した層でなく画面が自分で読む）。DB は新しい DB の所で読み、タイムラインも重い読みの接続だけを使う。
- 違うのは、相関図の `data-src` が `GET /api/graph` を指し、期間を UTC・ミリ秒までの字にして渡すこと（新しい API は時差の付いた時刻を受けない）。`static/network.js` は、今の server の欄の名（`as_of`・`current_settings`）と新しい欄の名（`asOf`・`currentSettings`）のどちらも読む。
- `templates/` と `static/` は今の server と共に使う（`crates/server/askama.toml` が `templates/` を指し、配る file は repo の `static/` を埋め込む）。段 9 で今の crate を消しても、動かさなくてよい。タイムラインの並べ方（`crates/server/src/timeline.rs`）は今の `src/timeline.rs` の写しで、段 9 まで 2 つを同じ中身に保つ。
- 試しに起こす binary は `castlist-next`（`crates/server/src/main.rs`）。今の `castlist-server` と同じ振る舞い: 環境変数 `CASTLIST_DB`（既定 `/data/castlist.db`）と `CASTLIST_LISTEN`（既定 `0.0.0.0:8080`）・記録の置き場は DB の隣の `logs/`（大きさで回す記録の file・終わり方の印・生きている印。[動かす](run.md) の「落ちた後に調べる」）・起きて初めに前の回の終わり方の行（`previous: …`）と起きた行（`castlist <版> listen=… db=…`）・SIGTERM と SIGINT で新しい問い合わせを断り、走っている物を終えてから閉じる。記録の書き手（`crates/server/src/runlog.rs`）は今の `src/runlog.rs` の写しで、段 9 まで 2 つを同じ中身に保つ（違うのは読む道の名と、hook の試験が見る file の場所の字だけ）。画面の browser の試験（`e2e/`）は、`CASTLIST_E2E_SERVER=../target/debug/castlist-next` で新しい server にも向く（見本を置く道と欄の名は `e2e/tests/wire.ts` が合わせる）。

## 守りの層

生成した道と手の道の前に、外から順に掛ける（`crates/server/src/guard.rs`）。断る答えは、生成した層の誤りと同じ restJson1 の形（header `X-Amzn-Errortype` に定義の型の名・本文は JSON の `{"message":…}`）にする（今の server は `{"error":…}`）。型は `ai.common` の `Forbidden`（403）・`PayloadTooLarge`（413）・`Unavailable`（503）で、service の `errors` に置いたので、どの操作の誤りの型にも入る（送り手は型で受けられる）。

| 順 | 守り | 振る舞い |
|---|---|---|
| 1 | 頭の印 | どの答えにも `X-Content-Type-Options: nosniff` と `Referrer-Policy: no-referrer` を、HTML の答えには CSP（[画面](ui.md)）を付ける（今の server の stamp と同じ。断った答えにも付く） |
| 2 | 問い合わせの記録 | 1 つにつき 1 行（method・uri・status・latency。tower-http の TraceLayer を今の server と同じ設定で）。記録の file にだけ書き、stdout には出さない。断った物も残る |
| 3 | Host と Origin | `Host` が 127.0.0.1・localhost でなければ、`GET`・`HEAD` のほかの要求に `Origin` が付いていれば、403 `Forbidden`（今の server と同じ決まり） |
| 3 | HEAD | `GET` にして通し、本文を捨てる（状態と header は `GET` と同じ）。今の server は axum の `get` の道が HEAD にも答えていた。生成した骨の道は GET だけなので、ここで揃える（`HEAD /healthz` は 200）。GET の無い道の HEAD は、GET と同じ答え（405 か 404） |
| 4 | 時間切れ | 答えの頭まで 5 秒（`REQUEST_TIMEOUT`）を過ぎれば 503 `Unavailable`（待っている物は捨てる。今の server と同じ）。本文を送る間は数えないので、大きな backup の file を送る途中では切らない（今の server と同じ）。試験は `Config` で短くする。接続を待つ長さ（書く `WRITE_WAIT` 2 秒・読む `READ_WAIT` 3 秒）は、この時間切れより短くする（混んでいる時に「混んでいる」の 503 が先に答え、送り手が時間切れと見分けられるように。試験で止めている） |
| 5 | 本文の上限 | 2 MiB（`BODY_LIMIT`。axum の既定と同じ）。生成した骨は本文を自分で読むので axum の上限が掛からない。だから守りの層が上限まで先に読み切り、超えれば 413 `PayloadTooLarge`（`Content-Length` の有る無しを問わない。castlist の本文は小さな JSON）。途中で切れた本文は 400 `SerializationException`（生成した層が読めない本文に返すのと同じ型） |

## データ

- DB は SQLite のまま、表を新しい migrations（`crates/server/migrations/`）で作る。表の作りは今の物を土台にし、決まり（足すだけの trigger・seq・外のキー）を引き継ぐ。
- id の値（アクターの UUID v7・出来事・線・設定の送り手の id）は変えない（pagetray の送り手と、切り替えをまたいで送り続ける roster の id のため）。新しい id の字の検査は、今の id をみな受ける。
- 今の DB からの移し（`castlist-next migrate --from <写し> --to <新しい DB>`。`crates/server/src/migrate.rs`）:
  - 移す元は、今の server の `GET /v1/backup` で取った写し（`VACUUM INTO` の file）。読むだけ（SQLite の URI の `mode=ro&immutable=1`）で開き、書かない。隣に中身のある `-wal` があれば、動いている DB なので断る。時刻の列の名は、表ごとに列を見て決める（0.3 からの `sent_at` と、0.1.x の `at` のどちらも読む）。castlist の 8 つの表が無い file・SQLite でない file・無い file も断る。どれも、移す先を作る前に断る。
  - 移す先は、無ければ作り、新しい migrations を当ててから、台帳の行（アクター・ラベル・付随する id・線・出来事・model と effort）が 1 つも無いことを見る。あれば断る（2 度目の移しも、移し終えた先なので断る）。移す元と同じ file も断る。
  - 1 つの transaction の中で、親の表から順（actors・labels・bindings・links・events・models・efforts・settings）に、insert だけで写す。外のキーと足すだけの trigger は効いたまま。`seq`・送り手の id（id の無い頼んだ物の設定も）・`received_at`・`sent_at`（時差の付いた字も、残した字のまま。新しい server は読む時に UTC に直す）・`clock_skew`・`body_hash`・`detail` は字のまま。`seq` をそのまま入れるので、`sqlite_sequence` も続き、移した後の行は次の番号から足される（名の番号も、アクターの最大の次）。
  - model と effort は名で結び直す（新しい DB の migrations が effort の 5 つ（low・medium・high・xhigh・max）を先に置くので、中の番号は替わりうる）。model と、先に置かれた物の外の effort は `first_seen_at` も写す。先に置かれた 5 つの `first_seen_at` は新しい DB の物になる（行を書き替えないため）。
  - 写した後、同じ transaction の中で突き合わせる: 表ごとの行の数・中身の digest（意味のある列を 1 行 1 つの JSON の配列にして受けた順に並べた字の SHA-256。model と effort の番号は名に替え、先に置かれた effort の `first_seen_at` は外す）・seq の続き（`sqlite_sequence`。行が無ければ 0）・外のキー（`foreign_key_check`）。1 つでも合わなければ、何も足さずに戻し、1 で終わる。合えば commit し、表ごとの行（`actors 4 → 4 digest … → … ok`）を出して 0 で終わる。引数の誤りは 2。
  - 起こす前に打つ物で、記録の置き場（`logs/`）は作らない。試験は `crates/server/tests/migrate.rs`（移す元は、同じ workspace の今の server の Router に /v1 の道で書き、`GET /v1/backup` で取る。0.1.x の形は、その写しの列の名を `at` に戻して作る）。
- 同じ id で同じ本文かを見る写し（`body_hash`）は、新しい本文の形で取る。今の DB から移した行の写しは今の形の物なので、新しい形の本文で同じ id を送り直すと 409 になる。特別な決まり（移した印で 200 にする）は置かない: 送り手の roster は 409 を「もう在るので済んだ」とみなす（今の server でも、送り手の時刻が写しに入るので、送り直しは 409 になる）。段 6c で roster が `Conflict` を済んだと扱い続けることを、この決まりの前提にする。
- 写しの形（`crates/server/src/body.rs`。替えれば、前に受けた物の送り直しが 409 になる）:
  1. 本文の欄を、定義の名の JSON の物にする。道の id は入れない。出来事は `issuer`・`by`・`type`・`detail`・`sentAt`、線は `issuer`・`type`・`from`・`to`・`sentAt`、model と effort は `issuer`・`by`・`model`・`effort`・`source`・`sentAt`。
  2. `detail` は、無ければ（JSON の null も）空の物 `{}`。`by`・`from`・`to` は送られた参照の字（`種類:値`）のまま。
  3. `sentAt` は DB に残す送り手の時刻と同じ字（UTC の RFC 3339。秒の下は要る桁だけ。例 `2026-11-24T00:00:00Z`・`2026-11-24T00:00:00.5Z`）。同じ時刻の違う書き方は同じ字になる。
  4. どの深さの物も鍵を UTF-8 の byte の順に並べ、空白を入れずに書く。字と数は serde_json の書き方（字は `"`・`\`・制御文字だけを escape する。小数は短い字）。
  5. その字の UTF-8 の byte の SHA-256 を、小文字の 16 進 64 字にする。
- 出来事の `detail` は、4 の形（鍵を並べた字）で残す。

## 後で決める物（候補）

- 鍵でない付随する id に、鍵と同じ種類の物を入れられる（例 `[claude.session:s-1, claude.session:s-2]`。s-2 は鍵でない行で残り、後で s-2 を鍵にした登録は別のアクターを作る）。今の server と同じ振る舞いで、作り直しでは替えていない。
- `asOf` は壁の時計から取るが、書く時の `receivedAt` は 6 つの表の最新より前にならないよう決める。機械の時計が戻ると、答えの `asOf` が同じ答えの行の `receivedAt` より前になり得る（今の server も同じ）。読む操作の PR で、読む transaction の中で表の最新を読み、今との遅い方を `asOf` にするかを決める。
- `asOf` から後を問う時の抜け: 書く側は受けた時刻を transaction の初めに決め、その後で commit する。だから、受けた時刻が前の答えの `asOf` より前で、まだ commit していなかった行は、次に `from=<前の asOf>` で問うても入らない（今の server も同じ）。読み手は、前の `asOf` より書く鍵の待ち（2 秒）ほど前から重ねて問い、id で重なりを除く。直すなら、読む写しの中の 6 つの表の最新の `receivedAt` を「次の起点」として返す欄を足す（`asOf` の意味は替えない）。
