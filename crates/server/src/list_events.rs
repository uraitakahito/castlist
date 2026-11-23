//! ListEvents: 出来事を、アクターをまたいで探す（監査の検索）。値の形（種類は 10 種のどれか・actor は ActorId・reason と source は
//! 空でない 256 字まで）は定義（model/）が見てあるので、ここでは true だけの絞りと頁の印を見る。
use std::sync::Arc;

use castlist_sdk::error::{ListEventsError, ValidationException};
use castlist_sdk::input::ListEventsInput;
use castlist_sdk::output::ListEventsOutput;

use crate::db::{self, Db};
use crate::errors::Failure;
use crate::search::{EventQuery, LIMIT};
use crate::{datetime, only_true, rows, stored_time, token};

const OPERATION: &str = "ListEvents";

/// 頁の印の頭（ほかの検索の印と分ける）。
const TOKEN_KIND: &str = "events";

fn invalid(message: impl Into<String>) -> ListEventsError {
    ValidationException {
        message: message.into(),
        field_list: None,
    }
    .into()
}

pub async fn list_events(
    db: Arc<Db>,
    input: ListEventsInput,
) -> Result<ListEventsOutput, ListEventsError> {
    let as_of = db::now();
    let query = query(input)?;
    let page = db
        .search_events(&query)
        .await
        .map_err(|e| Failure::db(OPERATION, e))?;
    let next_token = page
        .more
        .then(|| page.rows.last())
        .flatten()
        .map(|last| token::seq_token(TOKEN_KIND, last.seq));
    let data = |e: String| Failure::data(OPERATION, &e);
    Ok(ListEventsOutput {
        as_of: datetime(&as_of).map_err(data)?,
        events: page
            .rows
            .into_iter()
            .map(|e| rows::event_row(e).map_err(data))
            .collect::<Result<_, _>>()?,
        next_token,
    })
}

/// 引数を探す条件にする。
fn query(input: ListEventsInput) -> Result<EventQuery, ListEventsError> {
    let time = |name: &str, t: Option<&aws_smithy_types::DateTime>| {
        t.map(|t| stored_time(name, t)).transpose().map_err(invalid)
    };
    Ok(EventQuery {
        from: time("from", input.from.as_ref())?,
        to: time("to", input.to.as_ref())?,
        skewed: only_true("skewed", input.skewed).map_err(invalid)?,
        after: input
            .next_token
            .as_deref()
            .map(|t| token::seq_after(t, TOKEN_KIND, "出来事の検索"))
            .transpose()
            .map_err(invalid)?
            .unwrap_or(0),
        limit: input.max_results.map_or(LIMIT, i64::from),
        types: input.r#type.unwrap_or_default(),
        actor: input.actor,
        reason: input.reason,
        source: input.source,
    })
}
