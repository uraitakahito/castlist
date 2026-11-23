//! DB の行を、答えの形に直す（GetActor の中の物と、Put… の答えで同じ形。一覧の行は ListActors と GetLineage で同じ形）。直せなければ、その訳の字を返す（呼ぶ側が中の失敗にする）。
use aws_smithy_types::{Document, Number};
use castlist_sdk::model::{ActorRow, Event, Link, Setting};

use crate::datetime;
use crate::{db, search};

pub fn event(e: db::Event) -> Result<Event, String> {
    let detail = serde_json::from_str(&e.detail)
        .map_err(|err| format!("出来事 {} の detail が読めない: {err}", e.id))?;
    Ok(Event {
        sent_at: datetime(&e.sent_at)?,
        received_at: datetime(&e.received_at)?,
        id: e.id,
        actor_id: e.actor_id,
        r#type: e.kind,
        detail: document(detail),
        clock_skew: e.clock_skew,
    })
}

pub fn setting(s: db::Setting) -> Result<Setting, String> {
    Ok(Setting {
        sent_at: datetime(&s.sent_at)?,
        received_at: datetime(&s.received_at)?,
        id: s.id,
        actor_id: s.actor_id,
        model: s.model,
        effort: s.effort,
        source: s.source,
        clock_skew: s.clock_skew,
    })
}

pub fn link(l: db::Link) -> Result<Link, String> {
    Ok(Link {
        sent_at: datetime(&l.sent_at)?,
        received_at: datetime(&l.received_at)?,
        id: l.id,
        r#type: l.kind,
        from: l.from_actor,
        to: l.to_actor,
        clock_skew: l.clock_skew,
    })
}

/// JSON の値を、答えの Document にする（数は、符号の無い整数・負の整数・小数のどれかに）。
fn document(value: serde_json::Value) -> Document {
    use serde_json::Value;
    match value {
        Value::Null => Document::Null,
        Value::Bool(b) => Document::Bool(b),
        Value::Number(n) => Document::Number(match (n.as_u64(), n.as_i64()) {
            (Some(u), _) => Number::PosInt(u),
            (None, Some(i)) => Number::NegInt(i),
            (None, None) => Number::Float(n.as_f64().unwrap_or(f64::NAN)),
        }),
        Value::String(s) => Document::String(s),
        Value::Array(items) => Document::Array(items.into_iter().map(document).collect()),
        Value::Object(map) => {
            Document::Object(map.into_iter().map(|(k, v)| (k, document(v))).collect())
        }
    }
}

/// 一覧の 1 行を、答えの形にする（ListActors の行と、GetLineage の点で同じ形）。
pub fn actor_row(a: search::ActorItem) -> Result<ActorRow, String> {
    Ok(ActorRow {
        sent_at: datetime(&a.sent_at)?,
        received_at: datetime(&a.received_at)?,
        first_entered_at: a.first_entered_at.as_deref().map(datetime).transpose()?,
        last_exited_at: a.last_exited_at.as_deref().map(datetime).transpose()?,
        url: format!("/actors/{}", a.id),
        id: a.id,
        name: a.name,
        kind: a.kind,
        issuer: a.issuer,
        clock_skew: a.clock_skew,
        labels: a.labels.into_iter().collect(),
        last_exit_type: a.last_exit_type,
        present: a.present,
        inferred: a.inferred,
        model: a.model,
        effort: a.effort,
    })
}
