//! GetGraph: 図（タイムライン・相関図）の点と線。値の形（ラベルは `鍵:値`・時刻）は定義（model/）が見てある。
use std::sync::Arc;

use castlist_sdk::error::{GetGraphError, ValidationException};
use castlist_sdk::input::GetGraphInput;
use castlist_sdk::output::GetGraphOutput;

use crate::db::{self, Db, Ref};
use crate::errors::Failure;
use crate::graph::GraphQuery;
use crate::{datetime, rows, stored_time};

const OPERATION: &str = "GetGraph";

/// 図の引数（ラベルと、登録を受けた時刻の窓）を、図の絞りにする（窓が無ければ全部）。扱えない時刻なら、その訳。
/// GetGraph と GetTimeline が共に使う（同じ引数からは、同じ組）。
pub fn query(
    label: Option<&str>,
    from: Option<&aws_smithy_types::DateTime>,
    to: Option<&aws_smithy_types::DateTime>,
) -> Result<GraphQuery, String> {
    Ok(GraphQuery {
        label: label.map(|l| {
            let r = Ref::parse(l);
            (r.kind, r.value)
        }),
        from: from
            .map(|t| stored_time("from", t))
            .transpose()?
            .unwrap_or_default(),
        to: to
            .map(|t| stored_time("to", t))
            .transpose()?
            .unwrap_or_else(|| "9999".to_owned()),
    })
}

pub async fn get_graph(db: Arc<Db>, input: GetGraphInput) -> Result<GetGraphOutput, GetGraphError> {
    let as_of = db::now();
    let query = query(
        input.label.as_deref(),
        input.from.as_ref(),
        input.to.as_ref(),
    )
    .map_err(|message| {
        GetGraphError::from(ValidationException {
            message,
            field_list: None,
        })
    })?;
    let graph = db
        .graph(&query)
        .await
        .map_err(|e| Failure::db(OPERATION, e))?;
    let data = |e: String| Failure::data(OPERATION, &e);
    Ok(GetGraphOutput {
        as_of: datetime(&as_of).map_err(data)?,
        nodes: graph
            .nodes
            .into_iter()
            .map(|n| rows::graph_node(n).map_err(data))
            .collect::<Result<_, _>>()?,
        edges: graph
            .edges
            .into_iter()
            .map(|l| rows::link(l).map_err(data))
            .collect::<Result<_, _>>()?,
    })
}
