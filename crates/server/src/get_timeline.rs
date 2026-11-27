//! GetTimeline: タイムラインを描く元の値（GetGraph と同じ組の点と線に、その点の出来事と、model と effort の行の全部を足す）。
//! 値の形（ラベルは `鍵:値`・時刻）は定義（model/）が見てある。並べる計算は、呼ぶ側がする。
use std::sync::Arc;

use castlist_sdk::error::{GetTimelineError, ValidationException};
use castlist_sdk::input::GetTimelineInput;
use castlist_sdk::output::GetTimelineOutput;

use crate::db::{self, Db, Ref};
use crate::errors::Failure;
use crate::graph::GraphQuery;
use crate::{datetime, rows, stored_time};

const OPERATION: &str = "GetTimeline";

pub async fn get_timeline(
    db: Arc<Db>,
    input: GetTimelineInput,
) -> Result<GetTimelineOutput, GetTimelineError> {
    let as_of = db::now();
    let time = |name: &str, t: Option<&aws_smithy_types::DateTime>| {
        t.map(|t| stored_time(name, t))
            .transpose()
            .map_err(|message| {
                GetTimelineError::from(ValidationException {
                    message,
                    field_list: None,
                })
            })
    };
    let query = GraphQuery {
        label: input.label.as_deref().map(|l| {
            let r = Ref::parse(l);
            (r.kind, r.value)
        }),
        from: time("from", input.from.as_ref())?.unwrap_or_default(),
        to: time("to", input.to.as_ref())?.unwrap_or_else(|| "9999".to_owned()),
    };
    let timeline = db
        .timeline(&query)
        .await
        .map_err(|e| Failure::db(OPERATION, e))?;
    let data = |e: String| Failure::data(OPERATION, &e);
    Ok(GetTimelineOutput {
        as_of: datetime(&as_of).map_err(data)?,
        nodes: timeline
            .graph
            .nodes
            .into_iter()
            .map(|n| rows::graph_node(n).map_err(data))
            .collect::<Result<_, _>>()?,
        edges: timeline
            .graph
            .edges
            .into_iter()
            .map(|l| rows::link(l).map_err(data))
            .collect::<Result<_, _>>()?,
        events: timeline
            .events
            .into_iter()
            .map(|m| rows::timeline_event(m).map_err(data))
            .collect::<Result<_, _>>()?,
        settings: timeline
            .settings
            .into_iter()
            .map(|s| rows::setting(s).map_err(data))
            .collect::<Result<_, _>>()?,
    })
}
