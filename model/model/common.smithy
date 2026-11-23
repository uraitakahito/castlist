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
