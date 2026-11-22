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
    /// 一時の DB。落とす時に、folder ごと消す（`keep` なら残す）。
    pub db: std::path::PathBuf,
    /// 起こし直して前回の終わり方を見る試験は、DB の folder を残す。
    keep: bool,
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
        let mut server = Self::start_at(&app::temp_db("server"));
        server.keep = false;
        server
    }

    /// この DB で起こす（同じ folder で起こし直せる）。落としても folder は消さない。stdout は DB の隣の file に受ける。
    pub fn start_at(db: &std::path::Path) -> Self {
        // 空いた port は「取って、放して、渡す」ので、並べて流れる別の試験の server に先に取られることがある。
        // つながるかでは見分けられない（つながった先が別の server かもしれない）。自分の stdout に、待ち受けた後の
        // 行（listen=）が出るまで待ち、待ち受けられずに終わったら、印を戻して別の port でやり直す。
        let db = db.to_owned();
        let runs = db.with_file_name("logs").join("runs.log");
        let before = std::fs::read(&runs).ok();
        for _ in 0..20 {
            let port = TcpListener::bind("127.0.0.1:0")
                .and_then(|l| l.local_addr())
                .expect("空いた port")
                .port();
            let out = db.with_file_name(format!("stdout-{port}.txt"));
            let stdout = std::fs::File::create(&out).expect("stdout の file");
            let mut child = Command::new(env!("CARGO_BIN_EXE_castlist-server"))
                .env("CASTLIST_LISTEN", format!("127.0.0.1:{port}"))
                .env("CASTLIST_DB", &db)
                .stdout(Stdio::from(stdout))
                .spawn()
                .expect("castlist-server を起こす");
            let started = Instant::now();
            let listening = loop {
                let text = std::fs::read_to_string(&out).unwrap_or_default();
                if text.contains(&format!(" listen=127.0.0.1:{port} ")) {
                    break true;
                }
                if child.try_wait().expect("try_wait").is_some() {
                    break false;
                }
                assert!(
                    started.elapsed() < Duration::from_secs(10),
                    "10 秒で受けられるようにならない: {text}"
                );
                std::thread::sleep(Duration::from_millis(20));
            };
            if listening {
                return Self {
                    child,
                    port,
                    db,
                    keep: true,
                };
            }
            // 取られた port で終わった回の印を消す（試験が見るのは、やり直しの前までの印）。
            match &before {
                Some(bytes) => std::fs::write(&runs, bytes).expect("印を戻す"),
                None => drop(std::fs::remove_file(&runs)),
            }
        }
        panic!("20 回やり直しても、空いた port で起きない");
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

    /// stdout に出た字。
    pub fn stdout(&self) -> String {
        std::fs::read_to_string(self.db.with_file_name(format!("stdout-{}.txt", self.port)))
            .unwrap_or_default()
    }

    /// 止める合図を送り、終わるのを待つ。終わりの code を返す。
    pub fn stop(&mut self) -> Option<i32> {
        self.terminate();
        self.child.wait().expect("wait").code()
    }

    /// 合図なしで切る（SIGKILL）。
    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    pub fn try_wait(&mut self) -> Option<ExitStatus> {
        self.child.try_wait().expect("try_wait")
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if !self.keep {
            app::remove_db(&self.db);
        }
    }
}
