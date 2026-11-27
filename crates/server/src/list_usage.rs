//! ListUsage: トークンの使用量の行を、アクターをまたいで読む。値の形（時刻・`鍵:値`・ActorId）と、from と to が在ることは
//! 定義（model/）が見てあるので、ここでは窓の長さと頁の印を見る。集計はしない（まとめるのは呼ぶ側）。
use std::sync::Arc;

use castlist_sdk::error::{ListUsageError, ValidationException};
use castlist_sdk::input::ListUsageInput;
use castlist_sdk::output::ListUsageOutput;

use crate::db::{self, Db, Ref};
use crate::errors::Failure;
use crate::search::{LIMIT, UsageQuery};
use crate::{datetime, rows, stored_time, token};

const OPERATION: &str = "ListUsage";

/// 頁の印の頭（ほかの検索の印と分ける）。
const TOKEN_KIND: &str = "usage";

/// 窓（`to - from`）の上限。
const MAX_WINDOW: time::Duration = time::Duration::days(31);

fn invalid(message: impl Into<String>) -> ListUsageError {
    ValidationException {
        message: message.into(),
        field_list: None,
    }
    .into()
}

pub async fn list_usage(
    db: Arc<Db>,
    input: ListUsageInput,
) -> Result<ListUsageOutput, ListUsageError> {
    let as_of = db::now();
    let query = query(input)?;
    let found = db
        .search_usage(&query)
        .await
        .map_err(|e| Failure::db(OPERATION, e))?;
    let next_token = found
        .page
        .more
        .then(|| found.page.rows.last())
        .flatten()
        .map(|last| token::seq_token(TOKEN_KIND, last.seq));
    let data = |e: String| Failure::data(OPERATION, &e);
    Ok(ListUsageOutput {
        as_of: datetime(&as_of).map_err(data)?,
        usage: found
            .page
            .rows
            .into_iter()
            .map(|u| rows::usage(u).map_err(data))
            .collect::<Result<_, _>>()?,
        actors: found.actors.into_iter().map(rows::usage_actor).collect(),
        next_token,
    })
}

/// 引数を探す条件にする。
fn query(input: ListUsageInput) -> Result<UsageQuery, ListUsageError> {
    if input.to.as_nanos() - input.from.as_nanos() > MAX_WINDOW.whole_nanoseconds() {
        return Err(invalid("from から to までは 31 日まで"));
    }
    Ok(UsageQuery {
        from: stored_time("from", &input.from).map_err(invalid)?,
        to: stored_time("to", &input.to).map_err(invalid)?,
        labels: input
            .label
            .unwrap_or_default()
            .iter()
            .map(|l| {
                let r = Ref::parse(l);
                (r.kind, r.value)
            })
            .collect(),
        actor: input.actor_id,
        after: input
            .next_token
            .as_deref()
            .map(|t| token::seq_after(t, TOKEN_KIND, "トークンの使用量の検索"))
            .transpose()
            .map_err(invalid)?
            .unwrap_or(0),
        limit: input.max_results.map_or(LIMIT, i64::from),
    })
}
