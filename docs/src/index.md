# castlist

AI と人の「アクター」の台帳のサーバー。

- Claude Code の session 1 つ（起動 1 回）を、1 つのアクターとする。`/clear` と fork で新しいアクターに替わり、resume では同じアクターが続く。人（Product Manager）も 1 つのアクター。
- 本当の id と名は castlist だけが出す。Claude Code の session の id や cmux の workspace は、それに「付随する id」。
- 登場・退場と、誰が誰を起動したかを、足すだけの記録（書き替えも消しもしない）で残す。
- 図（タイムライン・相関図）の元の値と、監査のための検索を、API で返す。

v2.2.0 を出した（image は `ghcr.io/uraitakahito/castlist:v2.2.0`。誤りの文を、頼みの `Accept-Language` の言語（日本語か英語。付けなければ英語）で返す版。API は Smithy の定義から作る `/api/` の下の物: [API](api.md)）。**2.0.0 から、castlist は画面を持たない。**アクターの一覧・プロフィール・タイムライン・相関図は、この API を通して [callboard](https://uraitakahito.github.io/callboard/) が出す（前の画面の URL `http://127.0.0.1:8802/…` は 404）。

## 守り

- host（castlist を動かす機械）の外からは届かない（`127.0.0.1` だけに出す）。`Host` が `127.0.0.1`・`localhost` でない要求と、`Origin` の付いた書く要求は 403（browser で開いたほかのページからの読み書きを断る物。同じ network の container からは届く。[動かす](run.md) の「同じ network の container からは届く」）。
- 画面（HTML）の答えは無い（2.0.0 から）。どの答えにも、型の推し量りを止める印（`X-Content-Type-Options: nosniff`）と、referrer を送らせない印を付ける。
- container を起こす・止める・data の volume を消すのは Product Manager だけ（repo の設定の `infra.castlist` は `pm-only`。[動かす](run.md)）。
