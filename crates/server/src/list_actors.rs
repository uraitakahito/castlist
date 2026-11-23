//! ListActors: アクターを条件で探す（監査の検索）。値の形は定義（model/）が見てあるので、ここでは組み合わせと頁の印を見る。
use std::sync::Arc;

use castlist_sdk::error::ListActorsError;
use castlist_sdk::input::ListActorsInput;
use castlist_sdk::output::ListActorsOutput;

use crate::db::Db;
use crate::errors::Failure;

const OPERATION: &str = "ListActors";

pub async fn list_actors(
    _db: Arc<Db>,
    _input: ListActorsInput,
) -> Result<ListActorsOutput, ListActorsError> {
    Err(Failure::data(OPERATION, &"まだ無い").into())
}
