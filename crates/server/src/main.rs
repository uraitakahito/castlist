//! 新しい castlist の server を起こす binary（今の server の src/main.rs と同じ振る舞い）。切り替えまで image には入らない。
//! - DB の path は `CASTLIST_DB`（既定 `/data/castlist.db`）、待ち受ける所は `CASTLIST_LISTEN`（既定 `0.0.0.0:8080`）。
//! - 記録の置き場は DB の隣の `logs/`（[`castlist_next::runlog`]）。起きたら初めに、前の回の終わり方の行（`previous: …`）を出す。
//! - 止める合図（SIGTERM・SIGINT）で新しい問い合わせを断り、走っている物を終えてから閉じる。
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;

use castlist_next::db::Db;
use castlist_next::runlog::{self, RunLog};
use castlist_next::{Config, REQUEST_TIMEOUT, app_with};
use tokio::signal::unix::{SignalKind, signal};

#[tokio::main]
async fn main() -> ExitCode {
    let path = std::env::var("CASTLIST_DB").unwrap_or_else(|_| "/data/castlist.db".to_owned());
    let path = Path::new(&path);
    // 記録の置き場は DB の隣の logs/。DB を開く前に用意する（開けなかった訳も残すため）。
    let log = RunLog::beside(path);
    runlog::init(&log);
    let previous = log.previous();
    log.start(env!("CARGO_PKG_VERSION"));
    log.hook_panics();
    tracing::info!("{previous}");

    match serve(path, &log).await {
        Ok(()) => {
            log.stopped();
            tracing::info!("castlist stopped");
            ExitCode::SUCCESS
        }
        Err(e) => {
            // 誤りで終わった印を残す。残さないと、次の起動で SIGKILL と見分けられない。
            log.failed(&e.to_string());
            tracing::error!("castlist: {e}");
            ExitCode::FAILURE
        }
    }
}

/// 起きて、止める合図まで受け続ける。
async fn serve(path: &Path, log: &RunLog) -> Result<(), Box<dyn std::error::Error>> {
    let listen = std::env::var("CASTLIST_LISTEN").unwrap_or_else(|_| "0.0.0.0:8080".to_owned());
    // 開けない時（移行に落ちた・外のキーの崩れた行がある）は、訳を出して起きない。
    let db = Arc::new(Db::open(path).await?);
    let listener = tokio::net::TcpListener::bind(&listen).await?;
    tracing::info!(
        "castlist {} listen={listen} db={}",
        env!("CARGO_PKG_VERSION"),
        path.display()
    );
    tokio::spawn(runlog::keep_alive(log.clone()));

    // container の中では PID 1 になる。PID 1 は、受け口を作らないと止める合図を無視する。
    // 合図が来たら新しい問い合わせを断り、走っている物を終えてから閉じる。
    let mut term = signal(SignalKind::terminate())?;
    let mut int = signal(SignalKind::interrupt())?;
    let signalled = log.clone();
    let config = Config {
        timeout: REQUEST_TIMEOUT,
        runlog: Some(log.clone()),
    };
    axum::serve(listener, app_with(db.clone(), config))
        .with_graceful_shutdown(async move {
            let name = tokio::select! {
                _ = term.recv() => "SIGTERM",
                _ = int.recv() => "SIGINT",
            };
            // 合図を受けた時に 1 行。走っている物を待つ間に切られても、合図を受けたことは残る。
            // 閉じ終えた印（stopped）は、呼んだ側が書く。
            signalled.signal(name);
            tracing::info!("signal {name}");
        })
        .await?;
    db.close().await;
    Ok(())
}
