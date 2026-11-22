# 画面

castlist が HTML を組んで返す画面（askama。`templates/`）。castlist を動かす機械の browser で `http://127.0.0.1:8802/` を開く。

- 字はみな HTML として逃がす（札・名・出来事の `detail` など、送り手が書いた字も）。
- script を使うのは関係の網（相関図 C）だけで、castlist が配る file（`GET /static/network.js` と、取り込んだ cytoscape）だけを読む。inline の script は無い。style は castlist が配る CSS（`GET /static/site.css`）だけで、inline の style も無い。どの file も binary に埋め込む。
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

## `GET /graph/timeline` 時間の帯（相関図 B）

アクターが居た間を、横を時刻にした帯で出す。サーバーが SVG を組んで頁に埋め込む（script は使わない）。一覧の頭の「時間の帯」と、系図の頭から行ける（時間の帯の頭からは、一覧と系図へ戻れる）。

- 絞りは系図と同じ（`label`・`from`・`to`。`GET /v1/graph` と同じ組で、線の向こうの端のアクターも一歩だけ出す）。引数の誤りは 400 の画面。当たるアクターがいなければ、SVG を出さずにそう書く。
- 横は castlist が受けた時刻（`received_at`）を日本の時刻で。`from`・`to` を付ければ、横もその窓にする（窓の外は切る）。無ければ、居た間・印・線の時刻の初めから終わりまで（居る者がいれば今まで。1 分より短ければ 1 分に延ばす）。

| 物 | 描き方 |
|---|---|
| 行 | 1 つのアクターに 1 行。人を上に、ほかは初めの登場の受けた時刻の順（登場の無い者は登録の時刻）。左に名（プロフィールへの link）と種類（人・AI） |
| 帯 | 居る出来事（`entered`・`reentered`・`forked`）から、居ない出来事（`cleared`・`exited`・`closed`）まで（どれも seq の順。居る間の登場・居ない間の退場は何もしない）。退いた後にまた来れば、帯は別になる。居るままなら今（か窓の右の端）まで描き、右の端を尖らせる。窓の左の端で切った帯は、左の端を尖らせる |
| 帯の切れ | model と effort の替わり目（`settings` の行を seq の順に。送り手の時刻 `at` は見ない）で切る。同じ値が続けば切らない。登場の前に頼んだ物は、帯の初めから効く。切れに `opus-5-5 · high` の形の字を、幅に収まる時だけ書く（はみ出さない。字の幅は系図と同じ見積もり）。指を置くと（SVG の `<title>`）、名・値・頼んだ / 見た（見た値が、その前の頼んだ値と違えば「頼んだ物と違う」）・初めと終わりの時刻が出る |
| 切れの色 | model の名で決める class（`m-opus`・`m-sonnet`・`m-haiku`。ほかの model はみな `m-other`。記録が無ければ `m-none`）。凡例に、出てくる model を並べる |
| 実線の矢印 | 起こした（`launched`）。起こした者の行の、線を受けた時刻から、起こされた者の初めの帯の初めへ |
| 点線の矢印 | 継いだ（`continued`）。前の帯の終わり（後の初めより前の、最後の物）から、後の初めの帯の初めへ |
| 印 | 帯を縦に切る短い線。要約（`compacted`）・名乗り（`reported_in`）・model 不明（`model_unreadable`）。指を置くと、出来事の名と時刻が出る |
| 目盛り | 窓の長さで、10 秒〜1 年の切りのよい間（日本の時刻で揃える）を選ぶ。目盛りの間は 100 px より狭くしない。字は `時:分`（秒の間なら `時:分:秒`）で、初めと日が替わった目盛りに `月-日` を前に付ける。1 日より長い間は `月-日` |
| 今 | 窓の中なら、今の時刻に点線の縦の線 |

並べ方（`src/timeline.rs`。DB に触らない純な計算で、単体の試験を持つ）は、行ごとの居る・居ないの出来事と替わり目と印の時刻（ミリ秒）、線、窓を受けて、帯の切れ・矢印・印・目盛りの置き場を返す。同じ入力からは、いつも同じ図。並べる計算と HTML を組むのは `tokio::task::spawn_blocking` の糸で行う。色は `site.css` の class で決める（明るい色と暗い色）。矢印は帯の下に、帯の字は印の上に（縁取りを付けて）描く。

## `GET /graph/network` 関係の網（相関図 C）

誰が誰を起こし、誰が誰を継いだかを、点と線の網で出す。script を使う唯一の画面。一覧の頭の「関係の網」と、系図と時間の帯の頭から行ける（関係の網の頭からは、一覧・系図・時間の帯へ戻れる）。

- 絞りは系図と同じ（`label`・`from`・`to`）。castlist は引数を確かめるだけで（誤りは 400 の画面）、点と線は描かない。同じ引数を付けた `GET /v1/graph` の URL を、絵の場所の `data-src` に置く。
- `static/network.js` が、その URL から点と線を引き（同じ origin。CSP の `connect-src 'self'`）、cytoscape で canvas に描く。字は DOM の `textContent` でだけ置く。
- JavaScript を切った browser には、同じ絞りの系図と時間の帯を勧める字を出す（`<noscript>`。script は要らない）。下の「見せる点を選ぶ」欄は出さない。

| 物 | 描き方 |
|---|---|
| 点 | 1 つのアクター。名を下に書く（下地を地の色で塗り、横切る線を隠す）。AI は丸、人は角を丸めた四角で縁を緑に |
| 点の色 | 最新の model の系統（時間の帯と同じ決め方: `opus`・`sonnet`・`haiku`・ほか・記録なし）。色は `site.css` の `--m-opus` などを読むので、明るい色と暗い色の両方で替わる（替われば描き直す） |
| 薄い点 | 今いない者（塗りを薄く。名は薄くしない） |
| 実線の矢印 | 起こした（`launched`） |
| 点線の矢印 | 継いだ（`continued`。/clear の交代・fork の分身） |

- 並べ方は cytoscape の `cose`（力で引き合う並べ方）。初めの置き場は答えの順に円に置くので、同じ答えからは同じ図。点は引いて動かせ、ホイールで大きさを替えられる（並べ直しは頁の読み直し）。
- 点を押すと、そのアクターのプロフィールへ。字に重ねると、名・種類・model · effort・居るかが出る。
- 「見せる点を選ぶ」の model（短い名）と effort は、頁の中だけで、合わない点と、その点に付く線を隠す（API には聞き直さない）。選べる値は、描いた点に出てくる物と「記録なし」。
- 絵の下に、アクターと線の数と、API が答えた時（日本の時刻）を書く。絞った時は、見えている数も。

CSP を緩めずに cytoscape を使うための、2 つの決まり（`static/network.js`）:

- cytoscape は、初めに `<style>` を head に差し込もうとする（CSP の `style-src 'self'` が断り、違反になる）。同じ id（`__________cytoscape_stylesheet`）の印を先に置くと差し込まないので、そうする。差し込むはずだった決まり（容れ物を `position: relative` に）は `site.css` に書く。
- cytoscape は、絵の画面の中の位置を覚えておき、押した所の点を探す。覚え直すのは window の scroll と大きさの替わりの知らせが来た時だけなので、指が入った時と押す前に覚え直させる（`cy.resize()`）。

### 取り込んだ cytoscape

- [cytoscape](https://js.cytoscape.org/) 3.35.0（MIT。The Cytoscape Consortium）。npm の tarball の `package/dist/cytoscape.min.js` を、そのまま `static/vendor/cytoscape-3.35.0.min.js` に置き、castlist が binary に埋め込んで `GET /static/vendor/cytoscape-3.35.0.min.js` で配る（CDN からは読まない）。license の写しは `static/vendor/cytoscape-LICENSE`。
- 出どころの記録は `static/vendor/SOURCES.md`（package・version・tarball の URL・registry の `integrity`・tarball の中の path・置いた file の sha256・license）。
- `ci/check-vendor.sh`（CI の job `test`）が、registry の記録の tarball と `integrity` が SOURCES.md と同じか、落とした tarball の sha512 が `integrity` と同じか、tarball の中の file と commit した file の sha256 が SOURCES.md と同じか、license の file が tarball の物と同じかを見る。1 つでも違えば落ちる（書き替えた写しを当てることも CI で見る）。
- 試験の `tests/ui.rs` も、castlist が配る物の sha256 が SOURCES.md と同じかを見る。

版の上げ方:

1. 新しい版が npm の `latest` で、公開から 2 日たっていることを見る（`curl -s https://registry.npmjs.org/cytoscape | jq '."dist-tags", .time["<版>"]'`）。
2. 空の folder で tarball を落とし、`package/dist/cytoscape.min.js` を `static/vendor/cytoscape-<版>.min.js` に、`package/LICENSE` を `static/vendor/cytoscape-LICENSE` に写す。古い版の file は消す。
3. `static/vendor/SOURCES.md` の version・tarball・integrity（`https://registry.npmjs.org/cytoscape/<版>` の `dist.integrity`）・file・sha256 を書き替える。
4. 版の入った path を替える: `src/ui.rs` の `CYTOSCAPE_JS`・`CYTOSCAPE_PATH`、`tests/ui.rs` の `CYTOSCAPE`、CI の「見張りが、書き替えた写しを当てるか」の file の名。
5. `ci/check-vendor.sh`・`cargo test --locked`・`cd e2e && pnpm test`（console の誤りと CSP の違反が 0 か）を流す。cytoscape の変わり目（release の知らせ）で、`<style>` の差し込みと位置の覚え方が替わっていないかも見る。

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

## `GET /static/site.css`・`GET /static/network.js`・`GET /static/vendor/cytoscape-3.35.0.min.js`

画面の CSS（`text/css`）と、関係の網の script（`application/javascript`）。どれも binary に埋め込む。画面は `/static/site.css?v=<中身の写しの頭>`・`/static/network.js?v=<中身の写しの頭>` で読み、cytoscape は版が path に入るので、中身が替われば browser は読み直す。長く持たせてよい（`Cache-Control: public, max-age=31536000, immutable`）。
