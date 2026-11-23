//! ResolveActors: 鍵の付随する id から、出し手の中のアクターを引く。参照の形は定義（ActorRef）が見てある。
use std::sync::Arc;

use castlist_sdk::error::ResolveActorsError;
use castlist_sdk::input::ResolveActorsInput;
use castlist_sdk::model::{ActorSummary, Resolution};
use castlist_sdk::output::ResolveActorsOutput;

use crate::datetime;
use crate::db::{self, Db, Ref};
use crate::errors::Failure;

const OPERATION: &str = "ResolveActors";

pub async fn resolve(
    db: Arc<Db>,
    input: ResolveActorsInput,
) -> Result<ResolveActorsOutput, ResolveActorsError> {
    // 種類は `:` を持たないので、初めの `:` で分ける（`:` の無い参照は定義が断ってある）。
    let refs: Vec<Ref> = input
        .refs
        .iter()
        .map(|r| {
            let (kind, value) = r.split_once(':').unwrap_or_default();
            Ref {
                kind: kind.to_owned(),
                value: value.to_owned(),
            }
        })
        .collect();
    let as_of = datetime(&db::now()).map_err(|e| Failure::data(OPERATION, &e))?;
    let found = db
        .resolve(&input.issuer, &refs)
        .await
        .map_err(|e| Failure::db(OPERATION, e))?;
    Ok(ResolveActorsOutput {
        as_of,
        results: input
            .refs
            .into_iter()
            .zip(found)
            .map(|(r, actor)| Resolution {
                r#ref: r,
                actor: actor.map(|a| ActorSummary {
                    id: a.id,
                    name: a.name,
                    kind: a.kind,
                }),
            })
            .collect(),
    })
}
