//! DB の行を、答えの形に直す（GetActor の中の物と、Put… の答えと、図の点で同じ形。一覧の行は ListActors と GetLineage で同じ形。
//! 探した出来事と model と effort の行は、その形にアクターの名を足した物）。直せなければ、その訳の字を返す（呼ぶ側が中の失敗にする）。
use aws_smithy_types::{Document, Number};
use castlist_sdk::model::{
    ActorRow, ActorState, Event, EventRow, GraphNode, Link, Setting, SettingRow, TimelineEvent,
    Usage, UsageActor, UsageBucket,
};

use crate::datetime;
use crate::{db, graph, search};

pub fn event(e: db::Event) -> Result<Event, String> {
    let detail = serde_json::from_str(&e.detail)
        .map_err(|err| format!("出来事 {} の detail が読めない: {err}", e.id))?;
    Ok(Event {
        sent_at: datetime(&e.sent_at)?,
        received_at: datetime(&e.received_at)?,
        id: e.id,
        actor_id: e.actor_id,
        r#type: e.kind,
        detail: document(detail),
        clock_skew: e.clock_skew,
    })
}

pub fn setting(s: db::Setting) -> Result<Setting, String> {
    Ok(Setting {
        sent_at: datetime(&s.sent_at)?,
        received_at: datetime(&s.received_at)?,
        id: s.id,
        actor_id: s.actor_id,
        model: s.model,
        effort: s.effort,
        source: s.source,
        clock_skew: s.clock_skew,
    })
}

pub fn link(l: db::Link) -> Result<Link, String> {
    Ok(Link {
        sent_at: datetime(&l.sent_at)?,
        received_at: datetime(&l.received_at)?,
        id: l.id,
        r#type: l.kind,
        from: l.from_actor,
        to: l.to_actor,
        clock_skew: l.clock_skew,
    })
}

/// トークンの使用量の行を、答えの形にする（PutUsage の答えと、ListUsage の行で同じ形）。
pub fn usage(u: db::Usage) -> Result<Usage, String> {
    let broken = || {
        format!(
            "トークンの使用量 {} の buckets が読めない: {}",
            u.id, u.buckets
        )
    };
    let buckets: serde_json::Value = serde_json::from_str(&u.buckets).map_err(|_| broken())?;
    let buckets = buckets
        .as_array()
        .ok_or_else(broken)?
        .iter()
        .map(|b| {
            Some(UsageBucket {
                bucket: i32::try_from(b["bucket"].as_i64()?).ok()?,
                turns: b["turns"].as_i64()?,
                cost_micro_usd: b["costMicroUsd"].as_i64()?,
            })
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(broken)?;
    Ok(Usage {
        from_at: datetime(&u.from_at)?,
        to_at: datetime(&u.to_at)?,
        sent_at: datetime(&u.sent_at)?,
        received_at: datetime(&u.received_at)?,
        id: u.id,
        actor_id: u.actor_id,
        model: u.model,
        agent: u.agent,
        speed: u.speed,
        service_tier: u.service_tier,
        turns: u.turns,
        input_tokens: u.input_tokens,
        output_tokens: u.output_tokens,
        cache_read_tokens: u.cache_read_tokens,
        cache_write5m_tokens: u.cache_write_5m_tokens,
        cache_write1h_tokens: u.cache_write_1h_tokens,
        web_search_requests: u.web_search_requests,
        web_fetch_requests: u.web_fetch_requests,
        cost_micro_usd: u.cost_micro_usd,
        max_context: u.max_context,
        buckets,
        clock_skew: u.clock_skew,
    })
}

/// トークンの使用量の行に出てきたアクターを、答えの形にする。
pub fn usage_actor(a: search::UsageActor) -> UsageActor {
    UsageActor {
        id: a.id,
        name: a.name,
        labels: a.labels.into_iter().collect(),
    }
}

/// 今の状態を、答えの形にする（GetActor の state と、図の点で同じ形）。
pub fn state(s: db::State) -> Result<ActorState, String> {
    Ok(ActorState {
        presence: s.presence,
        present: s.present,
        inferred: s.inferred,
        entered_at: s.entered_at.as_deref().map(datetime).transpose()?,
        exited_at: s.exited_at.as_deref().map(datetime).transpose()?,
    })
}

/// 図の点を、答えの形にする。
pub fn graph_node(n: graph::Node) -> Result<GraphNode, String> {
    Ok(GraphNode {
        sent_at: datetime(&n.actor.sent_at)?,
        received_at: datetime(&n.actor.received_at)?,
        id: n.actor.id,
        name: n.actor.name,
        kind: n.actor.kind,
        issuer: n.actor.issuer,
        clock_skew: n.actor.clock_skew,
        state: state(n.state)?,
        current_settings: n.current_settings.map(setting).transpose()?,
    })
}

/// タイムラインの出来事を、答えの形にする。
pub fn timeline_event(m: graph::Moment) -> Result<TimelineEvent, String> {
    Ok(TimelineEvent {
        received_at: datetime(&m.received_at)?,
        actor_id: m.actor_id,
        r#type: m.kind,
        inferred: m.inferred,
        reason: m.reason,
    })
}

/// JSON の値を、答えの Document にする（数は、符号の無い整数・負の整数・小数のどれかに）。
fn document(value: serde_json::Value) -> Document {
    use serde_json::Value;
    match value {
        Value::Null => Document::Null,
        Value::Bool(b) => Document::Bool(b),
        Value::Number(n) => Document::Number(match (n.as_u64(), n.as_i64()) {
            (Some(u), _) => Number::PosInt(u),
            (None, Some(i)) => Number::NegInt(i),
            (None, None) => Number::Float(n.as_f64().unwrap_or(f64::NAN)),
        }),
        Value::String(s) => Document::String(s),
        Value::Array(items) => Document::Array(items.into_iter().map(document).collect()),
        Value::Object(map) => {
            Document::Object(map.into_iter().map(|(k, v)| (k, document(v))).collect())
        }
    }
}

/// 一覧の 1 行を、答えの形にする（ListActors の行と、GetLineage の点で同じ形）。
pub fn actor_row(a: search::ActorItem) -> Result<ActorRow, String> {
    Ok(ActorRow {
        sent_at: datetime(&a.sent_at)?,
        received_at: datetime(&a.received_at)?,
        first_entered_at: a.first_entered_at.as_deref().map(datetime).transpose()?,
        last_exited_at: a.last_exited_at.as_deref().map(datetime).transpose()?,
        url: format!("/actors/{}", a.id),
        id: a.id,
        name: a.name,
        kind: a.kind,
        issuer: a.issuer,
        clock_skew: a.clock_skew,
        labels: a.labels.into_iter().collect(),
        last_exit_type: a.last_exit_type,
        present: a.present,
        inferred: a.inferred,
        presence: a.presence,
        skewed: Some(a.skewed),
        model: a.model,
        effort: a.effort,
    })
}

/// 探した model と effort の 1 行を、答えの形にする（model と effort の形に、アクターの名を足した物）。
pub fn setting_row(item: search::SettingItem) -> Result<SettingRow, String> {
    let s = setting(item.setting)?;
    Ok(SettingRow {
        id: s.id,
        actor_id: s.actor_id,
        actor_name: item.actor_name,
        model: s.model,
        effort: s.effort,
        source: s.source,
        sent_at: s.sent_at,
        received_at: s.received_at,
        clock_skew: s.clock_skew,
    })
}

/// 探した出来事の 1 行を、答えの形にする（出来事の形に、アクターの名を足した物）。
pub fn event_row(item: search::EventItem) -> Result<EventRow, String> {
    let e = event(item.event)?;
    Ok(EventRow {
        id: e.id,
        actor_id: e.actor_id,
        actor_name: item.actor_name,
        r#type: e.r#type,
        detail: e.detail,
        sent_at: e.sent_at,
        received_at: e.received_at,
        clock_skew: e.clock_skew,
    })
}
