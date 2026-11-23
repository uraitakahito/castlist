//! 画面の URL の引数を読む所。画面の引数と誤りの訳は今の server（src/search.rs・src/api.rs）のまま
//! （名は snake_case。知らない名・1 つだけの名の重なり・読めない値は、引数の名を書いた 400 の画面）。
//! 新しい API（生成した層が引数を読む）とは別に、画面だけがこれで読む。
use axum::extract::Query;
use axum::extract::rejection::QueryRejection;
use axum::http::StatusCode;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::db::{self, Ref};
use crate::graph::GraphQuery;
use crate::search::{ActorItem, ActorQuery, LIMIT, Page, Sort};
use crate::token;

/// 一覧の 1 頁の上限。
const MAX_LIMIT: i64 = 1000;

/// 画面の誤り（状態と、画面に書く訳）。
pub struct PageError(pub StatusCode, pub String);

/// 引数の誤り（400）。訳には、誤った引数の名を入れる。
pub fn bad_request(message: impl Into<String>) -> PageError {
    PageError(StatusCode::BAD_REQUEST, message.into())
}

pub fn not_found() -> PageError {
    PageError(StatusCode::NOT_FOUND, "そのアクターは無い".to_owned())
}

impl From<sqlx::Error> for PageError {
    /// DB の誤り。接続を待ちきれなければ混んでいる（503）、ほかは中の失敗（500。訳は記録に書き、画面は決まった字）。
    fn from(e: sqlx::Error) -> Self {
        match e {
            sqlx::Error::PoolTimedOut => PageError(
                StatusCode::SERVICE_UNAVAILABLE,
                "混んでいる。少し待ってから送り直す".to_owned(),
            ),
            e => {
                tracing::error!(%e, "画面の DB の誤り");
                PageError(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "castlist の中で失敗した（記録を見る）".to_owned(),
                )
            }
        }
    }
}

pub type Pairs = Vec<(String, String)>;

/// 問い合わせの引数を、名と値の並びで読む（読めなければ 400）。
pub fn pairs(q: Result<Query<Pairs>, QueryRejection>) -> Result<Pairs, PageError> {
    q.map(|Query(pairs)| pairs)
        .map_err(|e| bad_request(format!("問い合わせの引数が読めない: {e}")))
}

/// model・effort・出し手・付随する id の種類・ラベルの鍵の名: 英小文字・数字・`-`・`.` で 64 字まで。
fn is_name(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.')
}

/// 値: 空でなく、制御文字の無い、256 字までの字。
fn is_value(s: &str) -> bool {
    !s.is_empty() && s.chars().count() <= 256 && !s.chars().any(char::is_control)
}

/// 問い合わせの引数（名と値の並び）。知らない名と、1 つだけの名の重なりは、読む時に断る。
pub struct Params(Pairs);

impl Params {
    pub fn new(pairs: Pairs, known: &[&str], repeatable: &[&str]) -> Result<Self, PageError> {
        for (i, (name, _)) in pairs.iter().enumerate() {
            if !known.contains(&name.as_str()) {
                return Err(bad_request(format!(
                    "知らない引数: {name}（使えるのは {}）",
                    known.join("・")
                )));
            }
            if !repeatable.contains(&name.as_str()) && pairs[..i].iter().any(|(n, _)| n == name) {
                return Err(bad_request(format!("{name} は 1 つだけ付ける")));
            }
        }
        Ok(Self(pairs))
    }

    fn one(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> {
        self.0
            .iter()
            .filter(move |(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    /// 時刻（RFC 3339）を、DB に残す字（UTC・ミリ秒まで）にする。
    pub fn time(&self, name: &str) -> Result<Option<String>, PageError> {
        self.one(name)
            .map(|v| {
                OffsetDateTime::parse(v, &Rfc3339)
                    .map(db::stamp)
                    .map_err(|_| {
                        bad_request(format!(
                            "{name} は RFC 3339 の時刻（例 2026-11-22T13:00:00+09:00）"
                        ))
                    })
            })
            .transpose()
    }

    fn flag(&self, name: &str) -> Result<Option<bool>, PageError> {
        match self.one(name) {
            None => Ok(None),
            Some("true") => Ok(Some(true)),
            Some("false") => Ok(Some(false)),
            Some(_) => Err(bad_request(format!("{name} は true か false"))),
        }
    }

    /// `true` だけを受ける絞り（付けなければ絞らない）。
    fn only_true(&self, name: &str) -> Result<bool, PageError> {
        match self.one(name) {
            None => Ok(false),
            Some("true") => Ok(true),
            Some(_) => Err(bad_request(format!(
                "{name} は true だけ（絞らないなら付けない）"
            ))),
        }
    }

    fn limit(&self) -> Result<i64, PageError> {
        match self.one("limit") {
            None => Ok(LIMIT),
            Some(v) => v
                .parse()
                .ok()
                .filter(|n| (1..=MAX_LIMIT).contains(n))
                .ok_or_else(|| bad_request(format!("limit は 1 から {MAX_LIMIT} までの数"))),
        }
    }

    /// 名の形の値（model・effort など）。
    fn name(&self, name: &str) -> Result<Option<String>, PageError> {
        self.one(name)
            .map(|v| {
                if is_name(v) {
                    Ok(v.to_owned())
                } else {
                    Err(bad_request(format!(
                        "{name} は英小文字・数字・- . で 64 字まで"
                    )))
                }
            })
            .transpose()
    }

    /// 空でない 256 字までの値（id など）。
    fn value(&self, name: &str) -> Result<Option<String>, PageError> {
        self.one(name)
            .map(|v| {
                if is_value(v) {
                    Ok(v.to_owned())
                } else {
                    Err(bad_request(format!("{name} は空でない 256 字まで")))
                }
            })
            .transpose()
    }

    /// 決まった物のどれか。
    fn choice(
        &self,
        name: &str,
        choices: &[&'static str],
    ) -> Result<Option<&'static str>, PageError> {
        self.one(name)
            .map(|v| {
                choices.iter().find(|c| **c == v).copied().ok_or_else(|| {
                    bad_request(format!("{name} は {} のどれか", choices.join("・")))
                })
            })
            .transpose()
    }
}

/// `鍵:値`・`種類:値` を初めの `:` で分ける。
fn pair(name: &str, v: &str, shape: &str) -> Result<(String, String), PageError> {
    match v.split_once(':') {
        Some((key, value)) if is_name(key) && is_value(value) => {
            Ok((key.to_owned(), value.to_owned()))
        }
        _ => Err(bad_request(format!("{name} は「{shape}」の形（{v}）"))),
    }
}

/// 一覧の頁の印（`after`）の頭（API の ListActors の nextToken と同じ形）。
const TOKEN_KIND: &str = "actors";

fn broken_after() -> PageError {
    bad_request("after が読めない（前の答えの next を、そのまま渡す）")
}

// --- 一覧 ---

/// 一覧（`GET /`）の引数の名。今の server の `GET /v1/actors` と同じ。
pub const ACTOR_PARAMS: &[&str] = &[
    "label",
    "binding",
    "at",
    "active_from",
    "active_to",
    "kind",
    "model",
    "effort",
    "active",
    "launched_by",
    "skewed",
    "sort",
    "order",
    "limit",
    "after",
];

fn sort_name(sort: Sort) -> &'static str {
    match sort {
        Sort::Entered => "entered",
        Sort::Exited => "exited",
    }
}

fn order_name(desc: bool) -> &'static str {
    if desc { "desc" } else { "asc" }
}

/// 一覧の引数（[`ACTOR_PARAMS`]）を、探す条件にする。`as_of` は `at` の無い `binding` の時刻。
pub fn actor_query(p: &Params, as_of: &str) -> Result<ActorQuery, PageError> {
    let labels = p
        .all("label")
        .map(|v| pair("label", v, "鍵:値"))
        .collect::<Result<Vec<_>, _>>()?;
    let at = p.time("at")?;
    let binding = match p.one("binding") {
        Some(v) => {
            let (kind, value) = pair("binding", v, "種類:値")?;
            Some((Ref { kind, value }, at.unwrap_or_else(|| as_of.to_owned())))
        }
        None if at.is_some() => {
            return Err(bad_request(
                "at は binding と一緒に付ける（居た間で探すなら active_from・active_to）",
            ));
        }
        None => None,
    };
    let sort = match p.choice("sort", &["entered", "exited"])? {
        Some("exited") => Sort::Exited,
        _ => Sort::Entered,
    };
    let desc = p.choice("order", &["asc", "desc"])? == Some("desc");
    let after = match p.one("after") {
        None => None,
        Some(v) => {
            let parts = token::decode(v).ok_or_else(broken_after)?;
            let [kind, token_sort, token_order, value, seq] = &parts[..] else {
                return Err(bad_request(
                    "after が読めない（actors の検索の next ではない）",
                ));
            };
            if kind != TOKEN_KIND {
                return Err(bad_request(
                    "after が読めない（actors の検索の next ではない）",
                ));
            }
            if token_sort != sort_name(sort) || token_order != order_name(desc) {
                return Err(bad_request(
                    "after は、同じ sort と order の答えの next を渡す",
                ));
            }
            Some((value.clone(), seq.parse().map_err(|_| broken_after())?))
        }
    };
    Ok(ActorQuery {
        labels,
        binding,
        active_from: p.time("active_from")?,
        active_to: p.time("active_to")?,
        kind: p.choice("kind", &["ai", "human"])?.map(str::to_owned),
        model: p.name("model")?,
        effort: p.name("effort")?,
        active: p.flag("active")?,
        launched_by: p.value("launched_by")?,
        skewed: p.only_true("skewed")?,
        ids: None,
        sort,
        desc,
        after,
        limit: Some(p.limit()?),
    })
}

/// 一覧の次の頁の印（終わりなら None）。
pub fn actor_next(query: &ActorQuery, page: &Page<ActorItem>) -> Option<String> {
    page.more.then(|| page.rows.last()).flatten().map(|last| {
        token::encode(&[
            TOKEN_KIND,
            sort_name(query.sort),
            order_name(query.desc),
            &last.sort_key,
            &last.seq.to_string(),
        ])
    })
}

// --- 図 ---

/// 図の画面（`GET /graph/timeline`・`GET /graph/network`）の引数の名。どれも 1 つだけ。
pub const GRAPH_PARAMS: &[&str] = &["label", "from", "to"];

/// 図の引数を、図の絞りにする（窓が無ければ全部）。
pub fn graph_query(p: &Params) -> Result<GraphQuery, PageError> {
    Ok(GraphQuery {
        label: p
            .one("label")
            .map(|v| pair("label", v, "鍵:値"))
            .transpose()?,
        from: p.time("from")?.unwrap_or_default(),
        to: p.time("to")?.unwrap_or_else(|| "9999".to_owned()),
    })
}
