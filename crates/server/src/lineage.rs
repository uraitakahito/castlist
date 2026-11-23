//! GetLineage: 起こした・継いだ線を上り下りする。値の形は定義（model/）が見てある。
use std::sync::Arc;

use castlist_sdk::error::{GetLineageError, NotFound};
use castlist_sdk::input::GetLineageInput;
use castlist_sdk::output::GetLineageOutput;

use crate::db::{self, Db};
use crate::errors::Failure;
use crate::search::MAX_DEPTH;
use crate::{datetime, rows};

const OPERATION: &str = "GetLineage";

pub async fn get_lineage(
    db: Arc<Db>,
    input: GetLineageInput,
) -> Result<GetLineageOutput, GetLineageError> {
    let direction = input.direction.as_deref().unwrap_or("both");
    // 深さは定義が 1〜20 に収めてある。無ければ上は根まで（20 段まで）、下は 1 段。
    let depth = input.depth.and_then(|d| u32::try_from(d).ok());
    let up = if direction == "down" {
        0
    } else {
        depth.unwrap_or(MAX_DEPTH)
    };
    let down = if direction == "up" {
        0
    } else {
        depth.unwrap_or(1)
    };
    let as_of = db::now();
    let Some(lineage) = db
        .lineage(&input.actor_id, up, down, input.r#type.as_deref())
        .await
        .map_err(|e| Failure::db(OPERATION, e))?
    else {
        return Err(NotFound {
            message: format!("アクター {} は無い", input.actor_id),
        }
        .into());
    };
    let data = |e: String| Failure::data(OPERATION, &e);
    Ok(GetLineageOutput {
        as_of: datetime(&as_of).map_err(data)?,
        actor_id: input.actor_id,
        nodes: lineage
            .nodes
            .into_iter()
            .map(|a| rows::actor_row(a).map_err(data))
            .collect::<Result<_, _>>()?,
        edges: lineage
            .edges
            .into_iter()
            .map(|l| rows::link(l).map_err(data))
            .collect::<Result<_, _>>()?,
    })
}
