//! ListEvents: 出来事を、アクターをまたいで探す（監査の検索）。
use std::sync::Arc;

use castlist_sdk::error::ListEventsError;
use castlist_sdk::input::ListEventsInput;
use castlist_sdk::output::ListEventsOutput;

use crate::db::Db;
use crate::errors::Failure;

const OPERATION: &str = "ListEvents";

pub async fn list_events(
    _db: Arc<Db>,
    _input: ListEventsInput,
) -> Result<ListEventsOutput, ListEventsError> {
    Err(Failure::data(OPERATION, &"まだ無い").into())
}
