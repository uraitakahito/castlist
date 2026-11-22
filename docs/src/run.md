# 動かす

Apple Container（`container` CLI）で、release の image を動かす。`127.0.0.1` だけで待ち受け、LAN にもインターネットにも開かない。

> [!WARNING]
> castlist の container を起こす・止める・消すのと、data の volume（`castlist-data`）を消すのは、**Product Manager だけ**がする。この章の命令は、Product Manager が自分の terminal で直に打つ。
>
> AI の担当は、この章の命令を打てない（repo の `.claude/roles/config.jsonc` の `infra.castlist` は `pm-only` で、`--name castlist`・`container start castlist`・`container stop castlist` などに当たる命令は、どの担当にも断られる）。担当は、打ってほしい 1 行を確認のページに書いて、`tpm ask pm` で Product Manager に送る。

| 物 | 値 |
|---|---|
| image | `ghcr.io/uraitakahito/castlist:v<version>`（[release](https://github.com/uraitakahito/castlist/releases) に digest がある。arm64） |
| container | `castlist`（`127.0.0.1:8802` → 中の 8080。メモリは Apple Container の下限の 200 MiB、CPU は 1） |
| URL | `http://127.0.0.1:8802/`（[画面](ui.md)。[API](api.md) は `/v1/`） |
| DB | 名前つきの volume `castlist-data`（中の `/data/castlist.db`）。Mac の folder は mount しない |
| 設定 | 環境変数 `CASTLIST_DB`（既定 `/data/castlist.db`）・`CASTLIST_LISTEN`（既定 `0.0.0.0:8080`）。image の既定のままでよい |

## 初めて起こす

```sh
container image pull ghcr.io/uraitakahito/castlist:v0.1.0
container image inspect ghcr.io/uraitakahito/castlist:v0.1.0 | grep -o '"digest" *: *"sha256:[0-9a-f]*"'   # release の digest がこの中にあるか
container volume create castlist-data
container run -d --name castlist -p 127.0.0.1:8802:8080 -v castlist-data:/data -m 200M -c 1 ghcr.io/uraitakahito/castlist:v0.1.0
```

- digest の見方: `container image list` に出る digest は、Apple Container が付ける別の値。release の digest は、`container image inspect` の `variants` の中の `digest` に出る。
- `container volume create` は初めての時だけ。2 度目からは、volume はもうある。
- 起きる時に、まだ当てていない移行の SQL（`migrations/`）を DB に当てる。

> [!WARNING]
> `-p` には必ず `127.0.0.1:` を書く。`-p 8802:8080` と書くと LAN に開く。

## 止める・起こし直す

```sh
container stop castlist
container start castlist
```

- 止めている間、castlist は答えない（担当の登録と出来事は送れない）。DB は volume に残る。
- Mac を起こし直すと止まったままになる。`container system start` の後に `container start castlist` を打ち、「確かめる」をする。

## 確かめる

1. `container list` で `castlist` が `running`。
2. `curl -s http://127.0.0.1:8802/healthz` が `{"status":"ok","version":"<version>","now":"…"}` を返す（`version` が起こした版）。
3. `lsof -nP -iTCP:8802 -sTCP:LISTEN` が `127.0.0.1:8802` だけを出す（`*:` で始まる行があれば LAN に開いている）。
4. `container logs castlist` の頭に `castlist <version> listen=0.0.0.0:8080 db=/data/castlist.db` が出ている。
5. browser で `http://127.0.0.1:8802/` を開き、一覧が出る。

## DB の写しを取る

**版を上げる前と、volume に触る前**に取る。動かしたまま取れる（[API](api.md) の `GET /v1/backup`）。

```sh
curl -fsS -o castlist-backup.db http://127.0.0.1:8802/v1/backup
sqlite3 castlist-backup.db 'select count(*) from actors'    # 開けて、件数が合うか
```

- 写しを取り直す時に前の物を上書きしないように、名に日時を入れてもよい（`castlist-$(date +%Y%m%d-%H%M).db`）。
- 写しには台帳のすべて（アクター・付随する id・出来事・線・model と effort）が入る。この repo にも CI にも置かない（`.gitignore` と `.dockerignore` が `*.db` を外す）。

## 版を上げる

1. [CHANGELOG.md](https://github.com/uraitakahito/castlist/blob/main/CHANGELOG.md) で、API と移行の SQL が変わるかを見る（移行が足された版は、当てた後に前の版へ戻せない）。
2. 新しい版の image を pull し、digest を release の値と見比べる（「初めて起こす」の初めの 2 行の版を替えて打つ）。
3. 「DB の写しを取る」をする。
4. 止めて、container を消し、新しい版で起こす。volume はそのまま付くので、data は残る。

   ```sh
   container stop castlist
   container rm castlist
   container run -d --name castlist -p 127.0.0.1:8802:8080 -v castlist-data:/data -m 200M -c 1 ghcr.io/uraitakahito/castlist:v<新しい版>
   ```

5. 「確かめる」をする。駄目なら、`container stop castlist`・`container rm castlist` の後、前の版で `container run` を打つ（移行が足されていなければ、同じ volume で起きる。足されていたら、3 の写しから戻す）。
6. 古い image は `container image delete` で消す。

## 写しから戻す

volume を失った・DB が壊れた時。戻す間、castlist は答えない。

```sh
container stop castlist
container run --rm -v castlist-data:/data -v "<写しの folder>:/in:ro" docker.io/library/rust:1.100.0-alpine sh -c 'rm -f /data/castlist.db-wal /data/castlist.db-shm && cp /in/<写しの file> /data/castlist.db'
container start castlist
```

- 動かす image は空で `cp` も無いので、file を volume に入れる時だけ、shell のある image を借りる（上は `Containerfile` の build の段と同じ物。何でもよい）。
- 起こしたら「確かめる」をする。

## volume を消す

> [!CAUTION]
> `container volume delete castlist-data` は、**台帳（アクター・出来事・線・model と effort のすべて）を消す**。足すだけの記録なので、ほかに控えは無い。戻せるのは、前に取った写しからだけ。
>
> volume を消すのは Product Manager だけで、消す前に「DB の写しを取る」をする。AI の担当は消さない（`pm-only` で断られる）。
