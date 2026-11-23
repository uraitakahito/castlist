# castlist の image。build の段で静的な binary を作り、動かす段は空の image に binary 1 つだけを置く。
# build の段の Rust の版は rust-toolchain.toml と合わせる（合わないと、build の中で別の版を取りに行く）。
FROM docker.io/library/rust:1.100.0-alpine@sha256:8d0e82abfdf9a38071e07cabc5afba4ddbe077631a80e4e742a760c38beadc33 AS build
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY . .
# 作るのは今の server だけ（新しい server の crates/server は、切り替えまで image に入らない）。
RUN cargo build --release --locked -p castlist && mkdir /data

FROM scratch
COPY --from=build /src/target/release/castlist-server /castlist-server
# DB の置き場。動かす時に volume を付ける。
COPY --from=build /data /data
ENV CASTLIST_DB=/data/castlist.db CASTLIST_LISTEN=0.0.0.0:8080
EXPOSE 8080
ENTRYPOINT ["/castlist-server"]
