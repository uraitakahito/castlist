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
- **どの tag も新しい server（1.0.0 から。AI-36）**: image は `Containerfile`（新しい server の `castlist-next` を `/castlist-server` の名で置く）で作り、`ci/smoke-image.sh <image> api` で確かめる。smoke は、新しい API の形で書いて読み返すほかに、下の「切り替え」の 2・3 の予行も流す: ghcr.io から今の版の image（v0.4.0。`ci/smoke-image.sh` の `old_image` に digest で固める）を取り、今の API でアクター・ラベル・線・model と effort・出来事を置いて写しを取り、確かめる image の migrate で新しい volume に移し、照らしの行がみな ok で、新しい API で同じ物が読めるかを見る。だから release の確かめは、ghcr.io の v0.4.0 の image に頼る（下の「機微な物が出てしまった時」で v0.4.0 を消すなら、先に予行を替える PR を出す）。
- **今の server（0.4.x）は、もう出さない**: 1.0.0 から `Containerfile` は新しい server を作るので、今の server を直して出す道は無い。直しが要れば、新しい server で直して次の版を出す。戻す時に使う今の版は v0.4.0 のままなので、smoke の予行の `old_image` も替えない。今の server の code（`src/` など）と `Containerfile.next` は段 9 で消す。
- **切り替え（0.4.x から 1.0.0 に上げる時。AI-36 の段 8）**: 1.0.0 の image の中の binary も `/castlist-server` で、環境変数と port も同じなので、[動かす](run.md) の命令で 0.4.x と替わるのは image の版と、**volume** だけ。
  1. **volume は新しい物（`castlist-data-v1`）にする。新しい server を、今の volume（`castlist-data`）で起こさない。** 今の DB は新しい server の形でない（新しい server は、今の server の移行を当てた DB を、何も書かずに断って止まる）。今の volume は、戻す時のために、そのまま残す。
  2. 先にデータを移す: 今の server の `GET /v1/backup` で写しを取り（止める直前に）、今の container を止めて消し、新しい volume を作ってから（`container volume create castlist-data-v1`）、新しい volume と写しの folder を付けて、1.0.0 の image に `migrate --from <写し> --to /data/castlist.db` を付けて起こす（例 `container run --rm -v castlist-data-v1:/data -v "<写しの folder>:/in:ro" ghcr.io/uraitakahito/castlist:v1.0.0 migrate --from /in/<写しの file> --to /data/castlist.db`）。写しは `-wal`・`-shm` の無い 1 つの file で渡す。表ごとの突き合わせ（[新しい API の形](next.md) の「データ」）が 1 つでも合わなければ、何も足さずに 1 で終わる。**1 で終わったら 3 に進まない。** 出た行（表ごとの `… ok` と、終わりの `移し終えた: …`）は残す。
  3. 移し終えてから、新しい volume で 1.0.0 を起こす（[動かす](run.md) の「初めて起こす」の `container run` の行）。
  - 戻す時は、1.0.0 を止めて消し、v0.4.0 の image と今の volume（`castlist-data`）で起こす。切り替えから戻すまでに 1.0.0 に書いた物は、今の volume に入らない。
- **機微な物が出てしまった時**: GitHub の package の画面で、その版を消す。秘密なら、すぐ替える（消しても、pull した人の手元には残る前提で動く）。直して次の版を出し、`ci/` の決まりに、同じ物を止める形を足す。
- **置いた時から public**: この repo は public で、image に repo を指す label を付けて置くので、package は初めの release から、だれでも pull できる（pagetray の v0.2.0 で確かめた。private の間に人が見る、という段は無い）。人が見られるのは、tag を打つ前だけ。だから、3 の確かめを push の前に置いてあり、決まりを緩める PR（`ci/` の file・`.dockerignore`・`Containerfile`）は、よく見てから merge する。

## image に入る物

image は外に出す物なので、入ってよい物を決めてあり、CI（job `image` と release の `check`。どちらも `ci/check-image.sh`）が毎回確かめる。1 つでも違えば落ちる。`Containerfile.next`（1.0.0 から `Containerfile` と同じ中身。段 9 で消す）も同じ決まりで、CI の job `image-next` が確かめる。

| 見る物 | 決まり | 書いてある所 |
|---|---|---|
| build の中に入る物 | 名指した source だけ（履歴・CI の設定・試験・手元の file は入れない） | `.dockerignore` |
| image の file | binary 1 つ（`castlist-server`）と、空の `/data` だけ。layer は 2 枚 | `ci/image-contents.txt` |
| 設定 | 環境変数は DB の path と待ち受けの 2 つだけ。label は `org.opencontainers.image.source`・`version`・`revision` だけ。開ける port は 8080 だけ | `ci/check-image.sh` の頭 |
| binary の中の字 | home の path・mail の形・秘密の鍵や token の頭が無い。gitleaks も掛ける（source にも） | `ci/scan-binary.sh`・`ci/strings-deny.txt`・`ci/strings-allow.txt` |
| 動くか | 空の image の中で起き、外の Host と browser からの書き込みを断り（断る答えは restJson1 の形）、画面（相関図も）と CSS と script が CSP の決まりどおりに出て、新しい API（`/api`）で登録と出来事を書いて読み返し、止めて起こし直した後も書いた物が読める。終わりに、今の版（v0.4.0）の写しを移す予行（上の「切り替え」の 2・3）をする | `ci/smoke-image.sh`（`api` を付ける） |

- binary には、画面の template・CSS・script と、取り込んだ cytoscape（MIT。`static/vendor/`）が埋め込まれる。cytoscape は npm の物と同じかを `ci/check-vendor.sh` が見る。
- build に秘密は要らない。`Containerfile` に `ARG` を書かず、`--build-arg`・`--secret` を使わない。label は、release の workflow が `--label` で付ける 3 つだけ。
- 出どころの記録（provenance・SBOM）は付けない。付ける時は、中身を見てから。
- release の workflow は、確かめる job（書き込みの権限が無い）と、置く job（`packages: write`・`contents: write`）に分かれている。置く job は、確かめた image の file を受け取り、checksum を見てから、そのまま置く。
- 本物の DB やそのバックアップを、この repo にも CI にも置かない（`.gitignore` と `.dockerignore` が `*.db` を外す）。
- 決まりを変える時（file を足す・通す字を足す）は、PR で見える形で上の file を直す。
