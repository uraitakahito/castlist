//! GetLineage: 起こした・継いだ線を上り下りする。
use std::sync::Arc;

use castlist_sdk::error::GetLineageError;
use castlist_sdk::input::GetLineageInput;
use castlist_sdk::output::GetLineageOutput;

use crate::db::Db;
use crate::errors::Failure;

const OPERATION: &str = "GetLineage";

pub async fn get_lineage(
    _db: Arc<Db>,
    _input: GetLineageInput,
) -> Result<GetLineageOutput, GetLineageError> {
    Err(Failure::data(OPERATION, &"まだ無い").into())
}
