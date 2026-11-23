$version: "2"

// castlist（アクターの台帳）の API。docs/src/api.md の決まりを、定義として書く。
namespace ai.castlist

use ai.common#ActorKind
use ai.common#ActorSummary
use ai.common#DateTime
use ai.common#Unavailable
use aws.protocols#restJson1
use smithy.framework#ValidationException
use smithy.test#httpRequestTests
use smithy.test#httpResponseTests

@restJson1
@title("castlist")
service Castlist {
    version: "1.0"
    operations: [GetHealth, RegisterActor]
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

/// アクターを登録する。鍵（`bindings` の初めの物）が出し手（`issuer`）の中に既にあれば、そのアクターを返す（200・`created` は false）。
/// 無ければ新しく作る（201）。id は castlist が出す（UUID v7）。名は `<namePrefix>.<番号>`（番号は出し手と接頭辞ごとに 1 から）。
/// 既にあった時は、`kind`・`namePrefix`・`labels` を見ず、変わった鍵でない付随する id と、変わった頼んだ model と effort だけを足す。
@http(method: "POST", uri: "/api/actors")
@httpRequestTests([
    {
        id: "RegisterActorRequest"
        documentation: "欄の名は camelCase。初めの付随する id が鍵"
        protocol: restJson1
        method: "POST"
        uri: "/api/actors"
        headers: { "Content-Type": "application/json" }
        body: """
            {"issuer":"tpm","kind":"ai","namePrefix":"se","labels":{"project":"castlist"},"bindings":[{"type":"claude.session","value":"s-1"},{"type":"cmux.workspace.ref","value":"workspace:1"}],"sentAt":"2026-11-24T00:00:00Z","settings":{"model":"claude-opus-5-5","effort":"high"}}"""
        bodyMediaType: "application/json"
        params: {
            issuer: "tpm"
            kind: "ai"
            namePrefix: "se"
            labels: { project: "castlist" }
            bindings: [
                { type: "claude.session", value: "s-1" }
                { type: "cmux.workspace.ref", value: "workspace:1" }
            ]
            sentAt: 1795478400
            settings: { model: "claude-opus-5-5", effort: "high" }
        }
    }
    {
        id: "RegisterActorRequestWithoutLabelsAndSettings"
        documentation: "labels と settings は無くてよい"
        protocol: restJson1
        method: "POST"
        uri: "/api/actors"
        headers: { "Content-Type": "application/json" }
        body: """
            {"issuer":"tpm","kind":"human","namePrefix":"pm","bindings":[{"type":"tpm.person","value":"pm"}],"sentAt":"2026-11-24T00:00:00Z"}"""
        bodyMediaType: "application/json"
        params: {
            issuer: "tpm"
            kind: "human"
            namePrefix: "pm"
            bindings: [{ type: "tpm.person", value: "pm" }]
            sentAt: 1795478400
        }
    }
])
@httpResponseTests([
    {
        id: "RegisterActorCreated"
        documentation: "新しく作れば 201"
        protocol: restJson1
        code: 201
        headers: { "Content-Type": "application/json" }
        body: """
            {"actor":{"id":"019abcde-0000-7000-8000-000000000001","name":"se.1","kind":"ai"},"created":true}"""
        bodyMediaType: "application/json"
        params: {
            actor: { id: "019abcde-0000-7000-8000-000000000001", name: "se.1", kind: "ai" }
            created: true
            status: 201
        }
    }
    {
        id: "RegisterActorExisting"
        documentation: "鍵が既にあれば、そのアクターを 200 で返す"
        protocol: restJson1
        code: 200
        headers: { "Content-Type": "application/json" }
        body: """
            {"actor":{"id":"019abcde-0000-7000-8000-000000000001","name":"se.1","kind":"ai"},"created":false}"""
        bodyMediaType: "application/json"
        params: {
            actor: { id: "019abcde-0000-7000-8000-000000000001", name: "se.1", kind: "ai" }
            created: false
            status: 200
        }
    }
])
operation RegisterActor {
    input := {
        /// 出し手（tpm・pagetray など）。名の番号と鍵は、出し手ごとに分かれる。
        @required
        issuer: Name

        @required
        kind: ActorKind

        /// 名の接頭辞（名は `<接頭辞>.<番号>`）。
        @required
        namePrefix: NamePrefix

        /// 札（project など）。新しく作った時だけ書く。
        labels: Labels

        /// 付随する id。初めの物が鍵（1 つのアクターにだけ付く）。鍵でない物は、同じ種類の最新と違う時だけ足す。
        @required
        bindings: Bindings

        /// 送り手の時刻。
        @required
        sentAt: DateTime

        /// 頼んだ model と effort。最新と違う時だけ足す。
        settings: RequestedSettings
    }

    output := {
        @required
        actor: ActorSummary

        /// 新しく作ったか。
        @required
        created: Boolean

        /// 新しく作れば 201、鍵が既にあれば 200。
        @required
        @httpResponseCode
        status: Integer
    }

    errors: [
        ValidationException
        Unavailable
    ]
}

/// 出し手・付随する id の種類・ラベルの鍵・model・effort の名: 英小文字・数字・`-`・`.` で 64 字まで。
@pattern("^[a-z0-9.-]{1,64}$")
string Name

/// 名の接頭辞: 英小文字・数字・`-` で 32 字まで。
@pattern("^[a-z0-9-]{1,32}$")
string NamePrefix

/// 値: 空でなく、制御文字（Unicode の Cc）の無い、256 字までの字。
@length(min: 1, max: 256)
@pattern("^[^\\x00-\\x1F\\x7F-\\x9F]*$")
string Text

/// 札（鍵は名の形）。
map Labels {
    key: Name
    value: Text
}

/// 付随する id 1 つ（claude.session・tpm.person・cmux.workspace.ref など）。
structure Binding {
    @required
    type: Name

    @required
    value: Text
}

/// 付随する id の並び（1 つ以上。初めの物が鍵）。
@length(min: 1)
list Bindings {
    member: Binding
}

/// 頼んだ model と effort。
structure RequestedSettings {
    @required
    model: Name

    @required
    effort: Name
}
