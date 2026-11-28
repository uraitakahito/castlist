//! ListSettings: model と effort の行を、アクターをまたいで探す（監査の検索）。値の形は定義（model/）が見てあるので、
//! ここでは true だけの絞りと頁の印を見る。
use std::sync::Arc;

use castlist_sdk::error::{ListSettingsError, ValidationException};
use castlist_sdk::input::ListSettingsInput;
use castlist_sdk::output::ListSettingsOutput;

use crate::db::{self, Db};
use crate::errors::Failure;
use crate::search::{LIMIT, SettingQuery};
use crate::token::Search;
use crate::{datetime, only_true, rows, stored_time, token};

const OPERATION: &str = "ListSettings";

/// 頁の印の頭（ほかの検索の印と分ける）。
const TOKEN_KIND: &str = "settings";

fn invalid(message: impl Into<String>) -> ListSettingsError {
    ValidationException {
        message: message.into(),
        field_list: None,
    }
    .into()
}

pub async fn list_settings(
    db: Arc<Db>,
    input: ListSettingsInput,
) -> Result<ListSettingsOutput, ListSettingsError> {
    let as_of = db::now();
    let query = query(input)?;
    let page = db
        .search_settings(&query)
        .await
        .map_err(|e| Failure::db(OPERATION, e))?;
    let next_token = page
        .more
        .then(|| page.rows.last())
        .flatten()
        .map(|last| token::seq_token(TOKEN_KIND, last.seq));
    let data = |e: String| Failure::data(OPERATION, &e);
    Ok(ListSettingsOutput {
        as_of: datetime(&as_of).map_err(data)?,
        settings: page
            .rows
            .into_iter()
            .map(|s| rows::setting_row(s).map_err(data))
            .collect::<Result<_, _>>()?,
        next_token,
    })
}

/// 引数を探す条件にする。
fn query(input: ListSettingsInput) -> Result<SettingQuery, ListSettingsError> {
    let time = |name: &str, t: Option<&aws_smithy_types::DateTime>| {
        t.map(|t| stored_time(name, t)).transpose().map_err(invalid)
    };
    Ok(SettingQuery {
        from: time("from", input.from.as_ref())?,
        to: time("to", input.to.as_ref())?,
        mismatch: only_true("mismatch", input.mismatch).map_err(invalid)?,
        skewed: only_true("skewed", input.skewed).map_err(invalid)?,
        after: input
            .next_token
            .as_deref()
            .map(|t| token::seq_after(t, TOKEN_KIND, Search::Settings))
            .transpose()
            .map_err(invalid)?
            .unwrap_or(0),
        limit: input.max_results.map_or(LIMIT, i64::from),
        model: input.model,
        effort: input.effort,
        source: input.source,
    })
}
