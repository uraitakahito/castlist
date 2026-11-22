# 画面

castlist が HTML を組んで返す画面（askama。`templates/`）。この Mac の browser で `http://127.0.0.1:8802/` を開く。

- 字はみな HTML として逃がす（札・名・出来事の `detail` など、送り手が書いた字も）。
- script は使わない。style は castlist が配る CSS（`GET /static/site.css`。binary に埋め込む）だけで、inline の style も無い。
- どの画面にも Content-Security-Policy を付ける: `default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'`。`form-action 'self'` は、一覧の絞りの form を castlist 自身へ送るため。
- 時刻は、castlist が受けた時刻（`received_at`）を日本の時刻（UTC+9）で出す。ISO の字は `<time datetime>` と `title` に置く（字に重ねると出る）。
- 明るい色と暗い色の両方を持つ（`prefers-color-scheme`）。

## `GET /` 一覧

アクターを、初めの登場の新しい順に出す。

| 列 | 中身 |
|---|---|
| 名 | プロフィールへの link |
| 種類 | AI か 人 |
| 居るか | いる・いない と、居る・居ないを替える出来事の最新（日本語の名） |
| Key・役 | 札 `tpm.key`・`tpm.role` |
| model · effort | 最新の model と effort |
| 初めの登場・最後の退場 | 受けた時刻 |
| ずれ | アクター・出来事・model と effort のどれかに、送り手の時計がずれた行がある |

絞りは、監査の検索 `GET /v1/actors` と同じ引数を URL に置く（[API](api.md) の「アクターを探す」）。form には `label`（何度でも）・`active`・`kind`・`model`・`effort`・`active_from`・`active_to`・`skewed` の欄がある。ほかの引数（`binding`・`at`・`launched_by`・`sort`・`order`・`limit`）も URL に書けば効き、form を送り直しても残る。

- form は空の欄も送るので、空の値は付けなかったのと同じにする。
- `order` が無ければ `desc`（新しい順）、`limit` が無ければ 100。
- 絞る条件が無ければ、`active_from` を今から 7 日前にして出し、そう書く（API は条件の無い検索を断るので。窓の条件は索引で引ける）。登場の記録の無いアクターは、この既定では出ない。札などで絞ると出る。
- 次の頁は「次の頁」の link（`rel="next"`）。条件はそのままで、API の `next` を `after` に付ける。
- 引数の誤りは、API と同じ訳の 400 の画面。

## `GET /actors/{id}` プロフィール

| 節 | 中身 |
|---|---|
| 頭 | 名・種類・id・出し手・登録の受けた時刻 |
| 今の様子 | 居るか（最新の出来事の名）・最新の model と effort（頼んだ・見た と受けた時刻）・初めの登場・最後の退場・ずれの印 |
| 札 | 札の全部 |
| 付随する id | 受けた順の全部（履歴）。鍵の物に「鍵」、種類ごとの最新に「今」 |
| 起こした人 | 起こした（`launched`）・継がせた（`continued`）線を上り、人の根までの鎖（根から順）。両方あれば起こした方を辿る |
| 起こした者 | このアクターが `launched` で起こした者 |
| 前と後 | `continued` で、このアクターに継がせた者（前）と、後を継いだ者（後） |
| model と effort の履歴 | 受けた時刻・model・effort・頼んだ / 見た。見た行のうち、その前の最新の頼んだ行と model か effort が違う物に「印」（監査の検索の `mismatch=true` と同じ決め方） |
| 出来事 | 受けた時刻・出来事の日本語の名（字に重ねると id）・訳（`detail.reason`）・元（`detail.source`）・ほかの `detail`・送り手の時刻・ずれの印 |

無い id は 404 の画面。

## `GET /graph/tree` 系図（相関図 A）

誰が誰を起こし、誰が誰を継いだかを、上から下への木で出す。サーバーが SVG を組んで頁に埋め込む（script は使わない）。一覧の頭の「系図」から行ける。

- 絞りは `GET /v1/graph` と同じ引数（`label`・`from`・`to`。どれも 1 つだけ。[API](api.md) の「相関図」）。絞った組に触れる線と、その線の向こうの端のアクター（一歩だけ）も出す（人の根が切れないように）。form は空の欄も送るので、空の値は付けなかったのと同じにする。条件が無ければ、全部のアクターを出す。
- 引数の誤りは、API と同じ訳の 400 の画面。当たるアクターがいなければ、SVG を出さずにそう書く。

| 物 | 描き方 |
|---|---|
| 箱 | 1 行目は名（プロフィールへの link）、2 行目は種類（人・AI）と最新の model · effort（`claude-` を落とす。`opus-5-5 · high`）、3 行目は いる・いない と、居る・居ないを替える出来事の最新（日本語の名）。人の箱は角を丸め、色を替える |
| 実線の矢印 | 起こした（`launched`）。起こした者を上に、起こされた者を下に置く |
| 点線の矢印 | 継いだ（`continued`。/clear の交代・fork の分身）。後は、前のすぐ右の兄弟に置く（前に起こした者が無ければ、前の隣の根）。後に起こした線もあれば、起こした方で置く |

並べ方（`src/tree.rs`。DB に触らない純な計算で、単体の試験を持つ）:

- 根は、組の中に起こした者がいないアクター（人の根など）。根は一番上の段に並ぶ。
- 兄弟は、初めの登場の受けた時刻の順に左から（登場の無い者は登録の受けた時刻）。
- 部分木ごとに横の幅を取り、親はその真ん中に置く。箱は重ならない。同じ記録からは、いつも同じ図。
- 箱の幅は字の数から見積もる（ASCII は字の大きさの 0.62、ほかは 1 字分。少し広めに取る）。
- 線が輪になっていても（起こした線の輪など）、輪を 1 か所で切って並べる。線は全部描く。
- 並べる計算と HTML を組むのは `tokio::task::spawn_blocking` の糸で行い、ほかの問い合わせを待たせない。
- 色は SVG の属性でなく `site.css` の class で決める（明るい色と暗い色。CSP が inline の style を断るので）。

## 出来事の名

id は英語のまま（API と DB）。日本語の名は画面だけで、`src/ui.rs` の `EVENT_NAMES` の 1 か所に置く。

| id | 名 |
|---|---|
| `entered` | 登場 |
| `reentered` | 再登場 |
| `forked` | 分身 |
| `compacted` | 要約 |
| `cleared` | 交代 |
| `exited` | 退場 |
| `closed` | 幕引き |
| `reported_in` | 名乗り |
| `model_unreadable` | model 不明 |
| `retracted` | 取り消し |

## `GET /static/site.css`

画面の CSS（`text/css`）。画面は `/static/site.css?v=<中身の写しの頭>` で読むので、中身が替われば browser は読み直す。長く持たせてよい（`Cache-Control: public, max-age=31536000, immutable`）。
