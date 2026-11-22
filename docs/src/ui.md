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
