//! GetGraph: 図（タイムライン・相関図）の点と線。
use std::sync::Arc;

use castlist_sdk::error::GetGraphError;
use castlist_sdk::input::GetGraphInput;
use castlist_sdk::output::GetGraphOutput;

use crate::db::Db;
use crate::errors::Failure;

pub async fn get_graph(
    _db: Arc<Db>,
    _input: GetGraphInput,
) -> Result<GetGraphOutput, GetGraphError> {
    Err(Failure::data("GetGraph", &"まだ無い").into())
}
