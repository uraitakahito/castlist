//! PutEvent: 出来事を足す。形は定義（model/）が見てあるので、ここでは detail が JSON の物かを見て DB に渡し、答えの形に直す。
use std::sync::Arc;

use castlist_sdk::error::PutEventError;
use castlist_sdk::input::PutEventInput;
use castlist_sdk::output::PutEventOutput;
use serde_json::Value;

use crate::db::{Db, NewEvent, Put, Ref};
use crate::errors::{Failure, PutFailure};
use crate::{body, rows};

const OPERATION: &str = "PutEvent";

pub async fn put_event(db: Arc<Db>, input: PutEventInput) -> Result<PutEventOutput, PutEventError> {
    Ok(put(&db, input).await?)
}

async fn put(db: &Db, input: PutEventInput) -> Result<PutEventOutput, PutFailure> {
    // 無い detail（と JSON の null）は、空の物にする。
    let detail = match input.detail.as_ref().map(body::json) {
        None | Some(Value::Null) => Value::Object(serde_json::Map::new()),
        Some(detail @ Value::Object(_)) => detail,
        Some(_) => return Err(PutFailure::Invalid("detail は JSON の物（{…}）".to_owned())),
    };
    let sent_at = crate::sent_time(&input.sent_at).map_err(PutFailure::Invalid)?;
    let put = db
        .put_event(
            &input.event_id,
            &NewEvent {
                issuer: &input.issuer,
                by: &Ref::parse(&input.by),
                kind: &input.r#type,
                detail: &detail,
                sent_at,
            },
        )
        .await
        .map_err(|e| PutFailure::write(OPERATION, e))?;
    let (row, status) = match put {
        Put::Created(row) => (row, 201),
        Put::Same(row) => (row, 200),
    };
    let e = rows::event(row).map_err(|e| Failure::data(OPERATION, &e))?;
    Ok(PutEventOutput {
        id: e.id,
        actor_id: e.actor_id,
        r#type: e.r#type,
        detail: e.detail,
        sent_at: e.sent_at,
        received_at: e.received_at,
        clock_skew: e.clock_skew,
        status,
    })
}
