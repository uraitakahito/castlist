# castlist

AI と人の「アクター」の台帳のサーバー。

- Claude Code の session 1 つ（起動 1 回）を、1 つのアクターとする。`/clear` と fork で新しいアクターに替わり、resume では同じアクターが続く。人（Product Manager）も 1 つのアクター。
- 本当の id と名は castlist だけが出す。Claude Code の session の id や cmux の workspace は、それに「付随する id」。
- 登場・退場と、誰が誰を起動したかを、足すだけの記録（書き替えも消しもしない）で残す。
- 図（系図・タイムライン・相関図）と、監査のための検索を持つ。

v0.1.0 を出した（image は `ghcr.io/uraitakahito/castlist:v0.1.0`）。画面の字の直し（状態・図の名など）は、まだ出していない次の版に入る。アクターの登録・出来事・線・model と effort の記録と、引き当て・図の点と線・監査の検索・DB の写しの [API](api.md) が答え、アクターの一覧とプロフィールと系図（図 A）とタイムライン（図 B）と相関図（図 C）の[画面](ui.md)がある（表は [DB](db.md)）。動かし方は [動かす](run.md)、出し方は [release を出す](release.md)。

## 守り

- castlist を動かす機械の中からの要求（`127.0.0.1` への要求）だけを受ける。`Host` が `127.0.0.1`・`localhost` でない要求と、`Origin` の付いた書く要求は 403。
- 画面の応答には Content-Security-Policy を付け、castlist 自身が配る script と style だけを許す（inline の script と style も断る。form は castlist 自身へだけ送れる）。script を使うのは相関図だけで、外の library（cytoscape。MIT）も CDN から読まず、npm の物と同じかを CI で確かめて castlist が配る。
- container を起こす・止める・data の volume を消すのは Product Manager だけ（repo の設定の `infra.castlist` は `pm-only`。[動かす](run.md)）。
