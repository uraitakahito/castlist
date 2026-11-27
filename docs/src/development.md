# 開発

## 道具

Rust は `rustup` で入れる（Homebrew の `rust` は `rust-toolchain.toml` を読まない）。版は `rust-toolchain.toml` が決め、この repo の中で `cargo` を打つと、その版が入って使われる。

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo test --locked -p castlist_sdk                                 # 定義の例から作った、生成した骨の中の試験（protocol の試験）
ci/check-sql.sh                                                    # 行を書き替える・消す SQL が無いか
ci/test-check-release-version.sh                                   # release の tag と版の突き合わせの試験
cargo test --locked --release -p castlist --test load -- --ignored --nocapture   # 負荷の試験（50 の客で 10 秒）
CASTLIST_LISTEN=127.0.0.1:18802 CASTLIST_DB=/tmp/castlist-dev.db cargo run --bin castlist-server   # 手元で起こす（本物の castlist の 8802 とは別の port・別の DB）
mdbook build docs                                                  # この本を作る
ci/gen.sh                                                          # 定義（model/）から server の骨（crates/castlist-sdk/）を作り直す
ci/gen.sh --check                                                  # 作り直した物と入れてある物が同じか（CI の job model）
ci/gen.sh --diff                                                   # 定義が、定義の入った最新の release から壊す形に変わっていないか（CI の job model）
```

手元で起こすのは、試しの castlist だけにする。本物の castlist の container を起こす・止める・volume を消すのは Product Manager だけ。

## 定義と生成

server は `crates/server`（package `castlist`・binary `castlist-server`）の 1 つで、API は Smithy の定義から作った骨（`crates/castlist-sdk/`）に処理を入れる。

- 定義は `model/model/*.smithy`。`ai.common`（`common.smithy`）は pagetray と共に使う形（誤り・時刻・ページ送り）で、pagetray と roster は、この repo の release の tag で固めて取る。
- 骨（`crates/castlist-sdk/`）は `ci/gen.sh` が作る。**手で直さない**。定義を替えたら `ci/gen.sh` を流し、作り直した物も同じ commit に入れる。workspace の member だが、既定の命令（`default-members`）の外で clippy は掛けない（server の crate が引くので `cargo clippy` も骨を見るが、`ci/gen.sh` が骨の `lib.rs` の頭に `#![allow(clippy::all)]` を足す）。中の protocol の試験は `cargo test --locked -p castlist_sdk` で流す。fmt は生成器が `rust-toolchain.toml` の rustfmt で整えた物を、`cargo fmt --all --check` が見る。
- 生成器の版は `model/smithy-build.json`（Maven の物）と `ci/gen.sh`（Smithy の CLI と digest）に書く。CLI と trait の package の版は揃える。`smithy-build.json` に名指した Maven の物は、公開から 2 日たっていなければ `ci/gen.sh` が断る（名指した物が引く物は見ない）。
- `ci/gen.sh --diff` は、定義の入った最新の release（版の後ろに `-` の付く pre-release（rc など）を除く）と比べる。pre-release の間は、まだ誰も使っていない形なので比べない。
- server の試験は `crates/server/tests/`。定義に書く例（`@httpRequestTests`・`@httpResponseTests`）は wire の形を止める試験で、生成した骨の試験になる（試験用の handler を使うので、処理の正しさは見ない）。処理を引っ張る試験は、本物の handler を入れた Router に要求を通す振る舞いの試験で、処理より先に書く（下の節）。
- 生成した service は `crates/server/src/generated.rs` の包みで axum の Router の fallback に入れ、守りの層をその前に掛ける。

## 試験を先に書く

どの PR も、先に試験だけの commit を作り、落ちることを見てから、通る実装を足す。

1. 試験だけの commit（`test: … を先に書く（まだ落ちる）`）。型と関数の形は先に置き、build が通らないだけの落ち方にしない（「期待と違う」で落ちるようにする）。
2. 通るまでの最小の実装を、次の commit にする。そのあと形を整える（試験は替えない）。
3. PR の本文に「先に書いた試験」の節を置き、落ちた写しと通った写しを載せる。

試験の層:

| 層 | 中身 | 在り処 |
|---|---|---|
| 受け入れ | 本物の binary を空いた port で起こし、HTTP で叩く | `crates/server/tests/server.rs`（守りの層の誤りの形・HEAD・本文の上限・backup も。道具は `crates/server/tests/common/server.rs`） |
| 道筋と DB | 試験ごとに一時の DB を作り、Router を直に呼ぶ。書く接続・読む接続を直に使って、DB の決まり（足すだけ・外のキー・索引）も見る | `crates/server/tests/` の `health.rs`・`register.rs`・`actor.rs`・`resolve.rs`・`events.rs`・`links.rs`・`settings.rs`・`lineage.rs`・`search_actors.rs`・`search_rows.rs`（監査の検索）・`graph.rs`・`db.rs`・`concurrency.rs`・`no_ui.rs`（前の画面の道が無いこと）・`guard.rs`（守りの層）・`backup.rs`（backup と logs の道）・`migrations.rs`（出した移行の file が替わっていない・後の版が書いた DB を移行を当てずに断る）（道具は `crates/server/tests/common/mod.rs`・`common/sample.rs`） |
| 負荷 | 本物の binary を一時の DB で起こし、50 の客が 10 秒叩く（読む物の 2 割は監査の検索。ふだんは流さない）。CI では一時の DB を memory の上（`TMPDIR=/dev/shm`）に置く（CI の機械の disk の書き込みの詰まりを測らないため） | `crates/server/tests/load.rs`（CI の job `load`） |
| 単体 | DB に触らない純な関数 | 各 file の `#[cfg(test)]` |
| 終わり方と記録 | 本物の binary を、試験ごとの空の folder の DB で起こす。止める合図・合図なしの切断・DB を開けない、の後に起こし直して、前回の終わり方の行を見る。問い合わせの行が記録の file にだけ残ること・多く問い合わせた後の `logs/` の大きさが上限の内であること・読む道 `GET /api/logs` も見る | `crates/server/tests/runlog.rs`（書き手と印の単体は `crates/server/src/runlog.rs`） |
| image | 空の image の中で起き、断るべき物を断り、前の画面の道が 404 で、API で書いて読み返し、止めて起こし直した後も読めて、前回の終わり方を次の起動で出す | `ci/smoke-image.sh <image>`（CI の job `image` と release の `check`） |
| release の版 | tag と `Cargo.toml` の版・`CHANGELOG.md` の節の突き合わせを、通る形と落ちる形で打つ | `ci/test-check-release-version.sh`（CI の job `test`） |

## 待たせない書き方

並列に来る問い合わせを、互いに待たせない。

- handler はみな `async`。tokio の糸を塞ぐ呼び出し（`std::thread::sleep`・`std::fs` の読み書き・同期の `TcpStream::connect` など）は、`clippy.toml` の `disallowed-methods` で断る（`cargo clippy` が誤りにする）。代わりに `tokio::time`・`tokio::fs`・`tokio::net` を使う。
- 鍵を握ったままの `await`（`clippy::await_holding_lock`）も誤りにする。
- 試験の道具（`crates/server/tests/`）は同期で書くので、その file の頭で `disallowed_methods` の断りを外している。
- **`crates/server/src/` で断りを外すのは `crates/server/src/runlog.rs` だけ**（終わり方の印と、大きさで回す記録の file。[動かす](run.md) の「落ちた後に調べる」）。わざとの例外で、記録の 1 行を同期で書く。裏の糸に渡して書く形は、`panic = "abort"` では渡したままの行を落とすので、知りたい「落ちる直前の行」が残らない。1 行の write は短い。急がない物（生きている印・読む道）は `spawn_blocking` に出す。上限（`LOG_FILE_MAX`・`LOG_FILES`・`RUNS_MAX`）を変えたら、「落ちた後に調べる」の表も直す。log に出す新しい行は、英語の `名=値` の形で書く。
