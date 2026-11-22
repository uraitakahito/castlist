//! JSON の API: アクターの登録・出来事・線・model と effort・引き当て・見る・DB の写し。
//! アクターは、鍵の付随する id（`type:value`）で指す。castlist が本当の id に引き当てる。

use crate::db::{self, Db, NewActor, NewEvent, NewLink, NewSetting, Put, Ref, WriteError};
use crate::error;
use axum::body::{Body, Bytes};
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::{Json, extract::rejection::QueryRejection};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tokio_util::io::ReaderStream;

/// 1 度に引き当てる参照の上限。
const MAX_REFS: usize = 100;

/// 誤りの答え（状態と、日本語の訳）。
pub struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        error(self.0, &self.1)
    }
}

fn invalid(message: impl Into<String>) -> ApiError {
    ApiError(StatusCode::UNPROCESSABLE_ENTITY, message.into())
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        WriteError::from(e).into()
    }
}

impl From<WriteError> for ApiError {
    fn from(e: WriteError) -> Self {
        match e {
            WriteError::Busy => ApiError(
                StatusCode::SERVICE_UNAVAILABLE,
                "混んでいる。少し待ってから送り直す".to_owned(),
            ),
            WriteError::Invalid(message) => invalid(message),
            WriteError::Conflict(message) => ApiError(StatusCode::CONFLICT, message),
            WriteError::Db(e) => {
                eprintln!("DB の誤り: {e}");
                ApiError(StatusCode::INTERNAL_SERVER_ERROR, "DB の誤り".to_owned())
            }
        }
    }
}

type Answer = Result<Response, ApiError>;

// --- 形を見る ---

/// model・effort・出し手・付随する id の種類・札の鍵の名: 英小文字・数字・`-`・`.` で 64 字まで。
pub fn is_name(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.')
}

/// 名の接頭辞: 英小文字・数字・`-` で 32 字まで（名は `<接頭辞>.<番号>`）。
fn is_prefix(s: &str) -> bool {
    (1..=32).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// 値: 空でなく、制御文字の無い、256 字までの字。
fn is_value(s: &str) -> bool {
    !s.is_empty() && s.chars().count() <= 256 && !s.chars().any(char::is_control)
}

/// 送り手が付ける id（出来事・線・model と effort）: 英数字と記号で 128 字まで。
fn is_id(s: &str) -> bool {
    (1..=128).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_graphic())
}

fn check(checks: &[(bool, &str)]) -> Result<(), ApiError> {
    match checks.iter().find(|(ok, _)| !ok) {
        Some((_, message)) => Err(invalid(*message)),
        None => Ok(()),
    }
}

/// `type:value` を分ける（初めの `:` で。値には `:` があってよい）。
fn parse_ref(s: &str) -> Result<Ref, ApiError> {
    match s.split_once(':') {
        Some((kind, value)) if is_name(kind) && is_value(value) => Ok(Ref {
            kind: kind.to_owned(),
            value: value.to_owned(),
        }),
        _ => Err(invalid(format!("参照は「種類:値」の形（{s}）"))),
    }
}

/// 札の `key:value`。
fn parse_label(s: &str) -> Result<(String, String), ApiError> {
    match s.split_once(':') {
        Some((key, value)) if is_name(key) && is_value(value) => {
            Ok((key.to_owned(), value.to_owned()))
        }
        _ => Err(invalid(format!("札は「鍵:値」の形（{s}）"))),
    }
}

fn parse_at(s: &str, name: &str) -> Result<OffsetDateTime, ApiError> {
    OffsetDateTime::parse(s, &Rfc3339).map_err(|_| invalid(format!("{name} は RFC 3339 の時刻")))
}

/// 本文の JSON を読む。読めなければ 422。
fn body<T: DeserializeOwned>(bytes: &Bytes) -> Result<T, ApiError> {
    serde_json::from_slice(bytes).map_err(|e| invalid(format!("本文の JSON が読めない: {e}")))
}

/// 同じ id で同じ本文かを比べるための写し（SHA-256 の 16 進）。
fn hash<T: Serialize>(body: &T) -> String {
    let bytes = serde_json::to_vec(body).unwrap_or_default();
    Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn put_answer<T: Serialize>(put: Put<T>) -> Response {
    match put {
        Put::Created(row) => (StatusCode::CREATED, Json(row)).into_response(),
        Put::Same(row) => (StatusCode::OK, Json(row)).into_response(),
    }
}

fn query<T>(q: Result<Query<T>, QueryRejection>) -> Result<T, ApiError> {
    q.map(|Query(q)| q)
        .map_err(|e| invalid(format!("問い合わせの引数が読めない: {e}")))
}

// --- 登録 ---

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewActorBody {
    issuer: String,
    kind: String,
    name_prefix: String,
    #[serde(default)]
    labels: BTreeMap<String, String>,
    bindings: Vec<BindingBody>,
    at: String,
    settings: Option<SettingsBody>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingBody {
    #[serde(rename = "type")]
    kind: String,
    value: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsBody {
    model: String,
    effort: String,
}

/// `POST /v1/actors`: 登録する。鍵（bindings の初め）が既にあれば、そのアクターを 200 で返す。
pub async fn register(State(db): State<Db>, bytes: Bytes) -> Answer {
    let b: NewActorBody = body(&bytes)?;
    let at = parse_at(&b.at, "at")?;
    check(&[
        (
            is_name(&b.issuer),
            "issuer は英小文字・数字・- . で 64 字まで",
        ),
        (
            matches!(b.kind.as_str(), "ai" | "human"),
            "kind は ai か human",
        ),
        (
            is_prefix(&b.name_prefix),
            "name_prefix は英小文字・数字・- で 32 字まで",
        ),
        (
            !b.bindings.is_empty(),
            "bindings に 1 つ以上（初めの物が鍵）",
        ),
        (
            b.bindings
                .iter()
                .all(|x| is_name(&x.kind) && is_value(&x.value)),
            "bindings の type は名の形、value は空でない 256 字まで",
        ),
        (
            b.labels.iter().all(|(k, v)| is_name(k) && is_value(v)),
            "labels の鍵は名の形、値は空でない 256 字まで",
        ),
        (
            b.settings
                .as_ref()
                .is_none_or(|s| is_name(&s.model) && is_name(&s.effort)),
            "model と effort は英小文字・数字・- . で 64 字まで",
        ),
    ])?;
    let bindings: Vec<Ref> = b
        .bindings
        .into_iter()
        .map(|x| Ref {
            kind: x.kind,
            value: x.value,
        })
        .collect();
    let labels: Vec<(String, String)> = b.labels.into_iter().collect();
    let (actor, created) = db
        .register(&NewActor {
            issuer: &b.issuer,
            kind: &b.kind,
            prefix: &b.name_prefix,
            labels: &labels,
            bindings: &bindings,
            at: &b.at,
            at_time: at,
            settings: b
                .settings
                .as_ref()
                .map(|s| (s.model.as_str(), s.effort.as_str())),
        })
        .await?;
    let status = if created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((
        status,
        Json(json!({ "id": actor.id, "name": actor.name, "kind": actor.kind, "created": created })),
    )
        .into_response())
}

// --- 出来事・線・model と effort ---

/// 出来事の種類（足す時は、migrations の CHECK と一緒に）。
pub const EVENT_TYPES: [&str; 10] = [
    "entered",
    "reentered",
    "forked",
    "compacted",
    "cleared",
    "exited",
    "closed",
    "reported_in",
    "model_unreadable",
    "retracted",
];

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EventBody {
    issuer: String,
    by: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default = "empty_object")]
    detail: Value,
    at: String,
}

fn empty_object() -> Value {
    json!({})
}

/// `PUT /v1/events/{id}`: 出来事を足す。
pub async fn put_event(State(db): State<Db>, Path(id): Path<String>, bytes: Bytes) -> Answer {
    let b: EventBody = body(&bytes)?;
    let at = parse_at(&b.at, "at")?;
    let by = parse_ref(&b.by)?;
    check(&[
        (is_id(&id), "id は英数字と記号で 128 字まで"),
        (
            is_name(&b.issuer),
            "issuer は英小文字・数字・- . で 64 字まで",
        ),
        (
            EVENT_TYPES.contains(&b.kind.as_str()),
            "type は entered・reentered・forked・compacted・cleared・exited・closed・reported_in・model_unreadable・retracted のどれか",
        ),
        (b.detail.is_object(), "detail は JSON の物（{…}）"),
    ])?;
    let detail = b.detail.to_string();
    let put = db
        .put_event(
            &id,
            &NewEvent {
                issuer: &b.issuer,
                by: &by,
                kind: &b.kind,
                detail: &detail,
                at: &b.at,
                at_time: at,
                body_hash: &hash(&b),
            },
        )
        .await?;
    Ok(put_answer(put))
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LinkBody {
    issuer: String,
    #[serde(rename = "type")]
    kind: String,
    from: String,
    to: String,
    at: String,
}

/// `PUT /v1/links/{id}`: 線を足す。
pub async fn put_link(State(db): State<Db>, Path(id): Path<String>, bytes: Bytes) -> Answer {
    let b: LinkBody = body(&bytes)?;
    let at = parse_at(&b.at, "at")?;
    let from = parse_ref(&b.from)?;
    let to = parse_ref(&b.to)?;
    check(&[
        (is_id(&id), "id は英数字と記号で 128 字まで"),
        (
            is_name(&b.issuer),
            "issuer は英小文字・数字・- . で 64 字まで",
        ),
        (
            matches!(b.kind.as_str(), "launched" | "continued"),
            "type は launched か continued",
        ),
    ])?;
    let put = db
        .put_link(
            &id,
            &NewLink {
                issuer: &b.issuer,
                kind: &b.kind,
                from: &from,
                to: &to,
                at: &b.at,
                at_time: at,
                body_hash: &hash(&b),
            },
        )
        .await?;
    Ok(put_answer(put))
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SettingBody {
    issuer: String,
    by: String,
    model: String,
    effort: String,
    source: String,
    at: String,
}

/// `PUT /v1/settings/{id}`: model と effort を足す。
pub async fn put_setting(State(db): State<Db>, Path(id): Path<String>, bytes: Bytes) -> Answer {
    let b: SettingBody = body(&bytes)?;
    let at = parse_at(&b.at, "at")?;
    let by = parse_ref(&b.by)?;
    check(&[
        (is_id(&id), "id は英数字と記号で 128 字まで"),
        (
            is_name(&b.issuer),
            "issuer は英小文字・数字・- . で 64 字まで",
        ),
        (
            is_name(&b.model),
            "model は英小文字・数字・- . で 64 字まで",
        ),
        (
            is_name(&b.effort),
            "effort は英小文字・数字・- . で 64 字まで",
        ),
        (
            matches!(b.source.as_str(), "requested" | "observed"),
            "source は requested か observed",
        ),
    ])?;
    let put = db
        .put_setting(
            &id,
            &NewSetting {
                issuer: &b.issuer,
                by: &by,
                model: &b.model,
                effort: &b.effort,
                source: &b.source,
                at: &b.at,
                at_time: at,
                body_hash: &hash(&b),
            },
        )
        .await?;
    Ok(put_answer(put))
}

// --- 引き当て・見る ---

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResolveBody {
    issuer: String,
    refs: Vec<String>,
}

/// `POST /v1/resolve`: 鍵の付随する id から、アクターを引く（無ければ null）。
pub async fn resolve(State(db): State<Db>, bytes: Bytes) -> Answer {
    let b: ResolveBody = body(&bytes)?;
    check(&[
        (
            is_name(&b.issuer),
            "issuer は英小文字・数字・- . で 64 字まで",
        ),
        (b.refs.len() <= MAX_REFS, "refs は 100 個まで"),
    ])?;
    let refs = b
        .refs
        .iter()
        .map(|r| parse_ref(r))
        .collect::<Result<Vec<_>, _>>()?;
    let as_of = db::now();
    let found = db.resolve(&b.issuer, &refs).await?;
    let results: Vec<Value> = b
        .refs
        .iter()
        .zip(found)
        .map(|(r, actor)| json!({ "ref": r, "actor": actor }))
        .collect();
    Ok(Json(json!({ "as_of": as_of, "results": results })).into_response())
}

/// `GET /v1/actors/{id}`: 1 つのアクターの全部。
pub async fn actor(State(db): State<Db>, Path(id): Path<String>) -> Answer {
    let as_of = db::now();
    match db.actor(&id).await? {
        Some(detail) => {
            let mut value = serde_json::to_value(detail).unwrap_or_default();
            value["as_of"] = json!(as_of);
            Ok(Json(value).into_response())
        }
        None => Err(ApiError(
            StatusCode::NOT_FOUND,
            "そのアクターは無い".to_owned(),
        )),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActorsQuery {
    binding: Option<String>,
    at: Option<String>,
    label: Option<String>,
}

/// `GET /v1/actors?binding=type:value&at=…` か `?label=key:value`。
pub async fn actors(State(db): State<Db>, q: Result<Query<ActorsQuery>, QueryRejection>) -> Answer {
    let q = query(q)?;
    let as_of = db::now();
    let actors = match (&q.binding, &q.label) {
        (Some(binding), None) => {
            let r = parse_ref(binding)?;
            let at = match &q.at {
                Some(at) => db::stamp(parse_at(at, "at")?),
                None => as_of.clone(),
            };
            db.actor_at(&r, &at).await?.into_iter().collect()
        }
        (None, Some(label)) => {
            let (key, value) = parse_label(label)?;
            db.actors_with_label(&key, &value).await?
        }
        _ => return Err(invalid("binding か label の、どちらか 1 つを付ける")),
    };
    Ok(Json(json!({ "as_of": as_of, "actors": actors })).into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphQuery {
    label: Option<String>,
    from: Option<String>,
    to: Option<String>,
}

/// `GET /v1/graph?label=key:value&from=…&to=…`: 相関図（点と線）。
pub async fn graph(State(db): State<Db>, q: Result<Query<GraphQuery>, QueryRejection>) -> Answer {
    let q = query(q)?;
    let label = q.label.as_deref().map(parse_label).transpose()?;
    let from = match &q.from {
        Some(from) => db::stamp(parse_at(from, "from")?),
        None => String::new(),
    };
    let to = match &q.to {
        Some(to) => db::stamp(parse_at(to, "to")?),
        None => "9999".to_owned(),
    };
    let as_of = db::now();
    let graph = db
        .graph(
            label.as_ref().map(|(k, v)| (k.as_str(), v.as_str())),
            &from,
            &to,
        )
        .await?;
    Ok(Json(json!({ "as_of": as_of, "nodes": graph.nodes, "edges": graph.edges })).into_response())
}

/// `GET /v1/backup`: DB の揃った写しを 1 つの file で返す（`curl -o castlist.db <URL>/v1/backup`）。
pub async fn backup(State(db): State<Db>) -> Answer {
    let path = db.backup().await?;
    let file = tokio::fs::File::open(&path).await;
    // 開いた後は名が要らない。消しても、読み終えるまで中身は残る。
    let _ = tokio::fs::remove_file(&path).await;
    let file = file.map_err(|e| {
        eprintln!("写しを開けない: {e}");
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "写しを開けない".to_owned(),
        )
    })?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/vnd.sqlite3"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"castlist.db\"",
            ),
        ],
        Body::from_stream(ReaderStream::new(file)),
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::{is_name, parse_ref};

    #[test]
    fn 名の形() {
        assert!(is_name("claude-opus-5-5"));
        assert!(is_name("cmux.workspace.ref"));
        assert!(is_name(&"a".repeat(64)));
        for bad in ["", "Opus", "a b", "a_b", "高い", &"a".repeat(65)] {
            assert!(!is_name(bad), "{bad}");
        }
    }

    #[test]
    fn 参照は初めの_colon_で分ける() {
        let r = parse_ref("cmux.workspace.ref:workspace:1").ok().unwrap();
        assert_eq!(
            (r.kind.as_str(), r.value.as_str()),
            ("cmux.workspace.ref", "workspace:1")
        );
        assert!(parse_ref("colon-less").is_err());
        assert!(parse_ref("type:").is_err());
    }
}
