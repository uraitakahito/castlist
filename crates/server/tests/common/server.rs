//! 受け入れの試験の道具: 本物の binary（castlist-server）を、server が自分で取った port で起こし、生の HTTP/1.1 で叩く。
//! 0.4.x の server（v0.4.0 の tag）の tests/common/mod.rs の Server を移した物（起こす binary と、一時の DB の folder の名だけが違う）。
// 試験の道具は同期で書く（async の中ではないので、糸を塞いでも困らない）。
#![allow(clippy::disallowed_methods)]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use super::{remove_db, temp_db};

/// 起こした castlist-server。落とす時に止める。
pub struct Server {
    child: Child,
    pub port: u16,
    /// 一時の DB。落とす時に、folder ごと消す（`keep` なら残す）。
    pub db: PathBuf,
    /// stdout を受ける file。
    out: PathBuf,
    /// 起こし直して前回の終わり方を見る試験は、DB の folder を残す。
    keep: bool,
}

/// 1 つの答え。
#[derive(Debug)]
pub struct Answer {
    pub status: u16,
    pub head: String,
    pub body: Vec<u8>,
}

impl Answer {
    /// header の値（名は大文字と小文字を問わずに引く）。
    pub fn header(&self, name: &str) -> Option<String> {
        self.head.lines().skip(1).find_map(|line| {
            let (key, value) = line.split_once(':')?;
            (key.trim().eq_ignore_ascii_case(name)).then(|| value.trim().to_owned())
        })
    }

    /// 本文の字。
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// 本文の JSON（読めなければ Null）。
    pub fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).unwrap_or(serde_json::Value::Null)
    }
}

/// 起こす binary。
pub fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_castlist-server")
}

impl Server {
    /// 空いた port で起こし、受けられるようになるまで待つ。
    pub fn start() -> Self {
        let mut server = Self::start_at(&temp_db("server"));
        server.keep = false;
        server
    }

    /// この DB で起こす（同じ folder で起こし直せる）。落としても folder は消さない。stdout は DB の隣の file に受ける。
    ///
    /// port は server が自分で取る（`127.0.0.1:0` で待ち受け、取れた番号を stdout の起きた行（listen=）に出す）。
    /// 試験が口を取って放してから渡すと、渡すまでの間に別の試験の server に取られうる（t-x36a・t-q4kd）。
    pub fn start_at(db: &Path) -> Self {
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let db = db.to_owned();
        // 同じ DB で起こし直す試験があるので、起こすごとに別の file に受ける。
        let out = db.with_file_name(format!(
            "stdout-{}.txt",
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let stdout = std::fs::File::create(&out).expect("stdout の file");
        let mut child = Command::new(binary())
            .env("CASTLIST_LISTEN", "127.0.0.1:0")
            .env("CASTLIST_DB", &db)
            // stderr は受け継ぐ（落ちた時の訳が、試験の出力に出る）。
            .stdout(Stdio::from(stdout))
            .spawn()
            .expect("castlist-server を起こす");
        let started = Instant::now();
        let port = loop {
            let text = std::fs::read_to_string(&out).unwrap_or_default();
            if let Some(port) = listening_port(&text) {
                break port;
            }
            if let Some(status) = child.try_wait().expect("try_wait") {
                panic!("待ち受ける前に終わった（{status}）: {text}");
            }
            if started.elapsed() > Duration::from_secs(10) {
                // 起こした物を残さない。
                let _ = child.kill();
                let _ = child.wait();
                panic!("10 秒の内に待ち受けない: {text}");
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        Self {
            child,
            port,
            db,
            out,
            keep: true,
        }
    }

    /// 生の要求を送り、答えを読む（要求は Connection: close にする）。
    pub fn send(&self, raw: &[u8]) -> Answer {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).expect("connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .expect("timeout");
        stream.write_all(raw).expect("write");
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).expect("read");
        let split = bytes
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .unwrap_or(bytes.len());
        let head = String::from_utf8_lossy(&bytes[..split]).into_owned();
        let mut body = bytes.get(split + 4..).unwrap_or_default().to_vec();
        if head
            .to_ascii_lowercase()
            .contains("\r\ntransfer-encoding: chunked")
        {
            body = dechunk(&body);
        }
        let status = head
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        Answer { status, head, body }
    }

    /// 生の要求（字）を送る。
    pub fn send_text(&self, raw: &str) -> Answer {
        self.send(raw.as_bytes())
    }

    pub fn get(&self, path: &str) -> Answer {
        self.send_text(&format!(
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
        ))
    }

    /// JSON の本文を付けた要求。
    pub fn json(&self, method: &str, path: &str, body: &serde_json::Value) -> Answer {
        let body = body.to_string();
        self.send_text(&format!(
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
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
        std::fs::read_to_string(&self.out).unwrap_or_default()
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

/// stdout の起きた行（`listen=127.0.0.1:<port> `）から、取れた port を読む（0 は取れていない）。
fn listening_port(stdout: &str) -> Option<u16> {
    let (_, rest) = stdout.split_once(" listen=127.0.0.1:")?;
    let (port, _) = rest.split_once(' ')?;
    port.parse().ok().filter(|&port| port != 0)
}

/// chunked の本文をつなぐ（長さを前に言わない答え。backup の file など）。
fn dechunk(mut raw: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    loop {
        let Some(end) = raw.windows(2).position(|w| w == b"\r\n") else {
            return body;
        };
        let size = std::str::from_utf8(&raw[..end])
            .ok()
            .and_then(|s| usize::from_str_radix(s.split(';').next().unwrap_or("").trim(), 16).ok())
            .expect("chunk の長さ");
        if size == 0 {
            return body;
        }
        let start = end + 2;
        body.extend_from_slice(&raw[start..start + size]);
        raw = &raw[start + size + 2..];
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if !self.keep {
            remove_db(&self.db);
        }
    }
}
