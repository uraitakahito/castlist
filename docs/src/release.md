# release を出す

image は public で、だれでも取って中を見られる。**出すのは release の workflow（`.github/workflows/release.yml`）だけ**で、手元から `push` しない（手元の build には、commit していない file と、自分の home の path が入りうる）。castlist に CLI は無いので、release に付ける file も無い。

1. `Cargo.toml` の `[package]` の `version` を上げ、`CHANGELOG.md` に `## <version>` の節を足す PR を merge する。
2. main のその commit に tag を打って push する: `git tag v<version> && git push origin v<version>`。
3. workflow の `check`（書き込みの権限が無い）が、tag が `Cargo.toml` の初めの `version = "…"` と合い CHANGELOG.md にその節があるか（`ci/check-release-version.sh`）・tag が main の上にあるか・image の中身の一覧・設定・秘密の走査（下の「image に入る物」）・動くか（`ci/smoke-image.sh`）を見る。**1 つでも落ちれば、`push` は動かず、image は出ない。**
4. `push`（`packages: write` と、release を書くための `contents: write`）が、確かめたその image（作り直さない）を `ghcr.io/uraitakahito/castlist:v<version>` に置き、release に digest と中身の一覧を書く。同じ tag がもう ghcr.io にあれば置かない。
5. release の中身の一覧が「`castlist-server` と空の `data` だけ」であることを見る（この時、image はもう public。違っていたら、すぐ下の「機微な物が出てしまった時」をする）。
6. 同じ tag の push で、docs の workflow（`.github/workflows/docs.yml`）が本を GitHub Pages（https://uraitakahito.github.io/castlist/）に出す。出たのを見る。
7. Product Manager に、[動かす](run.md) の 1 行（初めてなら「初めて起こす」、2 度目からは「版を上げる」）を送る。castlist を起こすのは Product Manager だけ。

- 出した tag は打ち直さない（digest が変わり、書き留めた値と合わなくなる）。直す時は、次の版を出す。
- **機微な物が出てしまった時**: GitHub の package の画面で、その版を消す。秘密なら、すぐ替える（消しても、pull した人の手元には残る前提で動く）。直して次の版を出し、`ci/` の決まりに、同じ物を止める形を足す。
- **置いた時から public**: この repo は public で、image に repo を指す label を付けて置くので、package は初めの release から、だれでも pull できる（pagetray の v0.2.0 で確かめた。private の間に人が見る、という段は無い）。人が見られるのは、tag を打つ前だけ。だから、3 の確かめを push の前に置いてあり、決まりを緩める PR（`ci/` の file・`.dockerignore`・`Containerfile`）は、よく見てから merge する。

## image に入る物

image は外に出す物なので、入ってよい物を決めてあり、CI（job `image` と release の `check`。どちらも `ci/check-image.sh`）が毎回確かめる。1 つでも違えば落ちる。

| 見る物 | 決まり | 書いてある所 |
|---|---|---|
| build の中に入る物 | 名指した source だけ（履歴・CI の設定・試験・手元の file は入れない） | `.dockerignore` |
| image の file | binary 1 つ（`castlist-server`）と、空の `/data` だけ。layer は 2 枚 | `ci/image-contents.txt` |
| 設定 | 環境変数は DB の path と待ち受けの 2 つだけ。label は `org.opencontainers.image.source`・`version`・`revision` だけ。開ける port は 8080 だけ | `ci/check-image.sh` の頭 |
| binary の中の字 | home の path・mail の形・秘密の鍵や token の頭が無い。gitleaks も掛ける（source にも） | `ci/scan-binary.sh`・`ci/strings-deny.txt`・`ci/strings-allow.txt` |
| 動くか | 空の image の中で起き、外の Host と browser からの書き込みを断り、画面（相関図も）と CSS と script が CSP の決まりどおりに出て、止めて起こし直せる | `ci/smoke-image.sh` |

- binary には、画面の template・CSS・script と、取り込んだ cytoscape（MIT。`static/vendor/`）が埋め込まれる。cytoscape は npm の物と同じかを `ci/check-vendor.sh` が見る。
- build に秘密は要らない。`Containerfile` に `ARG` を書かず、`--build-arg`・`--secret` を使わない。label は、release の workflow が `--label` で付ける 3 つだけ。
- 出どころの記録（provenance・SBOM）は付けない。付ける時は、中身を見てから。
- release の workflow は、確かめる job（書き込みの権限が無い）と、置く job（`packages: write`・`contents: write`）に分かれている。置く job は、確かめた image の file を受け取り、checksum を見てから、そのまま置く。
- 本物の DB やその写しを、この repo にも CI にも置かない（`.gitignore` と `.dockerignore` が `*.db` を外す）。
- 決まりを変える時（file を足す・通す字を足す）は、PR で見える形で上の file を直す。
