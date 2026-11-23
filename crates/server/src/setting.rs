//! PutSetting: model と effort を足す。形は定義（model/）が見てあるので、ここでは DB に渡して、答えの形に直す。
use std::sync::Arc;

use castlist_sdk::error::PutSettingError;
use castlist_sdk::input::PutSettingInput;
use castlist_sdk::output::PutSettingOutput;

use crate::db::{Db, NewSetting, Put, Ref};
use crate::errors::{Failure, PutFailure};
use crate::rows;

const OPERATION: &str = "PutSetting";

pub async fn put_setting(
    db: Arc<Db>,
    input: PutSettingInput,
) -> Result<PutSettingOutput, PutSettingError> {
    Ok(put(&db, input).await?)
}

async fn put(db: &Db, input: PutSettingInput) -> Result<PutSettingOutput, PutFailure> {
    let sent_at = crate::sent_time(&input.sent_at).map_err(PutFailure::Invalid)?;
    let put = db
        .put_setting(
            &input.setting_id,
            &NewSetting {
                issuer: &input.issuer,
                by: &Ref::parse(&input.by),
                model: &input.model,
                effort: &input.effort,
                source: &input.source,
                sent_at,
            },
        )
        .await
        .map_err(|e| PutFailure::write(OPERATION, e))?;
    // 最新と同じで足さなかった時も 200（返すのは、その最新の行）。
    let (row, status) = match put {
        Put::Created(row) => (row, 201),
        Put::Same(row) => (row, 200),
    };
    let s = rows::setting(row).map_err(|e| Failure::data(OPERATION, &e))?;
    Ok(PutSettingOutput {
        id: s.id,
        actor_id: s.actor_id,
        model: s.model,
        effort: s.effort,
        source: s.source,
        sent_at: s.sent_at,
        received_at: s.received_at,
        clock_skew: s.clock_skew,
        status,
    })
}
