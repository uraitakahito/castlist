# release を出す

image は public で、だれでも取って中を見られる。**出すのは release の workflow（`.github/workflows/release.yml`）だけ**で、手元から `push` しない（手元の build には、commit していない file と、自分の home の path が入りうる）。castlist に CLI は無いので、release に付ける file も無い。

1. `Cargo.toml` の `[workspace.package]` の `version` を上げ、`CHANGELOG.md` に `## <version>` の節を足す PR を merge する。
2. main のその commit に tag を打って push する: `git tag v<version> && git push origin v<version>`。
3. workflow の `check`（書き込みの権限が無い）が、tag が `Cargo.toml` の初めの `version = "…"` と合い CHANGELOG.md にその節があるか（`ci/check-release-version.sh`）・tag が main の上にあるか・image の中身の一覧・設定・秘密の走査（下の「image に入る物」）・動くか（`ci/smoke-image.sh`）を見る。**1 つでも落ちれば、`push` は動かず、image は出ない。**
4. `push`（`packages: write` と、release を書くための `contents: write`）が、確かめたその image（作り直さない）を `ghcr.io/uraitakahito/castlist:v<version>` に置き、release に digest と中身の一覧を書く。同じ tag がもう ghcr.io にあれば置かない。
5. release の中身の一覧が「`castlist-server` と空の `data` だけ」であることを見る（この時、image はもう public。違っていたら、すぐ下の「機微な物が出てしまった時」をする）。
6. 同じ tag の push で、docs の workflow（`.github/workflows/docs.yml`）が本を GitHub Pages（https://uraitakahito.github.io/castlist/）に出す。出たのを見る。
7. roster で、`cli/src/castlist.ts` の `CASTLIST_VERSION` を新しい版にする PR を出す。roster は、castlist が止まっていて container も無い時に、この版で `container run` の 1 行を出すので、上げ忘れると古い版を起こす 1 行が出る（roster の docs の「開発」の「castlist を上げる時」）。
8. Product Manager に、[動かす](run.md) の 1 行（初めてなら「初めて起こす」、2 度目からは「版を上げる」）を送る。castlist を起こすのは Product Manager だけ。

- 出した tag は打ち直さない（digest が変わり、書き留めた値と合わなくなる）。直す時は、次の版を出す。
- **rc**: tag に `-rc.` の付く物（`v<版>-rc.<n>`。`Cargo.toml` の版も `CHANGELOG.md` の節も `<版>-rc.<n>`）は、GitHub の pre-release にする。作り方と確かめは、ほかの tag と同じ。
- image は `Containerfile` で作る（server の crate `crates/server`（package `castlist`）の binary `castlist-server` を `/castlist-server` に置く）。確かめは `ci/smoke-image.sh <image>`。
- **0.4.x は、もう出さない**: 0.4.x の server の code は消したので、直して出す道は無い。直しが要れば、`crates/server` で直して次の版を出す。
- **機微な物が出てしまった時**: GitHub の package の画面で、その版を消す。秘密なら、すぐ替える（消しても、pull した人の手元には残る前提で動く）。直して次の版を出し、`ci/` の決まりに、同じ物を止める形を足す。
- **置いた時から public**: この repo は public で、image に repo を指す label を付けて置くので、package は初めの release から、だれでも pull できる（pagetray の v0.2.0 で確かめた。private の間に人が見る、という段は無い）。人が見られるのは、tag を打つ前だけ。だから、3 の確かめを push の前に置いてあり、決まりを緩める PR（`ci/` の file・`.dockerignore`・`Containerfile`）は、よく見てから merge する。

## 切り替え（済み）

1.0.0 で、0.4.x の server から、Smithy の定義から作る server（`crates/server`。API も DB の移行の SQL もみな替わった）に切り替えた（AI-36）。

- 0.4.x の server の `GET /v1/backup` で写しを取り、v1.0.0 の image の `migrate` で新しい volume `castlist-data-v1` に移し、その volume で 1.0.0 を起こした。`migrate` は、写した後に表ごとの行の数・中身の digest・seq の続き・外のキー・id の形を照らし、1 つでも合わなければ何も足さずに 1 で終わる。
- 切り替えの前の volume `castlist-data`（0.4.x の DB）と v0.4.0 の image は、戻す時のために、Product Manager が決める日まで残す。1.0.0 から後の server は、その DB を何も書かずに断る（[API](api.md) の「データ」）ので、`castlist-data` で 1.0.0 から後の版を起こさない。

戻す時（要る時だけ）:

1. 1.0.0 を止めて消し、v0.4.0 の image と `castlist-data` で起こす（[動かす](run.md) の起こす行の、image を `ghcr.io/uraitakahito/castlist:v0.4.0` に、`-v` を `castlist-data:/data` に替えた物）。切り替えから戻すまでに 1.0.0 に書いた物は、`castlist-data` に入らない。
2. 送り手も戻す: roster の castlist の client（`/api` の道）と、`CASTLIST_VERSION` を 0.4.x の物に戻さないと、roster は 0.4.x に書けない（1.0.0 の roster は、0.x の castlist を版で見て止まる）。
3. pagetray も組で戻す（v0.8.0 と `pagetray-data`。手順は pagetray の docs の「[release を出す](https://uraitakahito.github.io/pagetray/release.html)」の「切り替え」）。1.0.0 の pagetray は送り手を castlist の `POST /api/resolve` で聞くので、0.4.x の castlist（`/v1/resolve` だけ）には聞けず、送り手を付けた置く要求がみな断られる。逆に、0.8.x の pagetray は 1.0.0 の castlist に聞けない。

戻した後に、また切り替える時（castlist を先に 1.0.0 にしてから pagetray。切り替えの時と同じ順）:

1. 0.4.x の `GET /v1/backup` で写しを取り（止める直前に）、0.4.x を止めて消す。
2. 空の新しい volume を作る。`castlist-data-v1` には 1.0.0 の書いた行があり、`migrate` は台帳の行のある先を断るので、そこへは移せない。名は `castlist-data-v1-2`（3 度目からは番号を上げる）にする。`castlist-data` で始まる名は、Product Manager だけが触れる守りに当たる。
3. 移すのは **v1.0.0 の image の `migrate`** だけ（後の版には `migrate` が無い）。版の tag でなく digest で指す: `ghcr.io/uraitakahito/castlist@sha256:ae9ebbe8371f4f3abd5d4d8121c15aa43ce57d06e0466e61f9e8006c2a20b05d`。新しい volume を `/data` に、写しの folder を `/in:ro` に付けて、`migrate --from /in/<写しの file> --to /data/castlist.db` を付けて起こす。写しは `-wal`・`-shm` の無い 1 つの file で渡す。照らしが 1 つでも合わなければ、何も足さずに 1 で終わる。**1 で終わったら 4 に進まない。** 出た行（表ごとの `… ok` と、終わりの `移し終えた: …`）は残す。
4. 移し終えてから、その volume で今の版を起こす（[動かす](run.md) の「初めて起こす」の `container run` の行の、volume の名を替えた物）。
5. 動く volume の名が替わったので、[動かす](run.md) の起こす・リストア・記録を読む行と、roster が Product Manager に出す起こす 1 行（roster の `cli/src/castlist.ts` の `CASTLIST_VOLUME`）を新しい名に替える（roster にも PR を出す）。替えないと、その行のとおりに打った時に、古い `castlist-data-v1` で起こす。

## image に入る物

image は外に出す物なので、入ってよい物を決めてあり、CI（job `image` と release の `check`。どちらも `ci/check-image.sh`）が毎回確かめる。1 つでも違えば落ちる。

| 見る物 | 決まり | 書いてある所 |
|---|---|---|
| build の中に入る物 | 名指した source だけ（履歴・CI の設定・試験・手元の file は入れない） | `.dockerignore` |
| image の file | binary 1 つ（`castlist-server`）と、空の `/data` だけ。layer は 2 枚 | `ci/image-contents.txt` |
| 設定 | 環境変数は DB の path と待ち受けの 2 つだけ。label は `org.opencontainers.image.source`・`version`・`revision` だけ。開ける port は 8080 だけ | `ci/check-image.sh` の頭 |
| binary の中の字 | home の path・mail の形・秘密の鍵や token の頭が無い。gitleaks も掛ける（source にも） | `ci/scan-binary.sh`・`ci/strings-deny.txt`・`ci/strings-allow.txt` |
| 動くか | 空の image の中で起き、外の Host と browser からの書き込みを断り（断る答えは restJson1 の形）、画面（相関図も）と CSS と script が CSP の決まりどおりに出て、API（`/api`）で登録と出来事を書いて読み返し、止めて起こし直した後も書いた物が読める | `ci/smoke-image.sh <image>` |

- binary には、画面の template・CSS・script と、取り込んだ cytoscape（MIT。`static/vendor/`）が埋め込まれる。cytoscape は npm の物と同じかを `ci/check-vendor.sh` が見る。
- build に秘密は要らない。`Containerfile` に `ARG` を書かず、`--build-arg`・`--secret` を使わない。label は、release の workflow が `--label` で付ける 3 つだけ。
- 出どころの記録（provenance・SBOM）は付けない。付ける時は、中身を見てから。
- release の workflow は、確かめる job（書き込みの権限が無い）と、置く job（`packages: write`・`contents: write`）に分かれている。置く job は、確かめた image の file を受け取り、checksum を見てから、そのまま置く。
- 本物の DB やそのバックアップを、この repo にも CI にも置かない（`.gitignore` と `.dockerignore` が `*.db` を外す）。
- 決まりを変える時（file を足す・通す字を足す）は、PR で見える形で上の file を直す。
