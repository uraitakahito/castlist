//! PutUsage: アクターのトークンの使用量を 1 行足す。値の形（名・0 以上の数・段は 0〜40）は定義（model/）が見てあるので、
//! ここでは区間（同じ UTC の時の内）と段の並び（同じ段は 1 度だけ・和が行の数と合う）を見て DB に渡し、答えの形に直す。
use std::collections::HashSet;
use std::sync::Arc;

use castlist_sdk::error::PutUsageError;
use castlist_sdk::input::PutUsageInput;
use castlist_sdk::model::UsageBucket;
use castlist_sdk::output::PutUsageOutput;

use crate::db::{Bucket, Db, NewUsage, Put, Ref};
use crate::errors::{Failure, PutFailure};
use crate::{rows, stored_time};

const OPERATION: &str = "PutUsage";

/// DB に残す時刻の字（`2026-11-24T10:05:00.000Z`）のうち、年月日と時までの長さ。
const HOUR: usize = "2026-11-24T10".len();

pub async fn put_usage(db: Arc<Db>, input: PutUsageInput) -> Result<PutUsageOutput, PutUsageError> {
    Ok(put(&db, input).await?)
}

async fn put(db: &Db, input: PutUsageInput) -> Result<PutUsageOutput, PutFailure> {
    let sent_at = crate::sent_time(&input.sent_at).map_err(PutFailure::Invalid)?;
    let (from_at, to_at) = span(&input).map_err(PutFailure::Invalid)?;
    let buckets = buckets(&input).map_err(PutFailure::Invalid)?;
    let put = db
        .put_usage(
            &input.usage_id,
            &NewUsage {
                issuer: &input.issuer,
                by: &Ref::parse(&input.by),
                model: &input.model,
                agent: input.agent.as_deref(),
                speed: &input.speed,
                service_tier: &input.service_tier,
                from_at: &from_at,
                to_at: &to_at,
                turns: input.turns,
                input_tokens: input.input_tokens,
                output_tokens: input.output_tokens,
                cache_read_tokens: input.cache_read_tokens,
                cache_write_5m_tokens: input.cache_write5m_tokens,
                cache_write_1h_tokens: input.cache_write1h_tokens,
                web_search_requests: input.web_search_requests,
                web_fetch_requests: input.web_fetch_requests,
                cost_micro_usd: input.cost_micro_usd,
                max_context: input.max_context,
                buckets: &buckets,
                sent_at,
            },
        )
        .await
        .map_err(|e| PutFailure::write(OPERATION, e))?;
    let (row, status) = match put {
        Put::Created(row) => (row, 201),
        Put::Same(row) => (row, 200),
    };
    let u = rows::usage(row).map_err(|e| Failure::data(OPERATION, &e))?;
    Ok(PutUsageOutput {
        id: u.id,
        actor_id: u.actor_id,
        model: u.model,
        agent: u.agent,
        speed: u.speed,
        service_tier: u.service_tier,
        from_at: u.from_at,
        to_at: u.to_at,
        turns: u.turns,
        input_tokens: u.input_tokens,
        output_tokens: u.output_tokens,
        cache_read_tokens: u.cache_read_tokens,
        cache_write5m_tokens: u.cache_write5m_tokens,
        cache_write1h_tokens: u.cache_write1h_tokens,
        web_search_requests: u.web_search_requests,
        web_fetch_requests: u.web_fetch_requests,
        cost_micro_usd: u.cost_micro_usd,
        max_context: u.max_context,
        buckets: u.buckets,
        sent_at: u.sent_at,
        received_at: u.received_at,
        clock_skew: u.clock_skew,
        status,
    })
}

/// 区間の始めと終わりを、DB に残す字（UTC・ミリ秒まで・24 字。字の順が時刻の順）にする。始めが終わりより後の物と、
/// 同じ UTC の時（年月日と時が同じ）の内でない物は、その訳。
fn span(input: &PutUsageInput) -> Result<(String, String), String> {
    let from_at = stored_time("fromAt", &input.from_at)?;
    let to_at = stored_time("toAt", &input.to_at)?;
    if from_at > to_at {
        return Err(format!(
            "fromAt（{from_at}）は、toAt（{to_at}）より後にしない"
        ));
    }
    if from_at[..HOUR] != to_at[..HOUR] {
        return Err(format!(
            "fromAt（{from_at}）と toAt（{to_at}）は、同じ UTC の時（年月日と時が同じ）の内にする"
        ));
    }
    Ok((from_at, to_at))
}

/// 段の並びを bucket の順にする。同じ段が 2 つある物と、和が行の turns・costMicroUsd と合わない物は、その訳。
fn buckets(input: &PutUsageInput) -> Result<Vec<Bucket>, String> {
    let mut seen = HashSet::new();
    if let Some(twice) = input.buckets.iter().find(|b| !seen.insert(b.bucket)) {
        return Err(format!(
            "buckets に、同じ bucket（{}）が 2 つある",
            twice.bucket
        ));
    }
    let sum = |f: fn(&UsageBucket) -> i64| -> i128 {
        input.buckets.iter().map(|b| i128::from(f(b))).sum()
    };
    for (name, sum, row) in [
        ("turns", sum(|b| b.turns), input.turns),
        (
            "costMicroUsd",
            sum(|b| b.cost_micro_usd),
            input.cost_micro_usd,
        ),
    ] {
        if sum != i128::from(row) {
            return Err(format!(
                "buckets の {name} の和（{sum}）が、行の {name}（{row}）と合わない"
            ));
        }
    }
    let mut buckets: Vec<Bucket> = input
        .buckets
        .iter()
        .map(|b| Bucket {
            bucket: i64::from(b.bucket),
            turns: b.turns,
            cost_micro_usd: b.cost_micro_usd,
        })
        .collect();
    buckets.sort_by_key(|b| b.bucket);
    Ok(buckets)
}
