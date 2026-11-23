//! PutLink: 線を足す。形は定義（model/）が見てあるので、ここでは DB に渡して、答えの形に直す。
use std::sync::Arc;

use castlist_sdk::error::PutLinkError;
use castlist_sdk::input::PutLinkInput;
use castlist_sdk::output::PutLinkOutput;

use crate::db::{Db, NewLink, Put, Ref};
use crate::errors::{Failure, PutFailure};
use crate::rows;

const OPERATION: &str = "PutLink";

pub async fn put_link(db: Arc<Db>, input: PutLinkInput) -> Result<PutLinkOutput, PutLinkError> {
    Ok(put(&db, input).await?)
}

async fn put(db: &Db, input: PutLinkInput) -> Result<PutLinkOutput, PutFailure> {
    let sent_at = crate::sent_time(&input.sent_at).map_err(PutFailure::Invalid)?;
    let put = db
        .put_link(
            &input.link_id,
            &NewLink {
                issuer: &input.issuer,
                kind: &input.r#type,
                from: &Ref::parse(&input.from),
                to: &Ref::parse(&input.to),
                sent_at,
            },
        )
        .await
        .map_err(|e| PutFailure::write(OPERATION, e))?;
    let (row, status) = match put {
        Put::Created(row) => (row, 201),
        Put::Same(row) => (row, 200),
    };
    let l = rows::link(row).map_err(|e| Failure::data(OPERATION, &e))?;
    Ok(PutLinkOutput {
        id: l.id,
        r#type: l.r#type,
        from: l.from,
        to: l.to,
        sent_at: l.sent_at,
        received_at: l.received_at,
        clock_skew: l.clock_skew,
        status,
    })
}
