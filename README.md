# castlist

AI と人の「アクター」の台帳のサーバー。Claude Code の session 1 つ（起動 1 回）ごとに、本当の id と名を出し、登場・退場と、誰が誰を起動したかを足すだけの記録で残す。図（タイムライン・相関図）と、監査のための検索を持つ。

v0.2.0 を出した（image は `ghcr.io/uraitakahito/castlist:v0.2.0`）。系図の画面（`/graph/tree`）を消したのは、まだ出していない次の版に入る。アクターの登録・出来事・線・model と effort・引き当て・図の点と線・監査の検索・DB のバックアップの API と、アクターの一覧とプロフィールとタイムライン（図 B）と相関図（図 C）の画面がある。変えたことは [CHANGELOG.md](CHANGELOG.md)。

- 本: https://uraitakahito.github.io/castlist/ （release の後に出る。原文は [docs/src/](docs/src/)）
- 画面: [docs/src/ui.md](docs/src/ui.md)・API: [docs/src/api.md](docs/src/api.md)・DB: [docs/src/db.md](docs/src/db.md)
- 動かす（Product Manager だけ）: [docs/src/run.md](docs/src/run.md)・release を出す: [docs/src/release.md](docs/src/release.md)
- 開発のしかた: [docs/src/development.md](docs/src/development.md)
- license: MIT（[LICENSE](LICENSE)）。相関図の画面が使う cytoscape も MIT（[static/vendor/cytoscape-LICENSE](static/vendor/cytoscape-LICENSE)。出どころは [static/vendor/SOURCES.md](static/vendor/SOURCES.md)）
