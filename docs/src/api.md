# API

API は Smithy の定義（`model/`）から作る。protocol は `aws.protocols#restJson1` で、本文はどれも JSON。道は `/api/` の下で、`/healthz` だけは外。host（castlist を動かす機械）の外からは届かない。`Host` が `127.0.0.1`・`localhost` でない要求と、`Origin` の付いた書く要求は 403（browser で開いたほかのページからの読み書きを断る物。同じ network の container は `Host: 127.0.0.1` で読み書きできる。[動かす](run.md) の「同じ network の container からは届く」）。

- アクターは、鍵の付随する id（`種類:値`。例 `claude.session:5f0c…`・`tpm.person:pm`）で指す。本当の id への引き当ては castlist がする。値には `:` があってよい（初めの `:` で分ける）。
- 定義の形の名は `ai.castlist`（castlist の操作）と `ai.common`（`model/model/common.smithy`。pagetray と共に使う誤り・時刻・ページ送りの形）。

## 共通の決まり

| 決まり | 形 |
|---|---|
| 欄の名 | camelCase（`sentAt`・`receivedAt`・`clockSkew`・`asOf`・`actorId`・`namePrefix`） |
| 時刻 | RFC 3339 の UTC（末尾 `Z`。`ai.common#DateTime`）。Smithy の `date-time` は時差の付いた時刻（`+09:00`）を受けず、400 `SerializationException` で断る。送り手の時刻（`sentAt`）は UTC に直して残す。castlist が受けた時刻（`receivedAt`）は UTC・ミリ秒まで。答えの時刻はマイクロ秒まで書ける形（Smithy の `date-time` の書き方。残す字は切らない） |
| 誤り | 誤りの種類は header `X-Amzn-Errortype`（定義の型の名）で、訳は本文の `message`。定義に書いた型だけ: 400 `ValidationException`（欄の path と訳）・404 `NotFound`・409 `Conflict`・500 `InternalError`（server の中の失敗。訳は記録に書き、答えは決まった字。送り直しても直らないので `@retryable` は付けない）・503 `Unavailable`（混んでいる・時間切れ。`@retryable`）。読めない本文と時刻は、生成した層が 400 `SerializationException`。知らない道は、生成した層が 404 `UnknownOperationException`（道は在るが method の違う物は 405）。守りの層が断る 403 `Forbidden`・413 `PayloadTooLarge`・503 `Unavailable`（時間切れ）は、service の `errors` に置き、どの操作の誤りにも入る（下の「守りの層」） |
| 誤りの文の言語 | 誤りの `message` は、頼みの `Accept-Language` の言語で返す（2.2.0 から）。言語は日本語（`ja`）と英語（`en`）。並びを前から見て、初めに当たった物を選ぶ（`ja-JP` は `ja`。重み `;q=` と `q=0` は見ない。header の行が 2 つあれば、初めの行だけを見る）。**header が無い・`*`・どちらでもない言語（`fr` など）は、英語。**日本語で受けるには `Accept-Language: ja` を付ける（roster と callboard は、いつも言語を付ける）。誤りの種類（`X-Amzn-Errortype`）と状態の数は、言語で変わらない。呼ぶ側は、`message` の字で分けない（字は、言語と版で変わる）。守りの層が断る文も、同じ決まりで返す。生成した層が自分で返す文（定義の形の検査の 400・読めない本文・知らない道）は、いつも英語。文は `crates/server/locales/{en,ja}/main.ftl` に在る |
| 書いて足す操作 | 呼ぶ側が id を決める物は `PUT`（`PUT /api/events/{eventId}` など。`@idempotent`）。同じ id・同じ本文は 200、新しく足せば 201、同じ id で本文が違えば 409（前の行はそのまま）。状態の数は `@httpResponseCode` で返し、本文は足した行（同じなら前の行）を GetActor の中の物と同じ形で返す。同じ id かは、本文のアクターを引くより先に見る（引けないアクターでも、同じ id なら 409） |
| 引けないアクター | 本文の `by`・`from`・`to` が出し手の中の鍵で引けない時と、線の `from` と `to` が同じアクターの時は、400 `ValidationException`（訳は `message`） |
| ページ送り | `@paginated`。`maxResults`（1〜1000。既定 100）と、前の答えの `nextToken` をそのまま渡す（中は読まない）。終わりでは `nextToken` が無い |
| 探す答え | どれも `asOf`（読み始める前の castlist の時刻）を付ける |
| 無い値 | 値の無い欄は、欄ごと無い（null は書かない） |
| 決まった字の欄 | `kind` などの決まった字は、enum でなく `@pattern` の字で書く（codegen-server 0.1.40 の作る protocol の試験が、要る enum を空の字から作って panic するため。wire の形は同じ） |
| 知らない欄 | restJson1 は本文の知らない欄を読み飛ばす。id を外から紛れ込ませる道は、定義に欄が無いので無い |
| 問い合わせの引数 | 名は camelCase（`activeFrom`・`launchedBy`・`maxResults`・`nextToken`）。値の形（時刻・`鍵:値`・id・決まった字・数の範囲）は生成した層が 400 で断る。restJson1 は知らない引数を読み飛ばし、重ねられない引数を 2 度付ければ初めの物を使う。重ねられるのは list の引数（`label`・`type`）だけ。時刻の引数は DB に残す字（UTC・ミリ秒まで・24 字。ミリ秒より下は切る）に直して比べる |
| 時間 | 1 つの問い合わせは 5 秒まで。過ぎれば 503。書く接続を 2 秒・読む接続を 3 秒待っても空かなければ 503「混んでいる」。どちらも、少し待ってから送り直す |

名の形: issuer・付随する id の種類・ラベルの鍵・model・effort は、英小文字・数字・`-`・`.` で 64 字まで（`Name`）。名の接頭辞は、英小文字・数字・`-` で 32 字まで。値は空でなく、制御文字の無い 256 字まで（`Text`）。送り手の id（出来事・線・model と effort・トークンの使用量）は、空白の無い ASCII の印字できる字で 128 字まで（`SenderId`）。アクターの id は小文字の UUID（`ActorId`）。

## 操作

| 操作 | 道 | 中身 |
|---|---|---|
| `GetHealth` | `GET /healthz` | 動いているかと、castlist の今の時刻 |
| `RegisterActor` | `POST /api/actors` | アクターの登録 |
| `GetActor` | `GET /api/actors/{actorId}` | 1 つのアクターの全部 |
| `ListActors` | `GET /api/actors` | アクターを探す（監査の検索） |
| `GetLineage` | `GET /api/actors/{actorId}/lineage` | 誰が起動したか・誰を起動したか |
| `ListActorSettings` | `GET /api/actors/{actorId}/settings` | 1 つのアクターの model と effort の全部 |
| `GetActorSettingAt` | `GET /api/actors/{actorId}/settings/at?time=…` | ある時刻に効いていた model と effort |
| `PutEvent` | `PUT /api/events/{eventId}` | 出来事を足す |
| `ListEvents` | `GET /api/events` | 出来事を、アクターをまたいで探す |
| `PutLink` | `PUT /api/links/{linkId}` | 線を足す |
| `PutSetting` | `PUT /api/settings/{settingId}` | model と effort を足す |
| `ListSettings` | `GET /api/settings` | model と effort の行を、アクターをまたいで探す |
| `ResolveActors` | `POST /api/resolve` | 鍵の付随する id からアクターを引く |
| `GetGraph` | `GET /api/graph` | 図の点と線 |
| `GetTimeline` | `GET /api/timeline` | タイムラインを描く元の値（図の点と線と、その点の出来事と、model と effort の行の全部。1.1.0 から） |
| `PutUsage` | `PUT /api/usage/{usageId}` | アクターのトークンの使用量を 1 行足す（2.1.0 から） |
| `ListUsage` | `GET /api/usage` | トークンの使用量の行を、アクターをまたいで読む（2.1.0 から） |

手の道（Smithy の外）: `GET /api/backup`（SQLite の file）・`GET /api/logs`（字）。画面は castlist に無い（2.0.0 で外した。アクターの一覧・プロフィール・タイムライン・相関図は、この API を通して [callboard](https://github.com/uraitakahito/callboard) が出す。前の画面の道 `/`・`/actors/{id}`・`/graph/…`・`/static/…` は 404）。

- `GET /api/backup`（`crates/server/src/hand.rs`）: 動かしたまま、揃ったバックアップを 1 つの SQLite の file で返す。DB の隣の一時の file（`<DB>.backup-<pid>-<番号>`）に `VACUUM INTO` で作り、開いてから名を消し、`application/vnd.sqlite3`（`Content-Disposition: attachment; filename="castlist.db"`）で読みながら送る。DB 全体を読むので、重い読みの接続で作る（軽い読みの接続を取らない）。誤りは restJson1 の形の 503 `Unavailable`・500 `InternalError`。`curl -fsS -o castlist-backup-$(date +%Y%m%d-%H%M).db http://127.0.0.1:8802/api/backup`。
- `GET /api/logs`: 残した記録（終わり方の印と、時刻つきの行）を、古い順の字で返す（`text/plain; charset=utf-8`。多くて 4 MiB と 64 KiB）。`curl -s http://127.0.0.1:8802/api/logs`。castlist を止めずに、落ちた後の調べに使う。中身の file と行の読み方は [動かす](run.md) の「落ちた後に調べる」。行の形は、この API の決まりに入らない（人が読むための字）。記録の置き場を付けた時（binary）だけ在り、無い時（Router を直に呼ぶ試験）は生成した層の 404。

## 書く操作

### `RegisterActor` 登録

アクターを登録し、本当の id（UUID v7）と名（`<接頭辞>.<番号>`。番号は出し手と接頭辞ごとに 1 から）を返す。`bindings` の初めの物が鍵で、鍵は出し手の中で 1 つのアクターにだけ付く。外で決めた id で登録する道は無い。

```json
{
  "issuer": "tpm",
  "kind": "ai",
  "namePrefix": "se",
  "labels": { "project": "castlist", "role": "software-engineer" },
  "bindings": [
    { "type": "claude.session", "value": "5f0c2a8e-…" },
    { "type": "cmux.workspace.ref", "value": "workspace:3" }
  ],
  "sentAt": "2026-11-22T04:00:00Z",
  "settings": { "model": "claude-opus-5-5", "effort": "high" }
}
```

```json
{ "actor": { "id": "019a…", "name": "se.1", "kind": "ai" }, "created": true }
```

- 201: 新しいアクター（`created: true`）。
- 200: 鍵が既にある（`created: false`。同じ id と名）。この時は `kind`・`namePrefix`・`labels` を見ず、鍵でない付随する id のうち、そのアクターの同じ種類の最新と違う物だけを足し、`settings` が最新と違えば `requested` の model と effort を足す。ラベルは登録の初めの時だけ書く。`labels` と `settings` は無くてよい。
- 400: `kind` が `ai`・`human` でない、名の形が違う、`bindings` が空、など。

### `PutEvent` 出来事

```json
{ "issuer": "tpm", "by": "claude.session:5f0c2a8e-…", "type": "entered", "detail": {}, "sentAt": "2026-11-22T04:00:01Z" }
```

```json
{ "id": "e-1", "actorId": "019a…", "type": "entered", "detail": {}, "sentAt": "2026-11-22T04:00:01Z", "receivedAt": "2026-11-22T04:00:01.123Z", "clockSkew": false }
```

`type` は次の 10 のどれか: `entered`・`reentered`・`forked`・`compacted`・`cleared`・`exited`・`closed`・`reported_in`・`model_unreadable`・`retracted`。`detail` は JSON の物（`{…}`。無ければ `{}`）で、ほかの形は 400。答えの `detail` は送られた物と同じ中身（鍵の順は送られた物と同じとは限らない）。

推しの幕引き: 送り手が、終わりの出来事を送れずに落ちたと推して後から送る `closed` は、`detail` に `"inferred": true`（JSON の真偽の true）と訳（`reason`）を付ける。castlist は、これを状態の `inferred`（下の「1 つのアクター」と「アクターの一覧の 1 行」）と、画面（callboard）の推しの帯と「停止（推し）」に使う。roster が送る訳は `workspace_gone`（居場所が消えた）と `superseded`（居場所に後から別の session が入った）。`GET /api/events?type=closed&reason=workspace_gone` で探せる。

- 201: 新しく足した。200: 同じ id で同じ本文が既にある。409: 同じ id で本文が違う。400: `by` のアクターを引けない・`type` が決まりの外・`detail` が物でない。

### `PutLink` 線

誰が誰を起動したか（`launched`）・誰から引き継いだか（`continued`）。

```json
{ "issuer": "tpm", "type": "launched", "from": "tpm.person:pm", "to": "claude.session:5f0c2a8e-…", "sentAt": "2026-11-22T04:00:00Z" }
```

```json
{ "id": "l-1", "type": "launched", "from": "019a…（PM）", "to": "019a…（se.1）", "sentAt": "…", "receivedAt": "…", "clockSkew": false }
```

- 答えの `from`・`to` はアクターの id（要求の `from`・`to` は付随する id の参照で、castlist が引き当てた物）。
- 201・200・409・400 は出来事と同じ。ほかに、1 つのアクターが `launched` で起動されるのは 1 度だけで、2 度目は別の id でも 409。`from` と `to` が同じアクターなら 400。

### `PutSetting` model と effort

```json
{ "issuer": "tpm", "by": "claude.session:5f0c2a8e-…", "model": "claude-opus-5-5", "effort": "high", "source": "observed", "sentAt": "…" }
```

```json
{ "id": "t-1", "actorId": "019a…", "model": "claude-opus-5-5", "effort": "high", "source": "observed", "sentAt": "…", "receivedAt": "…", "clockSkew": false }
```

- `source` は `requested`（指定）か `observed`（実際）。
- 知らない model・effort の名も断らず、初めて来た時に一覧に足す。断るのは、形の違う名だけ（400）。
- 201: 新しく足した。200: 同じ id で同じ本文が既にある、または新しい id でも、そのアクターの最新（seq で。`source` は比べない）と同じ model と effort（行を足さず、最新の行を返す。登録の時に頼んだ行なら `id` が無い）。その時の id は残らないので、同じ id を後で送り直すと、その時の最新と比べ直す。409: 同じ id で本文が違う。

### `PutUsage` トークンの使用量

アクター（Claude Code の席）ごとの、トークンの使用量を残す（2.1.0 から）。1 行は、**アクター 1 人の、時計の 1 時間（UTC の時）の内の、同じ種類（`model`・`agent`・`speed`・`serviceTier`）の回のまとめ**。

```json
{
  "issuer": "tpm", "by": "claude.session:5f0c2a8e-…",
  "model": "claude-opus-5-5", "agent": "Explore", "speed": "standard", "serviceTier": "standard",
  "fromAt": "2026-11-24T10:05:00Z", "toAt": "2026-11-24T10:55:00Z",
  "turns": 3,
  "inputTokens": 30, "outputTokens": 1200, "cacheReadTokens": 90000, "cacheWrite5mTokens": 0, "cacheWrite1hTokens": 4000,
  "webSearchRequests": 2, "webFetchRequests": 5,
  "costMicroUsd": 250000, "maxContext": 61000,
  "buckets": [{ "bucket": 1, "turns": 1, "costMicroUsd": 50000 }, { "bucket": 2, "turns": 2, "costMicroUsd": 200000 }],
  "sentAt": "2026-11-24T11:00:00Z"
}
```

```json
{ "id": "u-1", "actorId": "019a…", "model": "claude-opus-5-5", "agent": "Explore", "…": "…", "sentAt": "…", "receivedAt": "…", "clockSkew": false }
```

| 欄 | 中身 |
|---|---|
| `issuer`・`by` | `PutSetting` と同じ（`by` は鍵の付随する id。アクターに引き当てる） |
| `model` | model の名（`Name`）。知らない名も断らず、初めて来た時に一覧（`models`）に足す |
| `agent` | 手伝い（subagent）の種類の名（`Text`）。本体の回なら付けない（答えにも、欄ごと無い） |
| `speed`・`serviceTier` | 速さの種類・service tier の名（`Text`。例 `standard`） |
| `fromAt`・`toAt` | 区間の始めの回と終わりの回の時刻（送り手の時計）。`fromAt` は `toAt` より後でなく、2 つは**同じ UTC の時（年月日と時が同じ）の内**。違えば 400。ミリ秒まで残す（それより下は切る） |
| `turns` | 回の数（1 以上） |
| `inputTokens`・`outputTokens`・`cacheReadTokens`・`cacheWrite5mTokens`・`cacheWrite1hTokens` | トークンの数（0 以上の整数。64 bit） |
| `webSearchRequests`・`webFetchRequests` | web の検索と取得の回数（0 以上の整数。64 bit）。どちらも必ず要る（無ければ 400。無かった時は 0 を送る） |
| `costMicroUsd` | 額（100 万分の 1 ドルの整数。0 以上） |
| `maxContext` | 区間の内でいちばん長い文脈（トークン。0 以上） |
| `buckets` | 文脈の長さの段ごとの数の並び（1〜41 個）。要素は `{bucket, turns, costMicroUsd}`。`bucket` は 0〜40（n 段は n×25,000 以上 (n+1)×25,000 未満のトークン。40 は 1,000,000 以上）。同じ `bucket` は 1 度だけで、`turns` の和は行の `turns` と、`costMicroUsd` の和は行の `costMicroUsd` と合う。違えば 400。並びは問わない（答えは `bucket` の順） |
| `sentAt` | 送り手の時刻。答えには `receivedAt`・`clockSkew` が付く（ほかの行と同じ） |

- **同じアクター・同じ時・同じ種類の行は、2 つ以上在りうる**（送り手が、増えた分を新しい id の行で送る）。**読む側は、数（`turns`・トークンの数・`webSearchRequests`・`webFetchRequests`・`costMicroUsd`・`buckets` の段ごとの `turns` と `costMicroUsd`）を足し、`maxContext` は大きい方を取る。**
- 201: 新しく足した。200: 同じ id で同じ本文が既にある。409: 同じ id で本文が違う。`PutSetting` の「最新と同じなら 200」は無い（数は、同じ中身でも別の行でありうる。同じかは、id と本文の写しだけで決める）。
- 400: `by` のアクターを引けない・区間が同じ時の内でない・段の決まりを破る（どれも `ValidationException` で、訳は `message`）。負の数・`turns` が 0・`bucket` が 0〜40 の外・要る欄が無い、は生成した層の 400（欄の path と訳）。区間と段の決まりは、同じ id かを見るより先に見る（決まりを破る本文は、id が在っても 400）。

### `ResolveActors` 引き当て

```json
{ "issuer": "tpm", "refs": ["claude.session:5f0c2a8e-…", "claude.session:知らない物"] }
```

```json
{
  "asOf": "2026-11-22T04:00:02Z",
  "results": [
    { "ref": "claude.session:5f0c2a8e-…", "actor": { "id": "019a…", "name": "se.1", "kind": "ai" } },
    { "ref": "claude.session:知らない物" }
  ]
}
```

出し手の中の、鍵の付随する id だけで引く。答えは問いと同じ順で、引けない物（知らない・鍵でない・出し手が違う）は `actor` が無い。`refs` は 1〜100 個。1 つでも形の違う参照があれば、全体を 400 で断る。

## `GetActor` 1 つのアクター

アクターの欄（`id`・`name`・`kind`・`issuer`・`sentAt`・`receivedAt`・`clockSkew`）と、`labels`（ラベル）・`bindings`（付随する id の全部）・`currentBindings`（種類ごとの最新を種類の順に）・`events`・`settings`（名で）・`currentSettings`（最新の 1 つ。無ければ無い）・`linksIn`・`linksOut`・`state`・`asOf`。1 つの読む transaction の中で、揃った写しを読む。ラベルは map（JSON の鍵の順は決まらない）で、ほかの並びは受けた順（seq の順）。

```json
{
  "id": "019a…", "name": "se.1", "kind": "ai", "issuer": "tpm",
  "labels": { "project": "castlist" },
  "state": { "presence": "entered", "present": true, "inferred": false, "enteredAt": "…" },
  "currentSettings": { "actorId": "019a…", "model": "claude-opus-5-5", "effort": "high", "source": "observed", "sentAt": "…", "receivedAt": "…", "clockSkew": false },
  "asOf": "…"
}
```

`state.present` は、状態（稼働中・停止）を替える出来事の最新（seq で）が `entered`・`reentered`・`forked` なら `true`、`cleared`・`exited`・`closed` なら `false`（まだ無ければ `false` で、`presence` は無い）。`state.enteredAt` は初めの `entered` を、`state.exitedAt` は最後の退場を受けた時刻。`state.inferred` は、今の帯を終わらせた出来事（最後の稼働にする出来事より後の、初めの停止にする出来事。タイムラインの帯と同じく、停止中の退場は数えない）が推しの幕引き（`closed` で `detail.inferred` が JSON の true）なら `true`。稼働中なら `false`。字の `"true"` は数えない。なので、推しの幕引きの後に遅れて来た退場では `true` のままで、退場の後に来た推しの幕引きでは `false` のまま。無いアクターは 404。

## 監査の検索

Auditor（台帳の件を監査する AI の席）が、件の記録に残っている物だけで castlist を引くための道。Auditor が持つのは、project・issue の Key（ラベル `tpm.key`）・役（ラベル `tpm.role`）・cmux の workspace（付随する id `cmux.workspace.ref`）・時刻だけ。どれも読む接続だけで答え、ほかの問い合わせと同じく 5 秒まで。読む操作は、どれも 1 つの読む transaction の中で読み、`asOf` は読み始める前に取る。

### 時刻の決まり

- 時刻の条件（`at`・`activeFrom`・`activeTo`・`from`・`to`・`time`）は、どれも castlist が受けた時刻（`receivedAt`）と比べる。送り手の時刻（`sentAt`）とは比べない。送り手の時計がずれていても、答えが揺れないように。
- 時刻は UTC（`Z`）で書く。日本時間の 11:00 は `02:00:00Z`。
- 「最新」は `seq` の大きい方。
- 窓の両端は含む（`from ≤ receivedAt ≤ to`）。前の答えの `asOf` を `from` に渡せば、その後に来た物を引ける（同じミリ秒の物は、前の答えと重なりうる。書き終える前の行の抜けは、下の「後で決める物」）。
- 答えの行には、`sentAt`・`receivedAt`・`clockSkew` を、どれも付ける。

### ページ

- `maxResults` は 1 ページの数（既定 100・1000 まで）。
- 答えの `nextToken` を、次の要求の `nextToken` にそのまま渡す（中は読まない）。終わりでは `nextToken` が無い。
- 並べる値と `seq` の続きから読む（keyset）ので、辿る間に行が足されても、元からあった行に穴も重なりも出ない（足された行は、並べ方によって後ろに出るか、もう過ぎた所に入って出ない）。
- `ListActors` の頁の印は、検索の種類・`sort`・`order`・最後の行の並べる値と seq を 16 進にした字。`ListSettings`・`ListEvents`・`ListUsage` の物は、検索の種類と最後の行の seq を 16 進にした字。崩れた物・ほかの検索の物・`sort` か `order` の違う物は 400。ほかの条件は結ばない。
- `ListActors` の並べる値（初めの登場・最後の退場）は出来事で動くので、もう出したアクターが辿る間に退場すると出直すことがある（動かない値で分けるなら並べ方の意味が変わるので、別の課題）。

### アクターの一覧の 1 行

`ListActors` の `actors[]` と、`GetLineage` の `nodes[]` は同じ形（`ActorRow`）。

```json
{
  "id": "019a…", "name": "se.3", "kind": "ai", "issuer": "tpm",
  "sentAt": "…", "receivedAt": "…", "clockSkew": false,
  "labels": { "project": "castlist", "tpm.key": "t-23zs", "tpm.role": "software-engineer" },
  "firstEnteredAt": "2026-11-22T02:10:00Z",
  "lastExitedAt": "2026-11-22T03:00:00Z", "lastExitType": "exited",
  "present": false, "inferred": false, "presence": "exited", "skewed": false,
  "model": "claude-opus-5-5", "effort": "high",
  "url": "/actors/019a…"
}
```

- `firstEnteredAt`: 最初の登場（`entered`・`reentered`・`forked` のどれか）の受けた時刻。`state.enteredAt`（`entered` だけ）とは違う。
- `lastExitedAt`・`lastExitType`: 最後の退場（`cleared`・`exited`・`closed`）の受けた時刻と種類。
- `present`: 今稼働中か（`GetActor` の `state.present` と同じ決め方）。
- `inferred`: 推しの幕引きで停止したか（`GetActor` の `state.inferred` と同じ決め方）。
- `presence`: 稼働を替える出来事（6 種）の最新の種類（`GetActor` の `state.presence` と同じ）。1 つも無ければ、欄ごと無い。1.1.0 で足した欄。
- `skewed`: 時刻ずれの行（登録・出来事・model と effort のどれか）を持つか（絞りの `skewed` と同じ意味。線は見ない）。`clockSkew` は登録だけの印で、これとは別。1.1.0 で足した欄（前の版の答えには無い）。
- `model`・`effort`: 最新（seq で）の名。
- `url`: アクターのプロフィールの画面の path（`/actors/{id}`）。画面は callboard が、この path で出す（[callboard の画面](https://uraitakahito.github.io/callboard/ui.html)）。castlist の番地に付けても開かない（2.0.0 から）。
- 値の無い欄（まだ登場していない者の `firstEnteredAt` など）は、欄ごと無い。

### `ListActors` アクターを探す

`GET /api/actors`。条件は、どれも AND。

| 引数 | 中身 |
|---|---|
| `label=鍵:値` | そのラベルを持つ。何度でも付けられ、どれも持つ者だけ |
| `binding=種類:値` | `at`（無ければ `asOf`）までに受けた、その値の付随する id の最新の行（出し手と鍵かを問わない）のアクター（1 つまで）。workspace のように使い回される物を、時刻で引き分ける |
| `at` | `binding` の時点。castlist の時計（`receivedAt`）での時刻で、送り手の時刻（`sentAt`）ではない。`binding` 無しでは 400 |
| `activeFrom`・`activeTo` | 稼働した期間が窓に掛かる（両端を含む）。稼働した期間は、最初の登場から最後の退場まで。最新の稼働・停止の出来事が退場でなければ、今まで稼働中。登場の無いアクターは入らない |
| `kind` | `ai` か `human` |
| `model`・`effort` | その名の settings の行を、1 つでも持つ（今の物でなくてよい） |
| `active` | `true` で今稼働中のアクター、`false` で今停止しているアクター |
| `launchedBy=<アクターの id>` | そのアクターが起動した（`launched`）。id は `ActorId` の形（UUID）だけを受ける（形の違う id は 400） |
| `skewed=true` | 送り手の時計がずれた行（アクター・出来事・model と effort のどれか）を持つ。true だけを受け、false は 400（「時刻ずれの無いアクター」と読まれないように。絞らないなら付けない） |
| `sort` | `entered`（最初の登場・既定）か `exited`（最後の退場）。まだ無い者は、昇順で先・降順で後。同じ値は受けた順（同じ向き） |
| `order` | `asc`（既定）か `desc` |
| `maxResults`・`nextToken` | ページ |

索引で引ける条件（`label`・`binding`・`activeFrom`・`activeTo`・`active=true`・`model`・`effort`・`launchedBy`・`skewed` のどれか）を 1 つ以上付ける。無ければ 400（アクターの表を端から読むことになるので。`kind` と `active=false` は数えない）。

```json
{ "asOf": "…", "actors": [{ "id": "019a…", "name": "se.1", "…": "…" }] }
```

Auditor の使い方（時刻は UTC。日本時間の 11:00〜12:00 は `02:00:00Z`〜`03:00:00Z`）:

- t-23zs の席のアクター（/clear で引き継いだアクターも。引き継いだアクターも同じラベルで登録される）: `?label=tpm.key:t-23zs`
- t-23zs の席のアクターで、11:00〜12:00 に稼働したアクター: `?label=tpm.key:t-23zs&activeFrom=2026-11-22T02:00:00Z&activeTo=2026-11-22T03:00:00Z`
- t-23zs の software-engineer で、その時間に稼働したアクター: 上に `&label=tpm.role:software-engineer` を足す
- workspace:3 を、11:30 に使っていたのは誰か: `?binding=cmux.workspace.ref:workspace:3&at=2026-11-22T02:30:00Z`
- tpm.1 が起動したアクター: `?launchedBy=<tpm.1 の id>`
- sonnet で動いたことのある者・effort max を使ったことのある者: `?model=claude-sonnet-5`・`?effort=max`
- 退かないまま残っている者: `?label=tpm.key:t-23zs&active=true`
- 退き方（`lastExitType`）を、最後に退いた順に: `?label=tpm.key:t-23zs&sort=exited&order=desc`
- 送り手の時計がずれた行を持つ者: `?label=tpm.key:t-23zs&skewed=true`
- 前の答えの後に動いた者: `?label=tpm.key:t-23zs&activeFrom=<前の答えの asOf>`

### `GetLineage` 誰が起動したか・誰を起動したか

`GET /api/actors/{actorId}/lineage`。

| 引数 | 中身 |
|---|---|
| `direction` | `up`（起動した側へ）・`down`（起動された側へ）・`both`（既定） |
| `depth` | 辿る段（1〜20）。付ければ上にも下にもそれを使う。無ければ、上は根まで（20 段まで）・下は 1 段 |
| `type` | `launched` か `continued` だけを辿る（上りと下りの両方。無ければ両方の種類） |

上る時は、人（`kind: human`）のアクターで止める（人は含め、その先へは上らない。起点が人なら上らない）。点を引く所も、線を辿る所と同じ 1 つの読む transaction で読む。

```json
{
  "asOf": "…", "actorId": "019a…（se.3）",
  "nodes": [{ "id": "019a…", "name": "pm.1", "…": "…" }],
  "edges": [{ "id": "l-1", "type": "launched", "from": "019a…", "to": "019a…", "sentAt": "…", "receivedAt": "…", "clockSkew": false }]
}
```

- `nodes` は起点と、辿って着いたアクター（一覧の 1 行の形。初めの登場の順、同じなら受けた順）。`edges` は辿った線（`GetActor` の線と同じ形。受けた順）。どちらも重ならない。
- se.3 を起動したのは誰か（/clear の引き継ぎを越えて、人の PM まで）: `/api/actors/<se.3 の id>/lineage?direction=up`
- tpm.1 が起動したアクターと、その先の引き継ぎ: `/api/actors/<tpm.1 の id>/lineage?direction=down&depth=2`
- 道のアクターの id は `ActorId` の形だけを受ける（形の違う id は 400）。無いアクターは 404。

### `ListActorSettings`・`GetActorSettingAt` X の model と effort

- `ListActorSettings`（`GET /api/actors/{actorId}/settings`）: そのアクターの model と effort の全部を受けた順に返す（`settings`）。頁は分けない（1 つのアクターの行は少ない）。行は `GetActor` の model と effort と同じ形（`Setting`。アクターの名は付けない）。
- `GetActorSettingAt`（`GET /api/actors/{actorId}/settings/at?time=…`）: 時刻 `time`（castlist の時計 `receivedAt` での時刻。送り手の時刻 `sentAt` ではない）に効いていた 1 行。受けた時刻が `time` まで（含む）の行の最新（seq で）1 つを `setting` に、比べた形（UTC・ミリ秒まで）に直した `time` を `at` に返す。行が無ければ `setting` は欄ごと無い。`time` は要る（無ければ 400）。`at` は 0001 年からしか書けないので、0000 年の `time` は 400。

```json
{
  "asOf": "…", "actorId": "019a…", "at": "2026-11-22T02:30:00Z",
  "setting": {
    "id": "t-1", "actorId": "019a…",
    "model": "claude-sonnet-5", "effort": "high",
    "source": "observed", "sentAt": "…", "receivedAt": "…", "clockSkew": false
  }
}
```

- se.2 は日本時間の 11:30 に何で動いていたか: `/api/actors/<se.2 の id>/settings/at?time=2026-11-22T02:30:00Z`
- 登録の時に頼んだ行（`requested`）には `id` が無い。
- 無いアクターは 404。

### `ListSettings` model と effort の行を、アクターをまたいで

`GET /api/settings`。条件はどれも AND で、無くてもよい。並びは受けた順（seq）。

| 引数 | 中身 |
|---|---|
| `model`・`effort` | その名の行 |
| `source` | `requested` か `observed` |
| `from`・`to` | 受けた時刻の窓 |
| `mismatch=true` | 実際の行（`observed`）のうち、同じアクターのそれより前（seq で）に最後に指定した行（`requested`）と、model か effort が違う物。指定した行がまだ無ければ入らない。true だけを受け、false は 400 |
| `skewed=true` | 送り手の時計がずれた行。true だけを受け、false は 400 |
| `maxResults`・`nextToken` | ページ（受けた順） |

```json
{ "asOf": "…", "settings": [{ "id": "t-1", "actorId": "019a…", "actorName": "se.2", "model": "claude-sonnet-5", "…": "…" }] }
```

- 行（`SettingRow`）は、model と effort の欄（mixin の `SettingFields`）に `actorName` を足した形。中の番号（`models`・`efforts` の id）は出さない。
- 指定と違う model・effort で動いていた所: `?mismatch=true`
- 日本時間の 11:00 から後に実際に動いた model: `?source=observed&from=2026-11-22T02:00:00Z`

### `ListEvents` 出来事を、アクターをまたいで

`GET /api/events`。条件はどれも AND で、無くてもよい。並びは受けた順（seq）。

| 引数 | 中身 |
|---|---|
| `type` | 出来事の種類（10 のどれか。ほかは 400）。何度でも付けられ、どれか（OR） |
| `actor=<アクターの id>` | そのアクターの物（`ActorId` の形だけ） |
| `from`・`to` | 受けた時刻の窓 |
| `reason`・`source` | `detail` の同じ名の欄（`detail.reason`・`detail.source`）が、その字と等しい（空でない 256 字まで）。ほかの欄では探せない（全文の検索はしない） |
| `skewed=true` | 送り手の時計がずれた行。true だけを受け、false は 400 |
| `maxResults`・`nextToken` | ページ（受けた順） |

```json
{ "asOf": "…", "events": [{ "id": "e-1", "actorId": "019a…", "actorName": "devops.1", "type": "model_unreadable", "detail": { "reason": "timeout" }, "sentAt": "…", "receivedAt": "…", "clockSkew": false }] }
```

- 行（`EventRow`）は、出来事の欄（mixin の `EventFields`）に `actorName` を足した形。
- model が読めなかった所（席をまたいで）: `?type=model_unreadable`、訳で絞るなら `&reason=timeout`
- 11:00〜12:00 の退場: `?type=cleared&type=exited&type=closed&from=…&to=…`
- /clear で入った者の登場: `?type=entered&source=clear`
- 前の答えの後に起きた物: `?from=<前の答えの asOf>`

## トークンの使用量

### `ListUsage` トークンの使用量の行を、アクターをまたいで

`GET /api/usage`（2.1.0 から）。`PutUsage` で足した行を、そのまま返す。集計はしない（まとめるのは呼ぶ側）。条件はどれも AND。並びは受けた順（seq）。軽い読みの接続（`ListSettings`・`ListEvents` と同じ側）の、1 つの読む transaction の中で読む。

| 引数 | 中身 |
|---|---|
| `from`・`to` | 区間の終わりの時刻（`toAt`）の窓。**始めは含み、終わりは含まない**（`from ≤ toAt < to`。隣り合う窓で、行が重ならない）。どちらも要る（無ければ 400）。`to − from` は 31 日まで（越えれば 400）。ほかの検索と違い、受けた時刻（`receivedAt`）とは比べない |
| `label=鍵:値` | そのラベルを持つアクターの行（`ListActors` の `label` と同じ書き方・同じ意味）。何度でも付けられ、どれも持つアクターの行だけ |
| `actorId=<アクターの id>` | そのアクターの行（`ActorId` の形だけ） |
| `maxResults`・`nextToken` | ページ（受けた順。ほかの検索と同じ） |

```json
{
  "asOf": "…",
  "usage": [{ "id": "u-1", "actorId": "019a…", "model": "claude-opus-5-5", "speed": "standard", "serviceTier": "standard", "fromAt": "2026-11-24T10:05:00Z", "toAt": "2026-11-24T10:55:00Z", "turns": 3, "…": "…" }],
  "actors": [{ "id": "019a…", "name": "se.1", "labels": { "project": "castlist", "tpm.key": "t-23zs" } }]
}
```

- 行（`Usage`）は `PutUsage` の答えと同じ形（mixin の `UsageFields`）。アクターは `actorId` で指す。
- `actors` は、**その頁の行に出てきたアクターを 1 度ずつ**（行に初めて出た順。`id`・`name`・`labels`）。次の頁には、その頁のアクターがまた付く。
- **同じアクター・同じ時・同じ種類の行は、2 つ以上在りうる。読む側は、数を足し、`maxContext` は大きい方を取る**（上の `PutUsage`）。
- `from > to` の窓と、行の無い窓は、空の答え（`usage` も `actors` も空）。
- 2026-11-24 の 1 日（UTC）の、castlist の席の行: `?from=2026-11-24T00:00:00Z&to=2026-11-25T00:00:00Z&label=project:castlist`

## 図と画面

- `GetGraph`（`GET /api/graph?label=鍵:値&from=…&to=…`）: 画面（callboard）の相関図が使う、アクターの点と線。ラベルと、登録を受けた時刻の窓（両端を含む）でアクターの組を絞り、組に触れる線（`from` か `to` が組の中）の全部と、その線の向こうの端のアクター（一歩だけ）も入れる（人の根が切れないように）。どれも無ければ全部のアクター。点も線も受けた順（seq）。重い読みの接続（2 本）の 1 つの読む transaction の中で読み、軽い読みの接続は使わない。引数はどれも 1 つだけ（重ねれば初めの物を使う）。
- `GetTimeline`（`GET /api/timeline?label=鍵:値&from=…&to=…`。1.1.0 から）: タイムラインを描く元の値。引数と、点（`nodes`）と線（`edges`）は `GetGraph` と同じ（同じ絞り・同じ順）。それに、点のアクターの出来事の全部（`events`。受けた順。欄は `actorId`・`type`・`receivedAt`・`inferred`（推しの幕引きか）・`reason`（`detail.reason` が字の時だけ）で、`detail` の全部は出さない）と、model と effort の行の全部（`settings`。受けた順。`GetActor` の行と同じ形）を足す。4 つの並びは、重い読みの接続の 1 つの読む transaction の中で読んだ、揃った写し。並べる計算（帯・切れ・矢印・目盛り）は、呼ぶ側がする。頁は分けない（並べる計算は、組の全部を 1 度に要る）ので、答えの大きさは、組のアクターの出来事の数で決まる。広い組を引く時は、`label` か期間で絞る。
- 点（`GraphNode`）は登録の欄と、`state`（`GetActor` の `state` と同じ形）と、`currentSettings`（`GetActor` の物と同じ形。無ければ無い）。線は `GetActor` の線と同じ形。

```json
{
  "asOf": "…",
  "nodes": [{ "id": "019a…", "name": "pm.1", "kind": "human", "issuer": "tpm", "sentAt": "…", "receivedAt": "…", "clockSkew": false, "state": { "present": false, "inferred": false } }],
  "edges": [{ "id": "l-1", "type": "launched", "from": "019a…", "to": "019a…", "sentAt": "…", "receivedAt": "…", "clockSkew": false }]
}
```

- 画面は castlist に無い（2.0.0 で外した）。図を描くのは callboard で、相関図は `GetGraph`、タイムラインは `GetTimeline` の答えから描く。

## 守りの層

生成した道と手の道の前に、外から順に掛ける（`crates/server/src/guard.rs`）。断る答えは、生成した層の誤りと同じ restJson1 の形（header `X-Amzn-Errortype` に定義の型の名・本文は JSON の `{"message":…}`）にする。型は `ai.common` の `Forbidden`（403）・`PayloadTooLarge`（413）・`Unavailable`（503）で、service の `errors` に置いたので、どの操作の誤りの型にも入る（送り手は型で受けられる）。

| 順 | 守り | 振る舞い |
|---|---|---|
| 1 | 頭の印 | どの答えにも `X-Content-Type-Options: nosniff` と `Referrer-Policy: no-referrer` を付ける（断った答えにも付く。HTML の答えは無いので、CSP は付けない） |
| 2 | 問い合わせの記録 | 1 つにつき 1 行（method・uri・status・latency。tower-http の TraceLayer）。記録の file にだけ書き、stdout には出さない。断った物も残る |
| 3 | Host と Origin | `Host` が 127.0.0.1・localhost でなければ、`GET`・`HEAD` のほかの要求に `Origin` が付いていれば、403 `Forbidden` |
| 3 | HEAD | `GET` にして通し、本文を捨てる（状態と header は `GET` と同じ。`Content-Length` も）。生成した骨の道は GET だけなので、ここで揃える（`HEAD /healthz` は 200）。GET の無い道の HEAD は、GET と同じ答え（405 か 404） |
| 4 | 時間切れ | 答えの頭まで 5 秒（`REQUEST_TIMEOUT`）を過ぎれば 503 `Unavailable`（待っている物は捨てる）。本文を送る間は数えないので、大きな backup の file を送る途中では切らない。試験は `Config` で短くする。接続を待つ長さ（書く `WRITE_WAIT` 2 秒・読む `READ_WAIT` 3 秒）は、この時間切れより短くする（混んでいる時に「混んでいる」の 503 が先に答え、送り手が時間切れと見分けられるように。試験で止めている） |
| 5 | 本文の上限 | 2 MiB（`BODY_LIMIT`。axum の既定と同じ）。生成した骨は本文を自分で読むので axum の上限が掛からない。だから守りの層が上限まで先に読み切り、超えれば 413 `PayloadTooLarge`（`Content-Length` の有る無しを問わない。castlist の本文は小さな JSON）。途中で切れた本文は 400 `SerializationException`（生成した層が読めない本文に返すのと同じ型） |

## データ

- DB は SQLite で、表は `crates/server/migrations/` の SQL で作る（[DB](db.md)）。決まり（足すだけの trigger・seq・外のキー）は表が持つ。
- 台帳は足すだけで、書き替えも消しもしない。同じ id は 2 度足さない。
- id の値（アクターの UUID v7・出来事・線・設定・トークンの使用量の送り手の id）は変えない（pagetray の送り手と roster が、同じ id を送り続けるため）。
- 「最新」は seq で決める。`receivedAt` は戻らない（書く transaction の中で、今と、どの表の最新の `receivedAt` の遅い方にする）。
- 0.4.x の DB は、v1.0.0 の image の `migrate` で新しい volume へ移した（AI-36 の切り替え。手順と照らしは [release を出す](release.md) の「切り替え（済み）」）。0.4.x から移した行の送り手の時刻は送られた字のまま残り（時差の付いた字もある）、読む所が UTC に直して返す。
- server（`Db::open` と binary）は、後の版（移行を足した版）が書いた DB（`_sqlx_migrations` に、この server の最も新しい版より後の版がある物）を、移行を当てずに断る（訳は「この server より新しい版の DB。…書いた版の image で起こす」）。記録の置き場（`logs/`）と WAL の file（`-wal`・`-shm`）は書く。ほかの合わない移行（中身の写しの違う版など）も、移行を当てずに「DB の移行に落ちた」で断る。binary は 1 で終わる。試験は `crates/server/tests/migrations.rs`（出した migrations の中身が 1 byte も替わっていないことも見る）。
- 同じ id で同じ本文かを見る写し（`body_hash`）は、下の形で取る。0.4.x から移した行の写しは 0.4.x の本文の形の物なので、その行と同じ id を送り直すと 409 になる。特別な決まり（移した印で 200 にする）は置かない: 送り手の roster は 409 を「もう在るので済んだ」とみなす。
- 写しの形（`crates/server/src/body.rs`。替えれば、前に受けた物の送り直しが 409 になる）:
  1. 本文の欄を、定義の名の JSON の物にする。道の id は入れない。出来事は `issuer`・`by`・`type`・`detail`・`sentAt`、線は `issuer`・`type`・`from`・`to`・`sentAt`、model と effort は `issuer`・`by`・`model`・`effort`・`source`・`sentAt`。トークンの使用量は `issuer`・`by`・`model`・`agent`（本体の回なら欄ごと無い）・`speed`・`serviceTier`・`fromAt`・`toAt`・`turns`・`inputTokens`・`outputTokens`・`cacheReadTokens`・`cacheWrite5mTokens`・`cacheWrite1hTokens`・`webSearchRequests`・`webFetchRequests`・`costMicroUsd`・`maxContext`・`buckets`・`sentAt`（`fromAt`・`toAt` は DB に残す字（UTC・ミリ秒まで・24 字）、`buckets` は `bucket` の順に並べた物。同じ時刻の違う書き方と、段の並びの違いは、同じ本文）。
  2. `detail` は、無ければ（JSON の null も）空の物 `{}`。`by`・`from`・`to` は送られた参照の字（`種類:値`）のまま。
  3. `sentAt` は DB に残す送り手の時刻と同じ字（UTC の RFC 3339。秒の下は要る桁だけ。例 `2026-11-24T00:00:00Z`・`2026-11-24T00:00:00.5Z`）。同じ時刻の違う書き方は同じ字になる。
  4. どの深さの物も鍵を UTF-8 の byte の順に並べ、空白を入れずに書く。字と数は serde_json の書き方（字は `"`・`\`・制御文字だけを escape する。小数は短い字）。
  5. その字の UTF-8 の byte の SHA-256 を、小文字の 16 進 64 字にする。
- 出来事の `detail` は、4 の形（鍵を並べた字）で残す。

## `GetHealth`

`GET /healthz` は `{"status":"ok","version":"…","now":"…"}`。`now` は castlist の今の時刻（送り手の roster が、自分の動く機械の時計との差を測る）。

## 後で決める物（候補）

- 鍵でない付随する id に、鍵と同じ種類の物を入れられる（例 `[claude.session:s-1, claude.session:s-2]`。s-2 は鍵でない行で残り、後で s-2 を鍵にした登録は別のアクターを作る）。
- `asOf` は壁の時計から取るが、書く時の `receivedAt` は 7 つの表の最新より前にならないよう決める。機械の時計が戻ると、答えの `asOf` が同じ答えの行の `receivedAt` より前になり得る。読む transaction の中で表の最新を読み、今との遅い方を `asOf` にするかを決める。
- `asOf` から後を問う時の抜け: 書く側は受けた時刻を transaction の初めに決め、その後で commit する。だから、受けた時刻が前の答えの `asOf` より前で、まだ commit していなかった行は、次に `from=<前の asOf>` で問うても入らない。読み手は、前の `asOf` より書く鍵の待ち（2 秒）ほど前から重ねて問い、id で重なりを除く。直すなら、読む写しの中の 7 つの表の最新の `receivedAt` を「次の起点」として返す欄を足す（`asOf` の意味は替えない）。
