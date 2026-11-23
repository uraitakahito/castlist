$version: "2"

// pagetray と castlist が共に使う形（共通の名前空間）。castlist の repo に置き、pagetray と roster は release の tag で固めて取る。
namespace ai.common

/// 次の頁の印。中は読まず、前の答えの `nextToken` をそのまま渡す。
@length(min: 1, max: 512)
string NextToken

/// 1 頁の数。
@range(min: 1, max: 1000)
integer MaxResults

/// 時刻（RFC 3339）。
@timestampFormat("date-time")
timestamp DateTime

/// 名指した物が無い。
@error("client")
@httpError(404)
structure NotFound {
    @required
    message: String
}

/// 同じ id の物がもうあり、中身が違う。
@error("client")
@httpError(409)
structure Conflict {
    @required
    message: String
}

/// 消した物（番号は 2 度使わない）。
@error("client")
@httpError(410)
structure Gone {
    @required
    message: String
}

/// 混んでいる・時間がかかりすぎた・呼ぶ先が答えない。少し待って送り直す。
@error("server")
@httpError(503)
@retryable
structure Unavailable {
    @required
    message: String
}

/// アクターの id（castlist が出す UUID v7 の字。小文字の 16 進）。
@pattern("^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")
string ActorId

/// アクターの種類: `ai` か `human`。
/// enum にしないのは、smithy-rs（codegen-server 0.1.40）が、答えに要る enum のある操作の要求の例（`@httpRequestTests`）から、
/// 空の字の enum を作って落ちる試験を作るため。wire の形は enum と同じ字なので、直った版で enum に替えても壊す変わりにならない。
@pattern("^(ai|human)$")
string ActorKind

/// アクターを指す時の短い形。
structure ActorSummary {
    @required
    id: ActorId

    /// `<接頭辞>.<番号>`（番号は出し手と接頭辞ごとに 1 から）。
    @required
    name: String

    @required
    kind: ActorKind
}
