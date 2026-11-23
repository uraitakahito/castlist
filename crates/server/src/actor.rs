//! GetActor: 1 つのアクターの全部。DB の行を、答えの形に直す。
use std::sync::Arc;

use aws_smithy_types::{Document, Number};
use castlist_sdk::error::{GetActorError, NotFound, Unavailable};
use castlist_sdk::input::GetActorInput;
use castlist_sdk::model::{ActorBinding, ActorState, Event, Link, Setting};
use castlist_sdk::output::GetActorOutput;

use crate::db::{self, Db};
use crate::{datetime, unavailable};

pub async fn get_actor(db: Arc<Db>, input: GetActorInput) -> Result<GetActorOutput, GetActorError> {
    let as_of = datetime(&db::now())?;
    let Some(detail) = db.actor(&input.actor_id).await.map_err(unavailable)? else {
        return Err(NotFound {
            message: format!("アクター {} は無い", input.actor_id),
        }
        .into());
    };
    let actor = detail.actor;
    Ok(GetActorOutput {
        sent_at: datetime(&actor.sent_at)?,
        received_at: datetime(&actor.received_at)?,
        id: actor.id,
        name: actor.name,
        kind: actor.kind,
        issuer: actor.issuer,
        clock_skew: actor.clock_skew,
        labels: detail.labels.into_iter().collect(),
        bindings: bindings(detail.bindings)?,
        current_bindings: bindings(detail.current_bindings)?,
        events: detail
            .events
            .into_iter()
            .map(event)
            .collect::<Result<_, _>>()?,
        settings: detail
            .settings
            .into_iter()
            .map(setting)
            .collect::<Result<_, _>>()?,
        current_settings: detail.current_settings.map(setting).transpose()?,
        links_in: links(detail.links_in)?,
        links_out: links(detail.links_out)?,
        state: ActorState {
            presence: detail.state.presence,
            present: detail.state.present,
            inferred: detail.state.inferred,
            entered_at: detail
                .state
                .entered_at
                .as_deref()
                .map(datetime)
                .transpose()?,
            exited_at: detail
                .state
                .exited_at
                .as_deref()
                .map(datetime)
                .transpose()?,
        },
        as_of,
    })
}

fn bindings(rows: Vec<db::Binding>) -> Result<Vec<ActorBinding>, Unavailable> {
    rows.into_iter()
        .map(|b| {
            Ok(ActorBinding {
                sent_at: datetime(&b.sent_at)?,
                received_at: datetime(&b.received_at)?,
                issuer: b.issuer,
                r#type: b.kind,
                value: b.value,
                is_key: b.is_key,
            })
        })
        .collect()
}

fn event(e: db::Event) -> Result<Event, Unavailable> {
    let detail = serde_json::from_str(&e.detail).map_err(|err| Unavailable {
        message: format!("出来事 {} の detail が読めない: {err}", e.id),
    })?;
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

fn setting(s: db::Setting) -> Result<Setting, Unavailable> {
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

fn links(rows: Vec<db::Link>) -> Result<Vec<Link>, Unavailable> {
    rows.into_iter()
        .map(|l| {
            Ok(Link {
                sent_at: datetime(&l.sent_at)?,
                received_at: datetime(&l.received_at)?,
                id: l.id,
                r#type: l.kind,
                from: l.from_actor,
                to: l.to_actor,
                clock_skew: l.clock_skew,
            })
        })
        .collect()
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
