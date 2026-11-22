//! 受け入れの試験の道具: binary を空いた port で起こし、生の HTTP/1.1 で叩く。
//! 道筋を直に呼ぶ道具は [`app`]。
#![allow(clippy::disallowed_methods, dead_code)]

pub mod app;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

/// 起こした castlist-server。落とす時に止める。
pub struct Server {
    child: Child,
    pub port: u16,
    /// 一時の DB。落とす時に消す。
    pub db: std::path::PathBuf,
}

/// 1 つの答え。
#[derive(Debug)]
pub struct Answer {
    pub status: u16,
    pub head: String,
    pub body: String,
}

impl Answer {
    /// header の値（名は小文字で引く）。
    pub fn header(&self, name: &str) -> Option<String> {
        self.head.lines().skip(1).find_map(|line| {
            let (key, value) = line.split_once(':')?;
            (key.trim().eq_ignore_ascii_case(name)).then(|| value.trim().to_owned())
        })
    }
}

impl Server {
    /// 空いた port で起こし、受けられるようになるまで待つ。
    pub fn start() -> Self {
        let port = TcpListener::bind("127.0.0.1:0")
            .and_then(|l| l.local_addr())
            .expect("空いた port")
            .port();
        let db = app::temp_db("server");
        let mut child = Command::new(env!("CARGO_BIN_EXE_castlist-server"))
            .env("CASTLIST_LISTEN", format!("127.0.0.1:{port}"))
            .env("CASTLIST_DB", &db)
            .stdout(Stdio::null())
            .spawn()
            .expect("castlist-server を起こす");
        let started = Instant::now();
        while TcpStream::connect(("127.0.0.1", port)).is_err() {
            if let Some(status) = child.try_wait().expect("try_wait") {
                panic!("castlist-server が起きる前に終わった: {status}");
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "10 秒で受けられるようにならない"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        Self { child, port, db }
    }

    /// 生の要求を送り、答えを読む（要求は Connection: close にする）。
    pub fn send(&self, raw: &str) -> Answer {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).expect("connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .expect("timeout");
        stream.write_all(raw.as_bytes()).expect("write");
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).expect("read");
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
        let status = head
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        Answer {
            status,
            head: head.to_owned(),
            body: body.to_owned(),
        }
    }

    pub fn get(&self, path: &str) -> Answer {
        self.send(&format!(
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
        ))
    }

    /// 止める合図（SIGTERM）を送る。
    pub fn terminate(&mut self) {
        let status = Command::new("kill")
            .args(["-TERM", &self.child.id().to_string()])
            .status()
            .expect("kill");
        assert!(status.success());
    }

    pub fn try_wait(&mut self) -> Option<ExitStatus> {
        self.child.try_wait().expect("try_wait")
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        app::remove_db(&self.db);
    }
}
