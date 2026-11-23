//! ListSettings: model と effort の行を、アクターをまたいで探す（監査の検索）。
use std::sync::Arc;

use castlist_sdk::error::ListSettingsError;
use castlist_sdk::input::ListSettingsInput;
use castlist_sdk::output::ListSettingsOutput;

use crate::db::Db;
use crate::errors::Failure;

const OPERATION: &str = "ListSettings";

pub async fn list_settings(
    _db: Arc<Db>,
    _input: ListSettingsInput,
) -> Result<ListSettingsOutput, ListSettingsError> {
    Err(Failure::data(OPERATION, &"まだ無い").into())
}
