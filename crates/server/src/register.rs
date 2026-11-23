//! RegisterActor: アクターを登録する。形は定義（model/）が見てあるので、ここでは DB に渡して、答えの形に直す。
use std::sync::Arc;

use castlist_sdk::error::{RegisterActorError, ValidationException};
use castlist_sdk::input::RegisterActorInput;
use castlist_sdk::model::ActorSummary;
use castlist_sdk::output::RegisterActorOutput;

use crate::db::{Db, NewActor, Ref, WriteError};
use crate::errors::{busy, internal};

pub async fn register(
    db: Arc<Db>,
    input: RegisterActorInput,
) -> Result<RegisterActorOutput, RegisterActorError> {
    let sent_at = crate::sent_time(&input.sent_at).map_err(invalid)?;
    // 札は鍵の順に書く（同じ本文なら、行の並びも同じになる）。
    let mut labels: Vec<(String, String)> = input.labels.unwrap_or_default().into_iter().collect();
    labels.sort();
    let bindings: Vec<Ref> = input
        .bindings
        .into_iter()
        .map(|b| Ref {
            kind: b.r#type,
            value: b.value,
        })
        .collect();
    let (actor, created) = db
        .register(&NewActor {
            issuer: &input.issuer,
            kind: &input.kind,
            prefix: &input.name_prefix,
            labels: &labels,
            bindings: &bindings,
            sent_at,
            settings: input
                .settings
                .as_ref()
                .map(|s| (s.model.as_str(), s.effort.as_str())),
        })
        .await
        .map_err(|e| match e {
            WriteError::Busy => busy().into(),
            WriteError::Invalid(message) => invalid(message),
            // 登録は同じ鍵なら前のアクターを返すので、409 にはならない（なれば中の失敗）。
            WriteError::Conflict(message) => internal("RegisterActor", &message).into(),
            WriteError::Db(e) => internal("RegisterActor", &e).into(),
        })?;
    Ok(RegisterActorOutput {
        actor: ActorSummary {
            id: actor.id,
            name: actor.name,
            kind: actor.kind,
        },
        created,
        status: if created { 201 } else { 200 },
    })
}

/// 定義の形では言えない、中身の誤り（400 ValidationException）。
fn invalid(message: impl Into<String>) -> RegisterActorError {
    ValidationException {
        message: message.into(),
        field_list: None,
    }
    .into()
}
