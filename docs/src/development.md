# 開発

## 道具

Rust は `rustup` で入れる（Homebrew の `rust` は `rust-toolchain.toml` を読まない）。版は `rust-toolchain.toml` が決め、この repo の中で `cargo` を打つと、その版が入って使われる。

画面の browser の試験（`e2e/`）だけは Node を使う。版は `e2e/.node-version`、pnpm の版は `e2e/package.json` の `packageManager`、Playwright の版は `e2e/pnpm-lock.yaml` が決める（公開から 2 日たっていない版は入れない。`e2e/pnpm-workspace.yaml`）。初めに `cd e2e && pnpm install --frozen-lockfile && pnpm exec playwright install chromium`。設定は `e2e/playwright.config.ts`（`CASTLIST_E2E_SERVER`・`CASTLIST_E2E_PORT`・`CASTLIST_E2E_CHANNEL`）。Node と Playwright は image にも binary にも入らない。

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
ci/check-sql.sh                                                    # 行を書き替える・消す SQL が無いか
ci/check-vendor.sh                                                 # 取り込んだ cytoscape が npm の物と同じか（static/vendor/SOURCES.md）
ci/test-check-release-version.sh                                   # release の tag と版の突き合わせの試験
(cd e2e && pnpm install --frozen-lockfile && pnpm test)            # 関係の網を本物の browser で見る（Playwright。castlist-server を作って空いた port で起こす）
cargo test --locked --release --test load -- --ignored --nocapture # 負荷の試験（50 の客で 10 秒）
CASTLIST_LISTEN=127.0.0.1:18802 CASTLIST_DB=/tmp/castlist-dev.db cargo run --bin castlist-server   # 手元で起こす（本物の castlist の 8802 とは別の port・別の DB）
mdbook build docs                                                  # この本を作る
```

手元で起こすのは、試しの castlist だけにする。本物の castlist の container を起こす・止める・volume を消すのは Product Manager だけ。

## 試験を先に書く

どの PR も、先に試験だけの commit を作り、落ちることを見てから、通る実装を足す。

1. 試験だけの commit（`test: … を先に書く（まだ落ちる）`）。型と関数の形は先に置き、build が通らないだけの落ち方にしない（「期待と違う」で落ちるようにする）。
2. 通るまでの最小の実装を、次の commit にする。そのあと形を整える（試験は替えない）。
3. PR の本文に「先に書いた試験」の節を置き、落ちた写しと通った写しを載せる。

試験の層:

| 層 | 中身 | 在り処 |
|---|---|---|
| 受け入れ | 本物の binary を空いた port で起こし、HTTP で叩く | `tests/server.rs`（道具は `tests/common/mod.rs`） |
| 道筋と DB | 試験ごとに一時の DB を作り、Router を直に呼ぶ。書く接続・読む接続を直に使って、DB の決まり（足すだけ・外のキー・索引）も見る | `tests/api.rs`・`tests/db.rs`・`tests/concurrency.rs`・`tests/search.rs`（監査の検索）・`tests/ui.rs`（画面）（道具は `tests/common/app.rs`） |
| 負荷 | 本物の binary を一時の DB で起こし、50 の客が 10 秒叩く（読む物の 2 割は監査の検索。ふだんは流さない） | `tests/load.rs`（CI の job `load`） |
| 単体 | DB に触らない純な関数 | 各 file の `#[cfg(test)]` |
| browser | 本物の castlist-server を空いた port と一時の DB で起こし、API で見本を置き、Chromium で関係の網を開く。点と線の数・押した先・model と effort の絞り・色（明るい・暗い）・console の誤りと CSP の違反が 0・script を切った時の字を見る | `e2e/`（Playwright。CI の job `e2e`。Node の依存の弱点は job `advisories` の `pnpm audit`） |
| image | 空の image の中で起き、断るべき物を断り、画面（関係の網も）と CSS と script が CSP の決まりどおりに出て、止めて起こし直せる | `ci/smoke-image.sh`（CI の job `image` と release の `check`） |
| release の版 | tag と `Cargo.toml` の版・`CHANGELOG.md` の節の突き合わせを、通る形と落ちる形で打つ | `ci/test-check-release-version.sh`（CI の job `test`） |

## 待たせない書き方

並列に来る問い合わせを、互いに待たせない。

- handler はみな `async`。tokio の糸を塞ぐ呼び出し（`std::thread::sleep`・`std::fs` の読み書き・同期の `TcpStream::connect` など）は、`clippy.toml` の `disallowed-methods` で断る（`cargo clippy` が誤りにする）。代わりに `tokio::time`・`tokio::fs`・`tokio::net` を使う。
- 鍵を握ったままの `await`（`clippy::await_holding_lock`）も誤りにする。
- 試験の道具（`tests/`）は同期で書くので、その file の頭で `disallowed_methods` の断りを外している。
