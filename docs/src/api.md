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

## `GET /v1/actors` 探す（基本だけ）

- `?binding=種類:値&at=時刻`: その時（受けた時刻が `at` 以前）に、その付随する id を持っていたアクター（`at` が無ければ今）。workspace のように使い回される付随する id を、時刻で引き分ける。
- `?label=鍵:値`: その札を持つアクター（登録の順）。

```json
{ "as_of": "…", "actors": [{ "id": "019a…", "name": "se.1", "kind": "ai" }] }
```

どちらか 1 つを付ける（無ければ・両方なら 422）。監査のための詳しい検索は、後の PR で足す。

## `GET /v1/graph` 相関図

`?label=鍵:値&from=時刻&to=時刻`（どれも無くてよい）。札と、登録の受けた時刻の窓で、アクターを絞る。絞った組に触れる線と、その線の向こうの端のアクター（一歩だけ）も含める（人の根が切れないように）。

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
