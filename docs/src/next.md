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
| 書いて足す操作 | 呼ぶ側が id を決める物は `put`（`PUT /api/events/{eventId}` など。同じ id・同じ本文は 200、新しく足せば 201、同じ id で本文が違えば 409）。状態の数は `@httpResponseCode` で返す |
| ページ送り | `@paginated`。`maxResults`（1〜1000。既定 100）と、前の答えの `nextToken` をそのまま渡す |
| 探す答え | どれも `asOf`（castlist の今の時刻）を付ける |
| 決まった字の欄 | `kind` などの決まった字は、enum でなく `@pattern` の字で書く（codegen-server 0.1.40 の作る protocol の試験が、要る enum を空の字から作って panic するため。wire の形は同じ）|
| 知らない欄 | restJson1 は知らない欄を読み飛ばす（今の 422 はやめる）。id を外から紛れ込ませる道は、定義に欄が無いので無い |

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

## データ

- DB は SQLite のまま、表を新しい migrations（`crates/server/migrations/`）で作る。表の作りは今の物を土台にし、決まり（足すだけの trigger・seq・外のキー）を引き継ぐ。
- id の値（アクターの UUID v7・出来事・線・設定の送り手の id）は変えない（pagetray の送り手と、切り替えをまたいで送り続ける roster の id のため）。新しい id の字の検査は、今の id をみな受ける。
- 同じ id で同じ本文かを見る写し（`body_hash`）は、新しい本文の形で取る。今の DB から移した行は「移した」印を持ち、同じ id の送り直しは、本文を比べずに 200 で前の物を返す（AI-36 の段 3 の移す命令）。

## 後で決める物（候補）

- 鍵でない付随する id に、鍵と同じ種類の物を入れられる（例 `[claude.session:s-1, claude.session:s-2]`。s-2 は鍵でない行で残り、後で s-2 を鍵にした登録は別のアクターを作る）。今の server と同じ振る舞いで、作り直しでは替えていない。
- 本文の上限: 生成した service は本文を自分で読むので、axum の既定の上限（2 MB）が掛からない。守りの層の PR で、上限と 413 の試験を足す。
- `asOf` は壁の時計から取るが、書く時の `receivedAt` は 6 つの表の最新より前にならないよう決める。機械の時計が戻ると、答えの `asOf` が同じ答えの行の `receivedAt` より前になり得る（今の server も同じ）。読む操作の PR で、読む transaction の中で表の最新を読み、今との遅い方を `asOf` にするかを決める。
