//! ListActorSettings と GetActorSettingAt: あるアクターの model と effort の全部と、ある時刻に効いていた物。
use std::sync::Arc;

use castlist_sdk::error::{GetActorSettingAtError, ListActorSettingsError};
use castlist_sdk::input::{GetActorSettingAtInput, ListActorSettingsInput};
use castlist_sdk::output::{GetActorSettingAtOutput, ListActorSettingsOutput};

use crate::db::Db;
use crate::errors::Failure;

pub async fn list_actor_settings(
    _db: Arc<Db>,
    _input: ListActorSettingsInput,
) -> Result<ListActorSettingsOutput, ListActorSettingsError> {
    Err(Failure::data("ListActorSettings", &"まだ無い").into())
}

pub async fn get_actor_setting_at(
    _db: Arc<Db>,
    _input: GetActorSettingAtInput,
) -> Result<GetActorSettingAtOutput, GetActorSettingAtError> {
    Err(Failure::data("GetActorSettingAt", &"まだ無い").into())
}
