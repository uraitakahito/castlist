# castlist

AI と人の「アクター」の台帳のサーバー。Claude Code の session 1 つ（起動 1 回）ごとに、本当の id と名を出し、登場・退場と、誰が誰を起動したかを足すだけの記録で残す。図（タイムライン・相関図）の元の値と、監査のための検索を、API で返す。

v2.1.1 を出した（image は `ghcr.io/uraitakahito/castlist:v2.1.1`）。アクターの登録・出来事・線・model と effort・トークンの使用量・引き当て・図の点と線・タイムラインの元の値・監査の検索・DB のバックアップの API（Smithy の定義 `model/` から作る。`/api/` の下）がある。**画面は持たない**（2.0.0 で外した。アクターの一覧・プロフィール・タイムライン・相関図は、[callboard](https://github.com/uraitakahito/callboard) が、この API を通して出す）。変えたことは [CHANGELOG.md](CHANGELOG.md)。

- 本: https://uraitakahito.github.io/castlist/ （release の後に出る。原文は [docs/src/](docs/src/)）
- API: [docs/src/api.md](docs/src/api.md)・DB: [docs/src/db.md](docs/src/db.md)・画面: callboard の本の「[画面](https://uraitakahito.github.io/callboard/ui.html)」
- 動かす（Product Manager だけ）: [docs/src/run.md](docs/src/run.md)・release を出す: [docs/src/release.md](docs/src/release.md)
- 開発のしかた: [docs/src/development.md](docs/src/development.md)
- license: MIT（[LICENSE](LICENSE)）
