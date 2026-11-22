# DB

SQLite の file 1 つ（`CASTLIST_DB`。既定は `/data/castlist.db`）。表は `migrations/0001_init.sql` に、監査の検索の索引は `migrations/0002_search.sql` にあり、移行の SQL は binary に埋め込む。移行は足すだけで、前の file は替えない。

## 接続

- WAL。書く接続は 1 本だけ（番号を数える所と、同じ物かを見る所が 1 列に並ぶ）。読む接続は 4 本で、読むだけ。書いている間も、読む物は待たない。図の点と線とタイムラインの重い読みは、別の読むだけの接続 2 本だけを使い、軽い読み（引き当て・検索・画面）の 4 本を取らない（並べて描いても、引き当てが空きを待たない）。
- どの接続にも `foreign_keys=ON`・`recursive_triggers=ON`（置き換えの insert が行を消す時も、消しを断る trigger が動く）・`busy_timeout` 5 秒・一時の物は memory。
- 書く接続を 2 秒待っても空かなければ、API は 503「混んでいる」で返す。
- 起きる時に、移行の後で `PRAGMA foreign_key_check` を流す。崩れた行が 1 つでもあれば、表と rowid を出して起きない。

## 足すだけ

台帳は足すだけで、書き替えも消しもしない。

- どの表にも、`before update` と `before delete` の trigger があり、`raise(abort, 'append-only: <表>')` で断る。`update`・`delete`・`insert or replace`・`on conflict do update` のどれも、この trigger で落ちる。
- Rust の source には、行を書き替える・消す SQL を書かない。`ci/check-sql.sh` が見張り、CI で流す（`ci/testdata/bad-sql.rs` で、見張りが当てることも見る）。
- 知らない model・effort の名は `insert … on conflict(name) do nothing` で足す（あれば何もしない）。

## 時刻と順

| 欄 | 中身 |
|---|---|
| `seq` | 受けた順の番号（`integer primary key autoincrement`）。master の表には無い |
| `at` | 送り手の時刻（RFC 3339 のまま残す） |
| `received_at` | castlist の時計（UTC・ミリ秒まで・24 字）。書く transaction の中で、今と、どの表の最新の `received_at` の遅い方にする。戻らない |
| `clock_skew` | `at` と `received_at` が 5 分より離れていれば 1。受けはする |

- 「最新」は `seq`（大きい方）だけで決める。送り手の時計がずれていても、`at` では決めない。
- 時刻の窓で探すのは `received_at`。

## 表

外のキーは `create table` の中に書き、消す・替える時は断る（`on delete restrict on update restrict`）。連れて消す（cascade）・空にする（set null）は使わない。どの表も `strict`。

| 表 | 中身 | 外のキー・決まり |
|---|---|---|
| `actors` | アクター。`id`（UUID v7・castlist が出す）・`issuer`・`kind`（`ai`・`human`）・`prefix`・`n`・`name`（`<prefix>.<n>`） | `unique(issuer, name)`・`unique(issuer, prefix, n)`。`n` は同じ出し手と接頭辞のアクターの数 ＋ 1（書く transaction の中で数える。数を持つ行は作らない） |
| `labels` | ラベル（`key`・`value`）。登録の時に 1 度だけ書く | `actor_id → actors(id)`・`unique(actor_id, key)` |
| `bindings` | 付随する id（`issuer`・`type`・`value`・`is_key`） | `actor_id → actors(id)`。鍵（`is_key = 1`）は `(issuer, type, value)` で 1 つだけ（部分の unique 索引）。鍵でない物は、そのアクターの同じ種類の最新と違う時だけ足す |
| `links` | 線（`launched`・`continued`）。`id` は送り手の id、`body_hash` は本文の写し | `from_actor`・`to_actor → actors(id)`。`launched` は `to_actor` ごとに 1 つ（部分の unique 索引） |
| `events` | 出来事（10 の種類）。`detail` は JSON の物 | `actor_id → actors(id)`。`type` と `detail` は CHECK で見る |
| `models`・`efforts` | 名の一覧（master）。`id`・`name`・`first_seen_at`。`efforts` は移行で low・medium・high・xhigh・max を入れる | `name` は unique で、形を CHECK で見る |
| `settings` | model と effort（`requested`・`observed`）。`id` は送り手の id（登録の時の物は null） | `actor_id → actors(id)`・`model_id → models(id)`・`effort_id → efforts(id)` |

## view

どれも、アクター 1 つごとの「`order by seq desc limit 1`」の副問い合わせで書く（索引で引けるように）。

| view | 中身 |
|---|---|
| `actor_state` | アクターごとに、状態（稼働中・停止）を替える出来事（`entered`・`reentered`・`forked`・`cleared`・`exited`・`closed`）の最新の種類（`presence`）と、稼働中か（`present`。初めの 3 つなら 1）・初めの `entered` の `received_at`（`entered_at`）・最後の退場の `received_at`（`exited_at`） |
| `actor_current_settings` | アクターごとの settings の最新を、model と effort の名で |
| `actor_current_bindings` | アクターと種類ごとの、付随する id の最新 |

## 索引

`events(actor_id, type, seq)`・`events(type, received_at)`・`settings(actor_id, seq)`・`settings(model_id, received_at)`・`settings(effort_id, received_at)`・`bindings(actor_id, type, seq)`・`bindings(type, value, received_at)`・`links(to_actor, type)`・`links(from_actor, type, seq)`・`labels(key, value, actor_id)`・`actors(received_at, id)`。

`0002_search.sql`（監査の検索のため）:

| 索引 | 使う所 |
|---|---|
| `actors(clock_skew)`・`events(clock_skew)`・`settings(clock_skew)` | `skewed=true`（時刻ずれのある行を持つアクター・出来事・model と effort）。後ろに seq が付くので、受けた順にも引ける |
| `events(received_at)`・`settings(received_at)` | `from`・`to` の窓だけで引く出来事と model と effort（「T から後に起きた物」） |

ほかの検索の条件は、0001 の索引で引く: ラベル（`labels(key, value, actor_id)`）・時刻での付随する id（`bindings(type, value, received_at)`）・稼働した期間の窓（`events(type, received_at)`）・アクターごとの最初の登場と最後の退場（`events(actor_id, type, seq)`）・起動したアクターと線の上下（`links(from_actor, type, seq)`・`links(to_actor, type)`）・model と effort の名（`settings(model_id, received_at)`・`settings(effort_id, received_at)`）・その時の model と effort（`settings(actor_id, seq)`）。

引き当てと見る SQL（`src/db.rs` の `LOOKUPS`）と、検索の SQL（`src/search.rs` の `plans()`。条件ごとと、全部を重ねた物）は、試験が `EXPLAIN QUERY PLAN` で、大きな表を端から読まないことを見る（`tests/db.rs`・`tests/search.rs`）。

## 写し

`GET /v1/backup` は、読む接続で `VACUUM INTO` を流し、DB の隣の一時の file に揃った写しを作って返す（書く接続を塞がない）。
