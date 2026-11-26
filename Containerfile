# castlist の image。build の段で静的な binary を作り、動かす段は空の image に binary 1 つだけを置く。
# server は crates/server（package castlist・binary castlist-server）。image の中の path は /castlist-server。
# 0.4.x の DB を移す migrate は 1.0.0 の image にだけある（docs/src/release.md の「切り替え」）。
# build の段の Rust の版は rust-toolchain.toml と合わせる（合わないと、build の中で別の版を取りに行く）。
FROM docker.io/library/rust:1.100.0-alpine@sha256:8d0e82abfdf9a38071e07cabc5afba4ddbe077631a80e4e742a760c38beadc33 AS build
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p castlist && mkdir /data

FROM scratch
COPY --from=build /src/target/release/castlist-server /castlist-server
# DB の置き場。動かす時に volume を付ける。
COPY --from=build /data /data
ENV CASTLIST_DB=/data/castlist.db CASTLIST_LISTEN=0.0.0.0:8080
EXPOSE 8080
ENTRYPOINT ["/castlist-server"]
