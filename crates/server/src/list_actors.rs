//! ListActors: アクターを条件で探す（監査の検索）。値の形は定義（model/）が見てあるので、ここでは組み合わせと頁の印を見る。
use std::sync::Arc;

use castlist_sdk::error::{ListActorsError, ValidationException};
use castlist_sdk::input::ListActorsInput;
use castlist_sdk::output::ListActorsOutput;

use crate::db::{self, Db, Ref};
use crate::errors::Failure;
use crate::search::{ActorQuery, LIMIT, Page, Sort};
use crate::{datetime, rows, stored_time};

const OPERATION: &str = "ListActors";

/// 頁の印の頭（ほかの検索の印と分ける）。
const TOKEN_KIND: &str = "actors";

fn invalid(message: impl Into<String>) -> ListActorsError {
    ValidationException {
        message: message.into(),
        field_list: None,
    }
    .into()
}

pub async fn list_actors(
    db: Arc<Db>,
    input: ListActorsInput,
) -> Result<ListActorsOutput, ListActorsError> {
    let as_of = db::now();
    let query = query(input, &as_of)?;
    if !query.narrowed() {
        return Err(invalid(
            "絞る条件を 1 つ以上付ける（label・binding・activeFrom・activeTo・active=true・model・effort・launchedBy・skewed のどれか）",
        ));
    }
    let page = db
        .search_actors(&query)
        .await
        .map_err(|e| Failure::db(OPERATION, e))?;
    let next_token = next_token(&query, &page);
    Ok(ListActorsOutput {
        as_of: datetime(&as_of).map_err(|e| Failure::data(OPERATION, &e))?,
        actors: page
            .rows
            .into_iter()
            .map(|a| rows::actor_row(a).map_err(|e| Failure::data(OPERATION, &e)))
            .collect::<Result<_, _>>()?,
        next_token,
    })
}

/// 引数を探す条件にする。`as_of` は `at` の無い `binding` の時刻。
fn query(input: ListActorsInput, as_of: &str) -> Result<ActorQuery, ListActorsError> {
    let time = |name: &str, t: Option<&aws_smithy_types::DateTime>| {
        t.map(|t| stored_time(name, t)).transpose().map_err(invalid)
    };
    let at = time("at", input.at.as_ref())?;
    let binding = match (&input.binding, at) {
        (Some(b), at) => Some((Ref::parse(b), at.unwrap_or_else(|| as_of.to_owned()))),
        (None, Some(_)) => {
            return Err(invalid(
                "at は binding と一緒に付ける（稼働した期間で探すなら activeFrom・activeTo）",
            ));
        }
        (None, None) => None,
    };
    let skewed = match input.skewed {
        None => false,
        Some(true) => true,
        Some(false) => return Err(invalid("skewed は true だけ（絞らないなら付けない）")),
    };
    let sort = match input.sort.as_deref() {
        Some("exited") => Sort::Exited,
        _ => Sort::Entered,
    };
    let desc = input.order.as_deref() == Some("desc");
    let after = input
        .next_token
        .as_deref()
        .map(|t| after(t, sort, desc))
        .transpose()?;
    Ok(ActorQuery {
        labels: input
            .label
            .unwrap_or_default()
            .iter()
            .map(|l| {
                let r = Ref::parse(l);
                (r.kind, r.value)
            })
            .collect(),
        binding,
        active_from: time("activeFrom", input.active_from.as_ref())?,
        active_to: time("activeTo", input.active_to.as_ref())?,
        kind: input.kind,
        model: input.model,
        effort: input.effort,
        active: input.active,
        launched_by: input.launched_by,
        skewed,
        ids: None,
        sort,
        desc,
        after,
        limit: Some(input.max_results.map_or(LIMIT, i64::from)),
    })
}

fn sort_name(sort: Sort) -> &'static str {
    match sort {
        Sort::Entered => "entered",
        Sort::Exited => "exited",
    }
}

fn order_name(desc: bool) -> &'static str {
    if desc { "desc" } else { "asc" }
}

/// 次の頁の印（終わりなら無い）: 種類・sort・order・最後の行の並べる値と seq を改行で繋ぎ、16 進にした字。
/// 中は読まずに、前の答えの物をそのまま渡してもらう。ほかの条件は結ばない（今の server と同じ）。
fn next_token(query: &ActorQuery, page: &Page) -> Option<String> {
    page.more.then(|| page.rows.last()).flatten().map(|last| {
        [
            TOKEN_KIND,
            sort_name(query.sort),
            order_name(query.desc),
            &last.sort_key,
            &last.seq.to_string(),
        ]
        .join("\n")
        .bytes()
        .map(|b| format!("{b:02x}"))
        .collect()
    })
}

/// 頁の印を読む（前の頁の最後の並べる値と seq）。崩れた物・ほかの検索の物・sort か order の違う物は 400。
fn after(token: &str, sort: Sort, desc: bool) -> Result<(String, i64), ListActorsError> {
    let bad = || invalid("nextToken が読めない（前の答えの nextToken を、そのまま渡す）");
    if !token.len().is_multiple_of(2) || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(bad());
    }
    let bytes = (0..token.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&token[i..i + 2], 16))
        .collect::<Result<Vec<u8>, _>>()
        .map_err(|_| bad())?;
    let text = String::from_utf8(bytes).map_err(|_| bad())?;
    let parts: Vec<&str> = text.split('\n').collect();
    let [kind, token_sort, token_order, value, seq] = parts[..] else {
        return Err(invalid(
            "nextToken が読めない（アクターの検索の nextToken ではない）",
        ));
    };
    if kind != TOKEN_KIND {
        return Err(invalid(
            "nextToken が読めない（アクターの検索の nextToken ではない）",
        ));
    }
    if token_sort != sort_name(sort) || token_order != order_name(desc) {
        return Err(invalid("nextToken は、同じ sort と order の答えの物を渡す"));
    }
    let seq = seq.parse().map_err(|_| bad())?;
    Ok((value.to_owned(), seq))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_reads_back_and_a_broken_or_other_one_is_refused() {
        let page = Page {
            rows: vec![crate::search::ActorItem {
                seq: 7,
                id: "a".into(),
                name: "se.1".into(),
                kind: "ai".into(),
                issuer: "tpm".into(),
                sent_at: String::new(),
                received_at: String::new(),
                clock_skew: false,
                labels: Default::default(),
                first_entered_at: None,
                last_exited_at: None,
                last_exit_type: None,
                present: false,
                inferred: false,
                model: None,
                effort: None,
                sort_key: "2026-11-22T00:00:00.000Z".into(),
            }],
            more: true,
        };
        let query = ActorQuery {
            sort: Sort::Exited,
            desc: true,
            ..ActorQuery::default()
        };
        let token = next_token(&query, &page).unwrap();
        assert_eq!(
            after(&token, Sort::Exited, true).ok(),
            Some(("2026-11-22T00:00:00.000Z".to_owned(), 7))
        );
        assert!(after(&token, Sort::Entered, true).is_err());
        assert!(after(&token, Sort::Exited, false).is_err());
        assert!(after("zz", Sort::Entered, false).is_err());
        assert!(after("abc", Sort::Entered, false).is_err());
        assert!(after("ff", Sort::Entered, false).is_err());
        assert!(
            next_token(
                &query,
                &Page {
                    rows: page.rows.clone(),
                    more: false
                }
            )
            .is_none()
        );
    }
}
