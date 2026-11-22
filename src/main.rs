use castlist::db::Db;
use castlist::router;
use tokio::signal::unix::{SignalKind, signal};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let listen = std::env::var("CASTLIST_LISTEN").unwrap_or_else(|_| "0.0.0.0:8080".to_owned());
    let path = std::env::var("CASTLIST_DB").unwrap_or_else(|_| "/data/castlist.db".to_owned());
    let db = Db::open(std::path::Path::new(&path)).await?;
    let listener = tokio::net::TcpListener::bind(&listen).await?;
    println!(
        "castlist {} listen={listen} db={path}",
        env!("CARGO_PKG_VERSION")
    );

    // container の中では PID 1 になる。PID 1 は、受け口を作らないと止める合図を無視する。
    // 合図が来たら新しい問い合わせを断り、走っている物を終えてから閉じる。
    let mut term = signal(SignalKind::terminate())?;
    let mut int = signal(SignalKind::interrupt())?;
    axum::serve(listener, router(db.clone()))
        .with_graceful_shutdown(async move {
            tokio::select! {
                _ = term.recv() => {}
                _ = int.recv() => {}
            }
        })
        .await?;
    db.close().await;
    println!("castlist stopped");
    Ok(())
}
