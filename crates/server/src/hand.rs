//! 手の道（Smithy の定義の外。backup・logs）。 0.4.x の server の `GET /v1/backup`・`GET /v1/logs` と同じ振る舞い。
//! - `GET /api/backup`: 動かしたまま、DB の揃ったバックアップを 1 つの SQLite の file で返す
//!   （`curl -fsS -o castlist-backup.db http://127.0.0.1:8802/api/backup`）。
//! - `GET /api/logs`: 残した記録（終わり方の印と、時刻つきの行）を、古い順の字で返す。記録の置き場がある時だけ。
//!
//! 誤りは、生成した層と同じ restJson1 の形（`Unavailable`・`InternalError`。[`crate::errors::reply`]）。
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use tokio_util::io::ReaderStream;

use crate::db::Db;
use crate::errors::Failure;
use crate::runlog::RunLog;

/// backup と、記録の置き場があれば logs の道。
pub fn routes(db: Arc<Db>, runlog: Option<RunLog>) -> Router {
    let routes = Router::new()
        .route("/api/backup", get(backup))
        .with_state(db);
    match runlog {
        Some(runlog) => routes.route("/api/logs", get(move || logs(runlog.clone()))),
        None => routes,
    }
}

/// バックアップを DB の隣の一時の file に作って開き、名を消してから送る（開いた後は名が要らない。消しても、読み終えるまで
/// 中身は残る）。時間切れ（守りの層）は送り始めるまでを数え、送る間は数えない（大きな DB でも切らない）。
async fn backup(State(db): State<Arc<Db>>) -> Response {
    let path = match db.backup().await {
        Ok(path) => path,
        Err(e) => return Failure::db("backup", e).reply(),
    };
    let file = tokio::fs::File::open(&path).await;
    let _ = tokio::fs::remove_file(&path).await;
    match file {
        Ok(file) => (
            [
                (header::CONTENT_TYPE, "application/vnd.sqlite3"),
                (
                    header::CONTENT_DISPOSITION,
                    "attachment; filename=\"castlist.db\"",
                ),
            ],
            Body::from_stream(ReaderStream::new(file)),
        )
            .into_response(),
        Err(e) => Failure::data("backup", &e).reply(),
    }
}

/// 印と記録を読む（塞ぐ読み方なので、tokio の糸の外で）。多くて 4 MiB と 64 KiB。
async fn logs(runlog: RunLog) -> Response {
    match tokio::task::spawn_blocking(move || runlog.read_all()).await {
        Ok(text) => ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], text).into_response(),
        Err(e) => Failure::data("logs", &e).reply(),
    }
}
