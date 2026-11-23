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

pub async fn get_graph(db: Arc<Db>, input: GetGraphInput) -> Result<GetGraphOutput, GetGraphError> {
    let as_of = db::now();
    let time = |name: &str, t: Option<&aws_smithy_types::DateTime>| {
        t.map(|t| stored_time(name, t))
            .transpose()
            .map_err(|message| {
                GetGraphError::from(ValidationException {
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
