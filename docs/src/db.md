# DB

SQLite の file 1 つ（`CASTLIST_DB`。既定は `/data/castlist.db`）。表・索引・view は `crates/server/migrations/0001_init.sql` にあり（1.0.0 から。0.4.x の 3 つの移行を当てた後の形を 1 つにまとめた物）、トークンの数と費えの表（`usage`）は `0002_usage.sql` が足す（2.1.0 から）。移行の SQL は binary に埋め込む。移行は足すだけで、出した file は替えない（替えていないことは `crates/server/tests/migrations.rs` が中身の写しで見る）。後の版が書いた DB は、開かずに断る（[API](api.md) の「データ」）。

## 接続

- WAL。書く接続は 1 本だけ（番号を数える所と、同じ物かを見る所が 1 列に並ぶ）。読む接続は 4 本で、読むだけ。書いている間も、読む物は待たない。図の点と線とタイムラインの重い読みは、別の読むだけの接続 2 本だけを使い、軽い読み（引き当て・検索・画面）の 4 本を取らない（並べて描いても、引き当てが空きを待たない）。
- どの接続にも `foreign_keys=ON`・`recursive_triggers=ON`（置き換えの insert が行を消す時も、消しを断る trigger が動く）・`busy_timeout` 5 秒・一時の物は memory。
- 書く接続を 2 秒・読む接続を 3 秒待っても空かなければ、API は 503「混んでいる」（`Unavailable`）で返す。
- 起きる時に、移行を当て（後の版が書いた DB なら、当てずに止まる）、その後で `PRAGMA foreign_key_check` を流す。崩れた行が 1 つでもあれば、表と rowid を出して起きない。

## 足すだけ

台帳は足すだけで、書き替えも消しもしない。

- どの表にも、`before update` と `before delete` の trigger があり、`raise(abort, 'append-only: <表>')` で断る。`update`・`delete`・`insert or replace`・`on conflict do update` のどれも、この trigger で落ちる。
- Rust の source には、行を書き替える・消す SQL を書かない。`ci/check-sql.sh` が見張り、CI で流す（`ci/testdata/bad-sql.rs` で、見張りが当てることも見る）。
- 知らない model・effort の名は `insert … on conflict(name) do nothing` で足す（あれば何もしない）。

## 時刻と順

| 欄 | 中身 |
|---|---|
| `seq` | 受けた順の番号（`integer primary key autoincrement`）。master の表には無い |
| `sent_at` | 送り手の時刻。送り手（今は roster）が送る時に付けた、送り手の時計の時刻。UTC の RFC 3339 に直して残す（秒の下は要る桁だけ）。0.4.x から移した行は、送られた字のまま（時差の付いた字もある。読む所が UTC に直す）。送り手の時計がずれていれば、この値もずれる |
| `received_at` | castlist の時計（UTC・ミリ秒まで・24 字）。書く transaction の中で、今と、どの表の最新の `received_at` の遅い方にする。戻らない |
| `clock_skew` | `sent_at` と `received_at` が 5 分より離れていれば 1。受けはする |

- 「最新」は `seq`（大きい方）だけで決める。送り手の時計がずれていても、`sent_at` では決めない。
- 時刻の窓で探すのは `received_at`。

## 表

外のキーは `create table` の中に書き、消す・替える時は断る（`on delete restrict on update restrict`）。連れて消す（cascade）・空にする（set null）は使わない。どの表も `strict`。

<figure>
<svg viewBox="0 0 760 540" width="100%" role="img" aria-label="castlist の表と外のキーの ER 図" style="max-width:760px">
<defs><marker id="er-fk" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto"><path d="M0,0L10,5L0,10z" style="fill:var(--icons)"/></marker></defs>
<g font-size="13" style="fill:var(--fg)">
<g style="stroke:var(--icons);stroke-width:1.5;fill:none">
<line data-fk="labels.actor_id->actors.id" x1="210" y1="44" x2="278" y2="160" marker-end="url(#er-fk)"/>
<line data-fk="bindings.actor_id->actors.id" x1="210" y1="164" x2="278" y2="190" marker-end="url(#er-fk)"/>
<line data-fk="events.actor_id->actors.id" x1="210" y1="304" x2="278" y2="250" marker-end="url(#er-fk)"/>
<line data-fk="links.from_actor->actors.id" x1="550" y1="62" x2="482" y2="160" marker-end="url(#er-fk)"/>
<line data-fk="links.to_actor->actors.id" x1="550" y1="80" x2="482" y2="176" marker-end="url(#er-fk)"/>
<line data-fk="settings.actor_id->actors.id" x1="550" y1="184" x2="482" y2="214" marker-end="url(#er-fk)"/>
<line data-fk="settings.model_id->models.id" x1="590" y1="262" x2="530" y2="318" marker-end="url(#er-fk)"/>
<line data-fk="settings.effort_id->efforts.id" x1="680" y1="262" x2="680" y2="318" marker-end="url(#er-fk)"/>
<line data-fk="usage.actor_id->actors.id" x1="320" y1="410" x2="360" y2="262" marker-end="url(#er-fk)"/>
<line data-fk="usage.model_id->models.id" x1="430" y1="440" x2="480" y2="388" marker-end="url(#er-fk)"/>
</g>
<g data-table="actors">
<rect x="280" y="140" width="200" height="120" rx="6" style="fill:var(--bg);stroke:var(--links);stroke-width:2"/>
<rect x="280" y="140" width="200" height="24" rx="6" style="fill:var(--table-header-bg);stroke:var(--links);stroke-width:2"/>
<text x="290" y="157" font-weight="bold">actors（アクター）</text>
<text x="290" y="182">id　主キー</text>
<text x="290" y="200">issuer・name</text>
<text x="290" y="218">kind</text>
<text x="290" y="236">sent_at</text>
<text x="290" y="254">received_at</text>
</g>
<g data-table="labels">
<rect x="10" y="20" width="200" height="66" rx="6" style="fill:var(--bg);stroke:var(--table-border-color);stroke-width:2"/>
<rect x="10" y="20" width="200" height="24" rx="6" style="fill:var(--table-header-bg);stroke:var(--table-border-color);stroke-width:2"/>
<text x="20" y="37" font-weight="bold">labels（ラベル）</text>
<text x="20" y="62">actor_id　外のキー</text>
<text x="20" y="80">key・value</text>
</g>
<g data-table="bindings">
<rect x="10" y="140" width="200" height="102" rx="6" style="fill:var(--bg);stroke:var(--table-border-color);stroke-width:2"/>
<rect x="10" y="140" width="200" height="24" rx="6" style="fill:var(--table-header-bg);stroke:var(--table-border-color);stroke-width:2"/>
<text x="20" y="157" font-weight="bold">bindings（付随する id）</text>
<text x="20" y="182">actor_id　外のキー</text>
<text x="20" y="200">type・value</text>
<text x="20" y="218">is_key</text>
<text x="20" y="236">sent_at</text>
</g>
<g data-table="events">
<rect x="10" y="280" width="200" height="102" rx="6" style="fill:var(--bg);stroke:var(--table-border-color);stroke-width:2"/>
<rect x="10" y="280" width="200" height="24" rx="6" style="fill:var(--table-header-bg);stroke:var(--table-border-color);stroke-width:2"/>
<text x="20" y="297" font-weight="bold">events（出来事）</text>
<text x="20" y="322">actor_id　外のキー</text>
<text x="20" y="340">id・type</text>
<text x="20" y="358">detail（JSON）</text>
<text x="20" y="376">sent_at</text>
</g>
<g data-table="links">
<rect x="550" y="20" width="200" height="102" rx="6" style="fill:var(--bg);stroke:var(--table-border-color);stroke-width:2"/>
<rect x="550" y="20" width="200" height="24" rx="6" style="fill:var(--table-header-bg);stroke:var(--table-border-color);stroke-width:2"/>
<text x="560" y="37" font-weight="bold">links（線）</text>
<text x="560" y="62">from_actor　外のキー</text>
<text x="560" y="80">to_actor　外のキー</text>
<text x="560" y="98">id・type</text>
<text x="560" y="116">sent_at</text>
</g>
<g data-table="settings">
<rect x="550" y="160" width="200" height="102" rx="6" style="fill:var(--bg);stroke:var(--table-border-color);stroke-width:2"/>
<rect x="550" y="160" width="200" height="24" rx="6" style="fill:var(--table-header-bg);stroke:var(--table-border-color);stroke-width:2"/>
<text x="560" y="177" font-weight="bold">settings（model と effort）</text>
<text x="560" y="202">actor_id　外のキー</text>
<text x="560" y="220">model_id　外のキー</text>
<text x="560" y="238">effort_id　外のキー</text>
<text x="560" y="256">sent_at</text>
</g>
<g data-table="models">
<rect x="440" y="320" width="150" height="66" rx="6" style="fill:var(--bg);stroke:var(--table-border-color);stroke-width:2"/>
<rect x="440" y="320" width="150" height="24" rx="6" style="fill:var(--table-header-bg);stroke:var(--table-border-color);stroke-width:2"/>
<text x="450" y="337" font-weight="bold">models</text>
<text x="450" y="362">id　主キー</text>
<text x="450" y="380">name</text>
</g>
<g data-table="efforts">
<rect x="605" y="320" width="150" height="66" rx="6" style="fill:var(--bg);stroke:var(--table-border-color);stroke-width:2"/>
<rect x="605" y="320" width="150" height="24" rx="6" style="fill:var(--table-header-bg);stroke:var(--table-border-color);stroke-width:2"/>
<text x="615" y="337" font-weight="bold">efforts</text>
<text x="615" y="362">id　主キー</text>
<text x="615" y="380">name</text>
</g>
<g data-table="usage">
<rect x="210" y="410" width="220" height="120" rx="6" style="fill:var(--bg);stroke:var(--table-border-color);stroke-width:2"/>
<rect x="210" y="410" width="220" height="24" rx="6" style="fill:var(--table-header-bg);stroke:var(--table-border-color);stroke-width:2"/>
<text x="220" y="427" font-weight="bold">usage（トークンの数と費え）</text>
<text x="220" y="452">actor_id　外のキー</text>
<text x="220" y="470">model_id　外のキー</text>
<text x="220" y="488">id・from_at・to_at</text>
<text x="220" y="506">数・buckets（JSON）</text>
<text x="220" y="524">sent_at</text>
</g>
</g>
</svg>
<figcaption>表と外のキー（矢印の先が指される表）。どの表もアクター（<code>actors</code>）を指し、<code>settings</code> は <code>models</code> と <code>efforts</code> も、<code>usage</code> は <code>models</code> も指す。<code>links</code> は起こした側（<code>from_actor</code>）と起こされた側（<code>to_actor</code>）の 2 本。主な列だけを描き、列の全部は下の「列」の章。</figcaption>
</figure>

図の箱（`data-table`）と線（`data-fk`）は、`crates/server/migrations/` の `0001_init.sql`・`0002_usage.sql` の表と外のキーに合わせて描いてある（0.4.x の server にあった、図と移行した DB を突き合わせる試験は、まだ `crates/server/tests/` に無い）。表や外のキーを替える移行を足したら、図も直す。

| 表 | 中身 | 外のキー・決まり |
|---|---|---|
| `actors` | アクター。`id`（UUID v7・castlist が出す）・`issuer`・`kind`（`ai`・`human`）・`prefix`・`n`・`name`（`<prefix>.<n>`） | `unique(issuer, name)`・`unique(issuer, prefix, n)`。`n` は同じ出し手と接頭辞のアクターの数 ＋ 1（書く transaction の中で数える。数を持つ行は作らない） |
| `labels` | ラベル（`key`・`value`）。登録の時に 1 度だけ書く | `actor_id → actors(id)`・`unique(actor_id, key)` |
| `bindings` | 付随する id（`issuer`・`type`・`value`・`is_key`） | `actor_id → actors(id)`。鍵（`is_key = 1`）は `(issuer, type, value)` で 1 つだけ（部分の unique 索引）。鍵でない物は、そのアクターの同じ種類の最新と違う時だけ足す |
| `links` | 線（`launched`・`continued`）。`id` は送り手の id、`body_hash` は本文の写し | `from_actor`・`to_actor → actors(id)`。`launched` は `to_actor` ごとに 1 つ（部分の unique 索引） |
| `events` | 出来事（10 の種類）。`detail` は JSON の物 | `actor_id → actors(id)`。`type` と `detail` は CHECK で見る |
| `models`・`efforts` | 名の一覧（master）。`id`・`name`・`first_seen_at`。`efforts` は移行で low・medium・high・xhigh・max を入れる | `name` は unique で、形を CHECK で見る |
| `settings` | model と effort（`requested`・`observed`）。`id` は送り手の id（登録の時の物は null） | `actor_id → actors(id)`・`model_id → models(id)`・`effort_id → efforts(id)` |
| `usage` | トークンの数と費え（2.1.0 から）。1 行は、アクター 1 人の、時計の 1 時間（UTC の時）の内の、同じ種類（model・`agent`・`speed`・`service_tier`）の回のまとめ。`id` は送り手の id、`body_hash` は本文の写し | `actor_id → actors(id)`・`model_id → models(id)`。数は `CHECK` で 0 以上（`turns` は 1 以上）。同じアクター・同じ時・同じ種類の行は 2 つ以上在りうる（unique にしない）。**読む側は、数を足し、`max_context` は大きい方を取る** |

## 列

表ごとの列（`pragma table_xinfo` の順）。型は SQLite の宣言の型で、どの表も `strict` なので、宣言と違う型の値は入らない（`INTEGER` は整数、`TEXT` は字。真偽は `INTEGER` の 0・1）。NULL の欄の「不可」は `not null` か主キー。中身の形のうち「API が見る」とある物は、DB の `CHECK` でなく API が受ける時に確かめる。この表は `0001_init.sql`・`0002_usage.sql` の列（列・型・NULL・既定）に合わせて書いてある（突き合わせる試験は、図と同じく、まだ無い）。

### `actors`

| 列 | 型 | NULL | 既定 | 決まり・中身の形 |
|---|---|---|---|---|
| `seq` | `INTEGER` | 不可 |  | 主キー（`autoincrement`）。受けた順の番号。1 から増え、使った番号は 2 度使わない |
| `id` | `TEXT` | 不可 |  | `unique`。castlist が出す UUID v7（小文字の 16 進とハイフンの 36 字） |
| `issuer` | `TEXT` | 不可 |  | 出し手（今は `tpm`）。英小文字・数字・`.`・`-` で 64 字まで（API が見る） |
| `kind` | `TEXT` | 不可 |  | `CHECK(kind in ('ai', 'human'))` |
| `prefix` | `TEXT` | 不可 |  | 名の接頭辞。英小文字・数字・`-` で 32 字まで（API が見る） |
| `n` | `INTEGER` | 不可 |  | `CHECK(n >= 1)`。同じ出し手と接頭辞のアクターの数 ＋ 1 |
| `name` | `TEXT` | 不可 |  | `<prefix>.<n>`（例 `se.3`）。`unique(issuer, name)`・`unique(issuer, prefix, n)` |
| `sent_at` | `TEXT` | 不可 |  | 送り手の時刻（送り手が送る時に付けた、送り手の時計の時刻）。UTC の RFC 3339（秒の下は要る桁だけ。例 `2026-11-22T04:00:00.123Z`）。0.4.x から移した行は送られた字のまま（例 `2026-11-22T13:00:00.123+09:00`） |
| `received_at` | `TEXT` | 不可 |  | castlist が受けた時刻。UTC・ミリ秒まで・24 字（`2026-11-22T04:00:00.123Z`）。字の順が時刻の順 |
| `clock_skew` | `INTEGER` | 不可 |  | `CHECK(clock_skew in (0, 1))`。`sent_at` と `received_at` が 5 分より離れていれば 1 |

### `bindings`

| 列 | 型 | NULL | 既定 | 決まり・中身の形 |
|---|---|---|---|---|
| `seq` | `INTEGER` | 不可 |  | 主キー（`autoincrement`）。受けた順の番号。1 から増え、使った番号は 2 度使わない |
| `actor_id` | `TEXT` | 不可 |  | `actors(id)` を指す外のキー |
| `issuer` | `TEXT` | 不可 |  | 出し手（今は `tpm`）。英小文字・数字・`.`・`-` で 64 字まで（API が見る） |
| `type` | `TEXT` | 不可 |  | 付随する id の種類（例 `claude.session`）。英小文字・数字・`.`・`-` で 64 字まで（API が見る） |
| `value` | `TEXT` | 不可 |  | 値。制御文字の無い 256 字まで（API が見る） |
| `is_key` | `INTEGER` | 不可 |  | `CHECK(is_key in (0, 1))`。1 は鍵（登録の初めの物）で、`(issuer, type, value)` で 1 つだけ |
| `sent_at` | `TEXT` | 不可 |  | 送り手の時刻（送り手が送る時に付けた、送り手の時計の時刻）。UTC の RFC 3339（秒の下は要る桁だけ。例 `2026-11-22T04:00:00.123Z`）。0.4.x から移した行は送られた字のまま（例 `2026-11-22T13:00:00.123+09:00`） |
| `received_at` | `TEXT` | 不可 |  | castlist が受けた時刻。UTC・ミリ秒まで・24 字（`2026-11-22T04:00:00.123Z`）。字の順が時刻の順 |

### `efforts`

| 列 | 型 | NULL | 既定 | 決まり・中身の形 |
|---|---|---|---|---|
| `id` | `INTEGER` | 不可 |  | 主キー（`autoincrement`）。`settings.effort_id` が指す |
| `name` | `TEXT` | 不可 |  | `unique`。`CHECK`: 1〜64 字で、英小文字・数字・`.`・`-` だけ |
| `first_seen_at` | `TEXT` | 不可 |  | 初めて来た時の `received_at`（UTC・ミリ秒まで・24 字）。移行が入れた effort は移行を当てた時刻 |

### `events`

| 列 | 型 | NULL | 既定 | 決まり・中身の形 |
|---|---|---|---|---|
| `seq` | `INTEGER` | 不可 |  | 主キー（`autoincrement`）。受けた順の番号。1 から増え、使った番号は 2 度使わない |
| `id` | `TEXT` | 不可 |  | `unique`。送り手が付ける id（英数字と記号で 128 字まで） |
| `actor_id` | `TEXT` | 不可 |  | `actors(id)` を指す外のキー |
| `type` | `TEXT` | 不可 |  | `CHECK`: `entered`・`reentered`・`forked`・`compacted`・`cleared`・`exited`・`closed`・`reported_in`・`model_unreadable`・`retracted` のどれか |
| `detail` | `TEXT` | 不可 | `'{}'` | `CHECK`: JSON の物（`json_valid` で、`json_type` が `object`） |
| `sent_at` | `TEXT` | 不可 |  | 送り手の時刻（送り手が送る時に付けた、送り手の時計の時刻）。UTC の RFC 3339（秒の下は要る桁だけ。例 `2026-11-22T04:00:00.123Z`）。0.4.x から移した行は送られた字のまま（例 `2026-11-22T13:00:00.123+09:00`） |
| `received_at` | `TEXT` | 不可 |  | castlist が受けた時刻。UTC・ミリ秒まで・24 字（`2026-11-22T04:00:00.123Z`）。字の順が時刻の順 |
| `clock_skew` | `INTEGER` | 不可 |  | `CHECK(clock_skew in (0, 1))`。`sent_at` と `received_at` が 5 分より離れていれば 1 |
| `body_hash` | `TEXT` | 不可 |  | 本文の写し（SHA-256 の 16 進・64 字）。同じ `id` で違う本文なら 409 |

### `labels`

| 列 | 型 | NULL | 既定 | 決まり・中身の形 |
|---|---|---|---|---|
| `seq` | `INTEGER` | 不可 |  | 主キー（`autoincrement`）。受けた順の番号。1 から増え、使った番号は 2 度使わない |
| `actor_id` | `TEXT` | 不可 |  | `actors(id)` を指す外のキー |
| `key` | `TEXT` | 不可 |  | 鍵。英小文字・数字・`.`・`-` で 64 字まで（API が見る）。`unique(actor_id, key)` |
| `value` | `TEXT` | 不可 |  | 値。制御文字の無い 256 字まで（API が見る） |
| `received_at` | `TEXT` | 不可 |  | castlist が受けた時刻。UTC・ミリ秒まで・24 字（`2026-11-22T04:00:00.123Z`）。字の順が時刻の順 |

### `links`

| 列 | 型 | NULL | 既定 | 決まり・中身の形 |
|---|---|---|---|---|
| `seq` | `INTEGER` | 不可 |  | 主キー（`autoincrement`）。受けた順の番号。1 から増え、使った番号は 2 度使わない |
| `id` | `TEXT` | 不可 |  | `unique`。送り手が付ける id（英数字と記号で 128 字まで） |
| `type` | `TEXT` | 不可 |  | `CHECK(type in ('launched', 'continued'))` |
| `from_actor` | `TEXT` | 不可 |  | `actors(id)` を指す外のキー（起こした・引き継がせたアクター） |
| `to_actor` | `TEXT` | 不可 |  | `actors(id)` を指す外のキー（起こされた・引き継いだアクター） |
| `sent_at` | `TEXT` | 不可 |  | 送り手の時刻（送り手が送る時に付けた、送り手の時計の時刻）。UTC の RFC 3339（秒の下は要る桁だけ。例 `2026-11-22T04:00:00.123Z`）。0.4.x から移した行は送られた字のまま（例 `2026-11-22T13:00:00.123+09:00`） |
| `received_at` | `TEXT` | 不可 |  | castlist が受けた時刻。UTC・ミリ秒まで・24 字（`2026-11-22T04:00:00.123Z`）。字の順が時刻の順 |
| `clock_skew` | `INTEGER` | 不可 |  | `CHECK(clock_skew in (0, 1))`。`sent_at` と `received_at` が 5 分より離れていれば 1 |
| `body_hash` | `TEXT` | 不可 |  | 本文の写し（SHA-256 の 16 進・64 字）。同じ `id` で違う本文なら 409 |

### `models`

| 列 | 型 | NULL | 既定 | 決まり・中身の形 |
|---|---|---|---|---|
| `id` | `INTEGER` | 不可 |  | 主キー（`autoincrement`）。`settings.model_id`・`usage.model_id` が指す |
| `name` | `TEXT` | 不可 |  | `unique`。`CHECK`: 1〜64 字で、英小文字・数字・`.`・`-` だけ |
| `first_seen_at` | `TEXT` | 不可 |  | 初めて来た時の `received_at`（UTC・ミリ秒まで・24 字） |

### `settings`

| 列 | 型 | NULL | 既定 | 決まり・中身の形 |
|---|---|---|---|---|
| `seq` | `INTEGER` | 不可 |  | 主キー（`autoincrement`）。受けた順の番号。1 から増え、使った番号は 2 度使わない |
| `id` | `TEXT` | 可 |  | `unique`。送り手が付ける id（英数字と記号で 128 字まで）。登録の時に頼んだ物（`requested`）には無い |
| `actor_id` | `TEXT` | 不可 |  | `actors(id)` を指す外のキー |
| `model_id` | `INTEGER` | 不可 |  | `models(id)` を指す外のキー |
| `effort_id` | `INTEGER` | 不可 |  | `efforts(id)` を指す外のキー |
| `source` | `TEXT` | 不可 |  | `CHECK(source in ('requested', 'observed'))` |
| `sent_at` | `TEXT` | 不可 |  | 送り手の時刻（送り手が送る時に付けた、送り手の時計の時刻）。UTC の RFC 3339（秒の下は要る桁だけ。例 `2026-11-22T04:00:00.123Z`）。0.4.x から移した行は送られた字のまま（例 `2026-11-22T13:00:00.123+09:00`） |
| `received_at` | `TEXT` | 不可 |  | castlist が受けた時刻。UTC・ミリ秒まで・24 字（`2026-11-22T04:00:00.123Z`）。字の順が時刻の順 |
| `clock_skew` | `INTEGER` | 不可 |  | `CHECK(clock_skew in (0, 1))`。`sent_at` と `received_at` が 5 分より離れていれば 1 |
| `body_hash` | `TEXT` | 可 |  | 本文の写し（SHA-256 の 16 進・64 字）。`id` のある物だけ |

### `usage`

`0002_usage.sql`（2.1.0 から）。同じアクター・同じ時（UTC の年月日と時）・同じ種類（`model_id`・`agent`・`speed`・`service_tier`）の行は、2 つ以上在りうる（送り手が、増えた分を新しい行で送る）。**読む側は、数（`turns`・`…_tokens`・`web_…_requests`・`cost_micro_usd`・`buckets` の段ごとの数）を足し、`max_context` は大きい方を取る。**

| 列 | 型 | NULL | 既定 | 決まり・中身の形 |
|---|---|---|---|---|
| `seq` | `INTEGER` | 不可 |  | 主キー（`autoincrement`）。受けた順の番号。1 から増え、使った番号は 2 度使わない |
| `id` | `TEXT` | 不可 |  | `unique`。送り手が付ける id（英数字と記号で 128 字まで） |
| `actor_id` | `TEXT` | 不可 |  | `actors(id)` を指す外のキー |
| `model_id` | `INTEGER` | 不可 |  | `models(id)` を指す外のキー |
| `agent` | `TEXT` | 可 |  | 手伝い（subagent）の種類の名。本体の回なら NULL。`CHECK`: NULL か、1 字以上（制御文字の無い 256 字までは API が見る） |
| `speed` | `TEXT` | 不可 |  | 速さの種類の名（例 `standard`）。`CHECK`: 1 字以上（制御文字の無い 256 字までは API が見る） |
| `service_tier` | `TEXT` | 不可 |  | service tier の名（例 `standard`）。`CHECK`: 1 字以上（制御文字の無い 256 字までは API が見る） |
| `from_at` | `TEXT` | 不可 |  | 区間の始めの回の時刻（送り手の時計）。`received_at` と同じ形（UTC・ミリ秒まで・24 字。字の順が時刻の順） |
| `to_at` | `TEXT` | 不可 |  | 区間の終わりの回の時刻。形は `from_at` と同じ。`from_at` より前でなく、同じ UTC の時（初めの 13 字が同じ）の内（API が見る）。読む口の窓（`from ≤ to_at < to`）は、この列で切る |
| `turns` | `INTEGER` | 不可 |  | `CHECK(turns >= 1)`。回の数 |
| `input_tokens` | `INTEGER` | 不可 |  | `CHECK(input_tokens >= 0)` |
| `output_tokens` | `INTEGER` | 不可 |  | `CHECK(output_tokens >= 0)` |
| `cache_read_tokens` | `INTEGER` | 不可 |  | `CHECK(cache_read_tokens >= 0)` |
| `cache_write_5m_tokens` | `INTEGER` | 不可 |  | `CHECK(cache_write_5m_tokens >= 0)`。5 分の cache に書いたトークンの数 |
| `cache_write_1h_tokens` | `INTEGER` | 不可 |  | `CHECK(cache_write_1h_tokens >= 0)`。1 時間の cache に書いたトークンの数 |
| `web_search_requests` | `INTEGER` | 不可 |  | `CHECK(web_search_requests >= 0)`。web の検索の回数 |
| `web_fetch_requests` | `INTEGER` | 不可 |  | `CHECK(web_fetch_requests >= 0)`。web の取得の回数 |
| `cost_micro_usd` | `INTEGER` | 不可 |  | `CHECK(cost_micro_usd >= 0)`。額（100 万分の 1 ドル） |
| `max_context` | `INTEGER` | 不可 |  | `CHECK(max_context >= 0)`。区間の内でいちばん長い文脈（トークン） |
| `buckets` | `TEXT` | 不可 |  | `CHECK`: JSON の配列（`json_valid` で、`json_type` が `array`）。文脈の長さの段ごとの数を、`[{"bucket":1,"costMicroUsd":50000,"turns":1}]` の形で `bucket` の順に。`bucket` は 0〜40（n 段は n×25,000 以上 (n+1)×25,000 未満のトークン。40 は 1,000,000 以上）で、同じ段は 1 度だけ、`turns`・`costMicroUsd` の和は行の `turns`・`cost_micro_usd` と合う（API が見る） |
| `sent_at` | `TEXT` | 不可 |  | 送り手の時刻（送り手が送る時に付けた、送り手の時計の時刻）。UTC の RFC 3339（秒の下は要る桁だけ。例 `2026-11-22T04:00:00.123Z`） |
| `received_at` | `TEXT` | 不可 |  | castlist が受けた時刻。UTC・ミリ秒まで・24 字（`2026-11-22T04:00:00.123Z`）。字の順が時刻の順 |
| `clock_skew` | `INTEGER` | 不可 |  | `CHECK(clock_skew in (0, 1))`。`sent_at` と `received_at` が 5 分より離れていれば 1 |
| `body_hash` | `TEXT` | 不可 |  | 本文の写し（SHA-256 の 16 進・64 字）。同じ `id` で違う本文なら 409 |

## view

どれも、アクター 1 つごとの「`order by seq desc limit 1`」の副問い合わせで書く（索引で引けるように）。

| view | 中身 |
|---|---|
| `actor_state` | アクターごとに、状態（稼働中・停止）を替える出来事（`entered`・`reentered`・`forked`・`cleared`・`exited`・`closed`）の最新の種類（`presence`）と、稼働中か（`present`。初めの 3 つなら 1）・初めの `entered` の `received_at`（`entered_at`）・最後の退場の `received_at`（`exited_at`） |
| `actor_current_settings` | アクターごとの settings の最新を、model と effort の名で |
| `actor_current_bindings` | アクターと種類ごとの、付随する id の最新 |

## 索引

`events(actor_id, type, seq)`・`events(type, received_at)`・`settings(actor_id, seq)`・`settings(model_id, received_at)`・`settings(effort_id, received_at)`・`bindings(actor_id, type, seq)`・`bindings(type, value, received_at)`・`links(to_actor, type)`・`links(from_actor, type, seq)`・`labels(key, value, actor_id)`・`actors(received_at, id)`。

監査の検索のための索引（0.4.x の `0002_search.sql` の物。`0001_init.sql` の後ろの方）:

| 索引 | 使う所 |
|---|---|
| `actors(clock_skew)`・`events(clock_skew)`・`settings(clock_skew)` | `skewed=true`（時刻ずれのある行を持つアクター・出来事・model と effort）。後ろに seq が付くので、受けた順にも引ける |
| `events(received_at)`・`settings(received_at)` | `from`・`to` の窓だけで引く出来事と model と effort（「T から後に起きた物」） |

トークンの数と費え（`0002_usage.sql`）:

| 索引 | 使う所 |
|---|---|
| `usage(to_at)` | 読む口（`ListUsage`）の窓（`from ≤ to_at < to`）。ラベルで絞る時も、この索引で窓の中の行だけを読む（表は増え続けるが、窓は 31 日まで） |
| `usage(actor_id, seq)` | アクターを名指した時（`actorId`）。そのアクターの行を、受けた順に |

ほかの検索の条件は、上の索引で引く: ラベル（`labels(key, value, actor_id)`）・時刻での付随する id（`bindings(type, value, received_at)`）・稼働した期間の窓（`events(type, received_at)`）・アクターごとの最初の登場と最後の退場（`events(actor_id, type, seq)`）・起動したアクターと線の上下（`links(from_actor, type, seq)`・`links(to_actor, type)`）・model と effort の名（`settings(model_id, received_at)`・`settings(effort_id, received_at)`）・その時の model と effort（`settings(actor_id, seq)`）。

引き当てと見る SQL（`crates/server/src/db.rs` の `LOOKUPS`）と、図の SQL（`crates/server/src/graph.rs` の `LOOKUPS`）と、検索の SQL（`crates/server/src/search.rs` の `plans()`。条件ごとと、全部を重ねた物）は、試験が `EXPLAIN QUERY PLAN` で、大きな表を端から読まないことを見る（`crates/server/tests/` の `db.rs`・`graph.rs`・`search_actors.rs`・`search_rows.rs`・`usage.rs`）。

## バックアップ

`GET /api/backup` は、重い読みの接続で `VACUUM INTO` を流し、DB の隣の一時の file に揃ったバックアップを作って返す（書く接続も、軽い読みの接続も塞がない）。
