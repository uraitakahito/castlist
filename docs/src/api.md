# API

どれも JSON で、この Mac の中からだけ受ける（`Host` が `127.0.0.1`・`localhost` でない要求と、`Origin` の付いた書く要求は 403）。

- アクターは、鍵の付随する id（`種類:値`。例 `claude.session:5f0c…`・`tpm.person:pm`）で指す。本当の id への引き当ては castlist がする。値には `:` があってよい（初めの `:` で分ける）。
- 時刻は RFC 3339。送り手の時刻（`at`）は、そのまま残す。castlist が受けた時刻（`received_at`）は UTC・ミリ秒まで。
- 誤りは `{"error":"…"}`（日本語）。探す答えには、castlist の今の時刻 `as_of` が付く。
- 1 つの問い合わせは 5 秒まで。過ぎれば 503。書く接続を 2 秒待っても空かなければ 503「混んでいる」。どちらも、少し待ってから送り直す。
- 本文に知らない欄があれば 422（外で決めた id を紛れ込ませない）。

名の形: issuer・付随する id の種類・札の鍵・model・effort は、英小文字・数字・`-`・`.` で 64 字まで。名の接頭辞は、英小文字・数字・`-` で 32 字まで。値は空でない 256 字まで。送り手の id（出来事・線・model と effort）は、英数字と記号で 128 字まで。

| 状態 | 訳 |
|---|---|
| 200 | 答えた。書く物では「同じ物が既にあったので、足していない」 |
| 201 | 新しく足した |
| 400 | 問い合わせの引数（`?…`）の誤り: 知らない名・読めない時刻・範囲の外の数・読めない `after`。訳に引数の名が入る |
| 404 | そのアクターは無い |
| 409 | 同じ id で、違う中身の物が既にある（前の物はそのまま） |
| 422 | 形が違う・指すアクターを知らない・種類が決まりの外 |
| 503 | 混んでいる・時間がかかりすぎた |

## `POST /v1/actors` 登録

アクターを登録し、本当の id（UUID v7）と名（`<接頭辞>.<番号>`。番号は出し手と接頭辞ごとに 1 から）を返す。`bindings` の初めの物が鍵で、鍵は 1 つのアクターにだけ付く。外で決めた id で登録する道は無い。

```json
{
  "issuer": "tpm",
  "kind": "ai",
  "name_prefix": "se",
  "labels": { "project": "castlist", "role": "software-engineer" },
  "bindings": [
    { "type": "claude.session", "value": "5f0c2a8e-…" },
    { "type": "cmux.workspace.ref", "value": "workspace:3" }
  ],
  "at": "2026-11-22T13:00:00+09:00",
  "settings": { "model": "claude-opus-5-5", "effort": "high" }
}
```

```json
{ "id": "019a…", "name": "se.1", "kind": "ai", "created": true }
```

- 201: 新しいアクター（`created: true`）。
- 200: 鍵が既にある（`created: false`。同じ id と名）。この時は、鍵でない付随する id のうち、そのアクターの同じ種類の最新と違う物だけを足し、`settings` が最新と違えば `requested` の model と effort を足す。札は登録の初めの時だけ書く。
- 422: `kind` が `ai`・`human` でない、名の形が違う、`bindings` が空、`at` が読めない、知らない欄がある。

## `PUT /v1/events/{id}` 出来事

```json
{ "issuer": "tpm", "by": "claude.session:5f0c2a8e-…", "type": "entered", "detail": {}, "at": "2026-11-22T13:00:01+09:00" }
```

```json
{ "id": "e-1", "actor_id": "019a…", "type": "entered", "detail": {}, "at": "2026-11-22T13:00:01+09:00", "received_at": "2026-11-22T04:00:01.123Z", "clock_skew": false }
```

`type` は次の 10 のどれか: `entered`・`reentered`・`forked`・`compacted`・`cleared`・`exited`・`closed`・`reported_in`・`model_unreadable`・`retracted`。`detail` は JSON の物（`{…}`。無ければ `{}`）。

- 201: 新しく足した。200: 同じ id で同じ本文が既にある。409: 同じ id で本文が違う。422: `by` のアクターを知らない・`type` が決まりの外・`detail` が物でない。

## `PUT /v1/links/{id}` 線

誰が誰を起こしたか（`launched`）・誰の後を継いだか（`continued`）。

```json
{ "issuer": "tpm", "type": "launched", "from": "tpm.person:pm", "to": "claude.session:5f0c2a8e-…", "at": "2026-11-22T13:00:00+09:00" }
```

```json
{ "id": "l-1", "type": "launched", "from": "019a…（PM）", "to": "019a…（se.1）", "at": "…", "received_at": "…", "clock_skew": false }
```

- 201・200・409・422 は出来事と同じ。ほかに、1 つのアクターが `launched` で起こされるのは 1 度だけで、2 度目は 409。`from` と `to` が同じアクターなら 422。

## `PUT /v1/settings/{id}` model と effort

```json
{ "issuer": "tpm", "by": "claude.session:5f0c2a8e-…", "model": "claude-opus-5-5", "effort": "high", "source": "observed", "at": "…" }
```

```json
{ "id": "t-1", "actor_id": "019a…", "model": "claude-opus-5-5", "effort": "high", "source": "observed", "at": "…", "received_at": "…", "clock_skew": false }
```

- `source` は `requested`（頼んだ）か `observed`（見た）。
- 知らない model・effort の名も断らず、初めて来た時に一覧に足す。断るのは、形の違う名（空・65 字以上・英小文字と数字と `-` `.` の外の字）だけ（422）。
- 201: 新しく足した。200: 同じ id で同じ本文が既にある、またはそのアクターの最新と同じ model と effort（行を足さず、最新の物を返す）。409: 同じ id で本文が違う。

## `POST /v1/resolve` 引き当て

```json
{ "issuer": "tpm", "refs": ["claude.session:5f0c2a8e-…", "claude.session:知らない物"] }
```

```json
{
  "as_of": "2026-11-22T04:00:02.000Z",
  "results": [
    { "ref": "claude.session:5f0c2a8e-…", "actor": { "id": "019a…", "name": "se.1", "kind": "ai" } },
    { "ref": "claude.session:知らない物", "actor": null }
  ]
}
```

鍵の付随する id だけで引く（鍵でない物は `null`）。`refs` は 100 個まで。形の違う参照は 422。

## `GET /v1/actors/{id}` 1 つのアクター

アクターの欄（`id`・`name`・`kind`・`issuer`・`at`・`received_at`・`clock_skew`）と、`labels`（札）・`bindings`（付随する id の全部）・`current_bindings`（種類ごとの最新）・`events`・`settings`（名で）・`current_settings`・`links_in`・`links_out`・`state`・`as_of`。どの一覧も受けた順（seq の順）。

```json
{
  "id": "019a…", "name": "se.1", "kind": "ai", "issuer": "tpm",
  "labels": { "project": "castlist" },
  "state": { "presence": "entered", "present": true, "entered_at": "…", "exited_at": null },
  "current_settings": { "model": "claude-opus-5-5", "effort": "high", "source": "observed", "at": "…", "received_at": "…" },
  "as_of": "…"
}
```

`state.present` は、居る・居ないを替える出来事の最新（seq で）が `entered`・`reentered`・`forked` なら `true`、`cleared`・`exited`・`closed` なら `false`（まだ無ければ `false`・`presence` は `null`）。無いアクターは 404。

## 監査の検索

Auditor（台帳の件を監査する AI の席）が、件の記録に残っている物だけで castlist を引くための道。Auditor が持つのは、project・issue の Key（札 `tpm.key`）・役（札 `tpm.role`）・cmux の workspace（付随する id `cmux.workspace.ref`）・時刻だけ。どれも読む接続だけで答え、ほかの問い合わせと同じく 5 秒まで。

### 時刻の決まり

- 時刻の条件（`at`・`active_from`・`active_to`・`from`・`to`）は、どれも castlist が受けた時刻（`received_at`）と比べる。送り手の時刻（`at`）とは比べない。送り手の時計がずれていても、答えが揺れないように。
- 「最新」は `seq` の大きい方。
- 窓の両端は含む（`from ≤ received_at ≤ to`）。前の答えの `as_of` を `from` に渡せば、その後に来た物を取りこぼさない（同じミリ秒の物は、前の答えと重なりうる）。
- 答えの行には、`at`・`received_at`・`clock_skew` を、どれも付ける。

### 頁

- `limit` は 1 頁の数（既定 100・1000 まで）。
- 答えの `next` を、次の要求の `after` にそのまま渡す（中は読まない）。終わりでは `next` が `null`。
- 並べる値と `seq` の続きから読む（keyset）ので、辿る間に行が足されても、元からあった行に穴も重なりも出ない（足された行は、並べ方によって後ろに出るか、もう過ぎた所に入って出ない）。
- 別の検索の `next`、`sort`・`order` を替えた時の `next` は 400。

### 断る物

知らない引数の名・1 つだけの引数を 2 度・読めない時刻・`limit` が 1〜1000 の外・読めない `after`・決まりの外の値は、どれも 400（`{"error":"知らない引数: colour（使えるのは …）"}` のように、引数の名を言う）。黙って捨てない。

### アクターの一覧の 1 行

`GET /v1/actors` の `actors[]` と、線の上下の `nodes[]` は同じ形。

```json
{
  "id": "019a…", "name": "se.3", "kind": "ai", "issuer": "tpm",
  "at": "…", "received_at": "…", "clock_skew": false,
  "labels": { "project": "castlist", "tpm.key": "t-23zs", "tpm.role": "software-engineer" },
  "first_entered_at": "2026-11-22T02:10:00.000Z",
  "last_exited_at": "2026-11-22T03:00:00.000Z", "last_exit_type": "exited",
  "present": false,
  "model": "claude-opus-5-5", "effort": "high",
  "url": "/actors/019a…"
}
```

- `first_entered_at`: 初めの登場（`entered`・`reentered`・`forked` のどれか）の受けた時刻。`state.entered_at`（`entered` だけ）とは違う。
- `last_exited_at`・`last_exit_type`: 最後の退場（`cleared`・`exited`・`closed`）の受けた時刻と種類。
- `present`: 今居るか（`GET /v1/actors/{id}` の `state.present` と同じ決め方）。
- `model`・`effort`: 最新（seq で）の名。
- `url`: アクターのプロフィールの画面の path（[画面](ui.md)）。

### `GET /v1/actors` アクターを探す

条件は、どれも AND。

| 引数 | 中身 |
|---|---|
| `label=鍵:値` | その札を持つ。何度でも付けられ、どれも持つ者だけ |
| `binding=種類:値` | その付随する id を、`at` の時（無ければ今）に最後に受けたアクター（1 つまで）。workspace のように使い回される物を、時刻で引き分ける |
| `at` | `binding` の時刻。`binding` 無しでは 400 |
| `active_from`・`active_to` | 居た間が窓に掛かる。居た間は、初めの登場から最後の退場まで。最新の居る・居ないの出来事が退場でなければ、今まで居る。登場の無いアクターは入らない |
| `kind` | `ai` か `human` |
| `model`・`effort` | その名の settings の行を、1 つでも持つ（今の物でなくてよい） |
| `active` | `true` で今居る者、`false` で今居ない者 |
| `launched_by=<アクターの id>` | その者が起こした（`launched`） |
| `skewed=true` | 送り手の時計がずれた行（アクター・出来事・model と effort のどれか）を持つ |
| `sort` | `entered`（初めの登場・既定）か `exited`（最後の退場）。まだ無い者は、昇順で先・降順で後 |
| `order` | `asc`（既定）か `desc` |
| `limit`・`after` | 頁 |

索引で引ける条件（`label`・`binding`・`active_from`・`active_to`・`active=true`・`model`・`effort`・`launched_by`・`skewed` のどれか）を 1 つ以上付ける。無ければ 400（アクターの表を端から読むことになるので）。

```json
{ "as_of": "…", "actors": [{ "id": "019a…", "name": "se.1", "…": "…" }], "next": null }
```

Auditor の使い方:

- t-23zs の席のアクター（/clear で継いだ者も。継いだ者も同じ札で登録される）: `?label=tpm.key:t-23zs`
- t-23zs の席のアクターで、11:00〜12:00 に居た者: `?label=tpm.key:t-23zs&active_from=2026-11-22T11:00:00%2B09:00&active_to=2026-11-22T12:00:00%2B09:00`（`+` は `%2B` に）
- t-23zs の software-engineer で、その時間に居た者: 上に `&label=tpm.role:software-engineer` を足す
- workspace:3 に、11:30 に居たのは誰か: `?binding=cmux.workspace.ref:workspace:3&at=2026-11-22T11:30:00%2B09:00`
- tpm.1 が起こした者: `?launched_by=<tpm.1 の id>`
- sonnet で動いたことのある者・effort max を使ったことのある者: `?model=claude-sonnet-5`・`?effort=max`
- 退かないまま残っている者: `?label=tpm.key:t-23zs&active=true`
- 退き方（`last_exit_type`）を、最後に退いた順に: `?label=tpm.key:t-23zs&sort=exited&order=desc`
- 送り手の時計がずれた行を持つ者: `?label=tpm.key:t-23zs&skewed=true`
- 前の答えの後に動いた者: `?label=tpm.key:t-23zs&active_from=<前の答えの as_of>`

### `GET /v1/actors/{id}/lineage` 誰が起こしたか・誰を起こしたか

| 引数 | 中身 |
|---|---|
| `direction` | `up`（起こした側へ）・`down`（起こされた側へ）・`both`（既定） |
| `depth` | 辿る段（1〜20）。無ければ、上は根まで（20 段まで）・下は 1 段 |
| `type` | `launched` か `continued` だけを辿る（無ければ両方） |

上る時は、人（`kind: human`）のアクターで止める（人は含め、その先へは上らない）。

```json
{
  "as_of": "…", "actor_id": "019a…（se.3）",
  "nodes": [{ "id": "019a…", "name": "pm.1", "…": "…" }],
  "edges": [{ "id": "l-1", "type": "launched", "from": "019a…", "to": "019a…", "at": "…", "received_at": "…", "clock_skew": false }]
}
```

- se.3 を起こしたのは誰か（/clear の継ぎを越えて、人の PM まで）: `/v1/actors/<se.3 の id>/lineage?direction=up`
- tpm.1 が起こした者と、その先の継ぎ: `/v1/actors/<tpm.1 の id>/lineage?direction=down&depth=2`
- 無いアクターは 404。

### `GET /v1/actors/{id}/settings` X の時刻 T の model と effort

- `?at=時刻`: その時に効いていた 1 行（受けた時刻が `at` 以前で、seq の最大）。まだ無ければ `"setting": null`。
- `at` が無ければ、全部を受けた順で（`"settings": […]`）。

```json
{
  "as_of": "…", "actor_id": "019a…", "at": "2026-11-22T02:30:00.000Z",
  "setting": {
    "id": "t-1", "actor_id": "019a…", "actor_name": "se.2",
    "model": "claude-sonnet-5", "model_id": 2, "effort": "high", "effort_id": 3,
    "source": "observed", "at": "…", "received_at": "…", "clock_skew": false
  }
}
```

- se.2 は 11:30 に何で動いていたか: `/v1/actors/<se.2 の id>/settings?at=2026-11-22T11:30:00%2B09:00`
- 無いアクターは 404。

### `GET /v1/settings` model と effort の行を、アクターをまたいで

| 引数 | 中身 |
|---|---|
| `model`・`effort` | その名の行 |
| `source` | `requested` か `observed` |
| `from`・`to` | 受けた時刻の窓 |
| `mismatch=true` | 見た行（`observed`）のうち、同じアクターのその前の最新の頼んだ行（`requested`）と、model か effort が違う物。頼んだ行がまだ無ければ入らない |
| `skewed=true` | 送り手の時計がずれた行 |
| `limit`・`after` | 頁（受けた順） |

```json
{ "as_of": "…", "settings": [{ "id": "t-1", "actor_id": "019a…", "actor_name": "se.2", "model": "claude-sonnet-5", "…": "…" }], "next": null }
```

- 頼んだ物と違う model・effort で動いていた所: `?mismatch=true`
- 11:00 から後に見た model: `?source=observed&from=2026-11-22T11:00:00%2B09:00`

### `GET /v1/events` 出来事を、アクターをまたいで

| 引数 | 中身 |
|---|---|
| `type` | 出来事の種類（10 のどれか）。何度でも付けられ、どれか |
| `actor=<アクターの id>` | そのアクターの物 |
| `from`・`to` | 受けた時刻の窓 |
| `reason`・`source` | `detail` の決まった欄（`detail.reason`・`detail.source`）が、その値。ほかの欄では探せない（全文の検索はしない） |
| `skewed=true` | 送り手の時計がずれた行 |
| `limit`・`after` | 頁（受けた順） |

```json
{ "as_of": "…", "events": [{ "id": "e-1", "actor_id": "019a…", "actor_name": "devops.1", "type": "model_unreadable", "detail": { "reason": "timeout" }, "at": "…", "received_at": "…", "clock_skew": false }], "next": null }
```

- model が読めなかった所（席をまたいで）: `?type=model_unreadable`、訳で絞るなら `&reason=timeout`
- 11:00〜12:00 の退場: `?type=cleared&type=exited&type=closed&from=…&to=…`
- /clear で入った者の登場: `?type=entered&source=clear`
- 前の答えの後に起きた物: `?from=<前の答えの as_of>`

## `GET /v1/graph` 相関図

`?label=鍵:値&from=時刻&to=時刻`（どれも無くてよい。どれも 1 つだけ）。札と、登録の受けた時刻の窓で、アクターを絞る。知らない引数・読めない時刻・形の違う `label` は 400（訳に引数の名）。絞った組に触れる線と、その線の向こうの端のアクター（一歩だけ）も含める（人の根が切れないように）。

```json
{
  "as_of": "…",
  "nodes": [{ "id": "019a…", "name": "pm.1", "kind": "human", "issuer": "tpm", "at": "…", "received_at": "…", "clock_skew": false, "state": { … }, "current_settings": null }],
  "edges": [{ "id": "l-1", "type": "launched", "from": "019a…", "to": "019a…", "at": "…", "received_at": "…", "clock_skew": false }]
}
```

## `GET /v1/backup` DB の写し

動かしたまま、揃った写しを 1 つの SQLite の file で返す（`application/vnd.sqlite3`）。`curl -o castlist.db http://127.0.0.1:8802/v1/backup`。写しは DB の隣の一時の file に `VACUUM INTO` で作り、送り始めたら消す。

## `GET /healthz`

`{"status":"ok","version":"…","now":"…"}`。`now` は castlist の今の時刻（tpm が Mac の時計との差を測る）。
