# castlist

AI と人の「アクター」の台帳のサーバー。Claude Code の session 1 つ（起動 1 回）ごとに、本当の id と名を出し、登場・退場と、誰が誰を起こしたかを足すだけの記録で残す。相関図（系図・時間の帯・関係の網）と、監査のための検索を持つ。

作っている途中（今は `/healthz` だけが答える）。

- 本: https://uraitakahito.github.io/castlist/ （release の後に出る。原文は [docs/src/](docs/src/)）
- 開発のしかた: [docs/src/development.md](docs/src/development.md)
- license: MIT（[LICENSE](LICENSE)）
