$version: "2"

// castlist（アクターの台帳）の API。docs/src/api.md の決まりを、定義として書く。
namespace ai.castlist

use ai.common#DateTime
use aws.protocols#restJson1
use smithy.test#httpRequestTests
use smithy.test#httpResponseTests

@restJson1
@title("castlist")
service Castlist {
    version: "1.0"
    operations: [GetHealth]
}

/// 動いているかと、castlist の今の時刻（送り手が、自分の時計との差を測る）。
@readonly
@http(method: "GET", uri: "/healthz")
@httpRequestTests([
    {
        id: "GetHealthRequest"
        documentation: "本文も引数も無い"
        protocol: restJson1
        method: "GET"
        uri: "/healthz"
    }
])
@httpResponseTests([
    {
        id: "GetHealthResponse"
        documentation: "状態・版・今の時刻を返す"
        protocol: restJson1
        code: 200
        headers: { "Content-Type": "application/json" }
        body: """
            {"status":"ok","version":"1.0.0","now":"2026-11-24T00:00:00Z"}"""
        bodyMediaType: "application/json"
        params: { status: "ok", version: "1.0.0", now: 1795478400 }
    }
])
operation GetHealth {
    input := {}

    output := {
        /// いつも `ok`。
        @required
        status: String

        /// castlist の版。
        @required
        version: String

        /// castlist の今の時刻。
        @required
        now: DateTime
    }
}
