//! ListActorSettings と GetActorSettingAt: あるアクターの model と effort の全部と、ある時刻に効いていた物。値の形は定義（model/）が見てある。
use std::sync::Arc;

use aws_smithy_types::date_time::Format;
use castlist_sdk::error::{
    GetActorSettingAtError, ListActorSettingsError, NotFound, ValidationException,
};
use castlist_sdk::input::{GetActorSettingAtInput, ListActorSettingsInput};
use castlist_sdk::output::{GetActorSettingAtOutput, ListActorSettingsOutput};

use crate::db::{self, Db};
use crate::errors::Failure;
use crate::i18n::{text, text_with};
use crate::{datetime, rows, stored_time};

fn not_found(id: &str) -> NotFound {
    NotFound {
        message: text_with("actor-not-found", &[("id", &id)]),
    }
}

pub async fn list_actor_settings(
    db: Arc<Db>,
    input: ListActorSettingsInput,
) -> Result<ListActorSettingsOutput, ListActorSettingsError> {
    const OPERATION: &str = "ListActorSettings";
    let as_of = db::now();
    let Some(settings) = db
        .actor_settings(&input.actor_id)
        .await
        .map_err(|e| Failure::db(OPERATION, e))?
    else {
        return Err(not_found(&input.actor_id).into());
    };
    let data = |e: String| Failure::data(OPERATION, &e);
    Ok(ListActorSettingsOutput {
        as_of: datetime(&as_of).map_err(data)?,
        settings: settings
            .into_iter()
            .map(|s| rows::setting(s.setting).map_err(data))
            .collect::<Result<_, _>>()?,
        actor_id: input.actor_id,
    })
}

pub async fn get_actor_setting_at(
    db: Arc<Db>,
    input: GetActorSettingAtInput,
) -> Result<GetActorSettingAtOutput, GetActorSettingAtError> {
    const OPERATION: &str = "GetActorSettingAt";
    let as_of = db::now();
    let invalid = |message: String| ValidationException {
        message,
        field_list: None,
    };
    // 答えの at（RFC 3339）は 0001 年からしか書けないので、書けない時刻は先に断る。
    if input.time.fmt(Format::DateTime).is_err() {
        return Err(invalid(text("setting-time-range")).into());
    }
    // 問いの時刻は DB に残す字（UTC・ミリ秒まで）に直して比べ、直した物を答えの at にする。
    let at = stored_time("time", &input.time).map_err(invalid)?;
    let Some(setting) = db
        .actor_setting_at(&input.actor_id, &at)
        .await
        .map_err(|e| Failure::db(OPERATION, e))?
    else {
        return Err(not_found(&input.actor_id).into());
    };
    let data = |e: String| Failure::data(OPERATION, &e);
    Ok(GetActorSettingAtOutput {
        as_of: datetime(&as_of).map_err(data)?,
        at: datetime(&at).map_err(data)?,
        setting: setting
            .map(|s| rows::setting(s.setting).map_err(data))
            .transpose()?,
        actor_id: input.actor_id,
    })
}
