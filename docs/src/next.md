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
| 誤り | 定義に書いた型だけ: 400 `ValidationException`（欄の path と訳）・404 `NotFound`・409 `Conflict`・500 `InternalError`（server の中の失敗。訳は記録に書き、答えは決まった字。送り直しても直らないので `@retryable` は付けない）・503 `Unavailable`（混んでいる・時間切れ。`@retryable`）。知らない道は生成した層が 404 `UnknownOperationException` |
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

## データ

- DB は SQLite のまま、表を新しい migrations（`crates/server/migrations/`）で作る。表の作りは今の物を土台にし、決まり（足すだけの trigger・seq・外のキー）を引き継ぐ。
- id の値（アクターの UUID v7・出来事・線・設定の送り手の id）は変えない（pagetray の送り手と、切り替えをまたいで送り続ける roster の id のため）。新しい id の字の検査は、今の id をみな受ける。
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
- 本文の上限: 生成した service は本文を自分で読むので、axum の既定の上限（2 MB）が掛からない。守りの層の PR で、上限と 413 の試験を足す。
- `asOf` は壁の時計から取るが、書く時の `receivedAt` は 6 つの表の最新より前にならないよう決める。機械の時計が戻ると、答えの `asOf` が同じ答えの行の `receivedAt` より前になり得る（今の server も同じ）。読む操作の PR で、読む transaction の中で表の最新を読み、今との遅い方を `asOf` にするかを決める。
