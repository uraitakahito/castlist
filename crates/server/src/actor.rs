//! GetActor: 1 つのアクターの全部。DB の行を、答えの形に直す。
use std::sync::Arc;

use castlist_sdk::error::{GetActorError, NotFound};
use castlist_sdk::input::GetActorInput;
use castlist_sdk::model::{ActorBinding, Event, Link, Setting};
use castlist_sdk::output::GetActorOutput;

use crate::db::{self, Db};
use crate::errors::Failure;
use crate::rows;

const OPERATION: &str = "GetActor";

/// DB の字の時刻を、答えの時刻にする（読めなければ中の失敗）。
fn datetime(s: &str) -> Result<aws_smithy_types::DateTime, Failure> {
    crate::datetime(s).map_err(|e| Failure::data(OPERATION, &e))
}

pub async fn get_actor(db: Arc<Db>, input: GetActorInput) -> Result<GetActorOutput, GetActorError> {
    let as_of = datetime(&db::now())?;
    let Some(detail) = db
        .actor(&input.actor_id)
        .await
        .map_err(|e| Failure::db(OPERATION, e))?
    else {
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
        state: rows::state(detail.state).map_err(|e| Failure::data(OPERATION, &e))?,
        as_of,
    })
}

fn bindings(rows: Vec<db::Binding>) -> Result<Vec<ActorBinding>, Failure> {
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

fn event(e: db::Event) -> Result<Event, Failure> {
    rows::event(e).map_err(|e| Failure::data(OPERATION, &e))
}

fn setting(s: db::Setting) -> Result<Setting, Failure> {
    rows::setting(s).map_err(|e| Failure::data(OPERATION, &e))
}

fn links(rows: Vec<db::Link>) -> Result<Vec<Link>, Failure> {
    rows.into_iter()
        .map(|l| rows::link(l).map_err(|e| Failure::data(OPERATION, &e)))
        .collect()
}
