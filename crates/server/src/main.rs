//! 新しい castlist の server を起こす最小の binary（e2e/ の Playwright の試験を、新しい server にも向けるため）。
//! DB の path は `CASTLIST_DB`、待ち受ける所は `CASTLIST_LISTEN`（今の server の binary と同じ名と既定）。
//! 記録の file・止める合図での閉じ方・時間切れは、守りの層の PR（AI-36 の段 2）で足す。切り替えまで image には入らない。
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;

use castlist_next::db::Db;

#[tokio::main]
async fn main() -> ExitCode {
    match serve().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("castlist-next: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn serve() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::var("CASTLIST_DB").unwrap_or_else(|_| "/data/castlist.db".to_owned());
    let listen = std::env::var("CASTLIST_LISTEN").unwrap_or_else(|_| "0.0.0.0:8080".to_owned());
    // 開けない時（移行に落ちた・外のキーの崩れた行がある）は、訳を出して起きない。
    let db = Arc::new(Db::open(Path::new(&path)).await?);
    let listener = tokio::net::TcpListener::bind(&listen).await?;
    eprintln!(
        "castlist-next {} listen={listen} db={path}",
        env!("CARGO_PKG_VERSION")
    );
    axum::serve(listener, castlist_next::app(db)).await?;
    Ok(())
}
