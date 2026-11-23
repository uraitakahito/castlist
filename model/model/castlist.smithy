$version: "2"

// castlist（アクターの台帳）の API。docs/src/api.md の決まりを、定義として書く。
namespace ai.castlist

use ai.common#ActorId
use ai.common#ActorKind
use ai.common#ActorRef
use ai.common#ActorSummary
use ai.common#DateTime
use ai.common#InternalError
use ai.common#NotFound
use ai.common#Unavailable
use aws.protocols#restJson1
use smithy.framework#ValidationException
use smithy.test#httpRequestTests
use smithy.test#httpResponseTests

@restJson1
@title("castlist")
service Castlist {
    version: "1.0"
    operations: [GetHealth, RegisterActor, GetActor, ResolveActors]
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
        InternalError
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

/// 1 つのアクターの全部（札・付随する id・出来事・model と effort・線・今の状態）。1 つの読む transaction の中で、揃った写しを読む。
/// 札は鍵の順、ほかの並びは受けた順（seq）。`currentBindings` は種類ごとの最新を種類の順に、`currentSettings` は最新の 1 つ（無ければ無い）。
@readonly
@http(method: "GET", uri: "/api/actors/{actorId}")
@httpRequestTests([
    {
        id: "GetActorRequest"
        documentation: "アクターの id を道に入れる"
        protocol: restJson1
        method: "GET"
        uri: "/api/actors/019abcde-0000-7000-8000-000000000001"
        params: { actorId: "019abcde-0000-7000-8000-000000000001" }
    }
])
@httpResponseTests([
    {
        id: "GetActorResponse"
        documentation: "欄の名は camelCase。出来事の detail は送られた JSON の物のまま"
        protocol: restJson1
        code: 200
        headers: { "Content-Type": "application/json" }
        body: """
            {"id":"019abcde-0000-7000-8000-000000000001","name":"se.1","kind":"ai","issuer":"tpm","sentAt":"2026-11-24T00:00:00Z","receivedAt":"2026-11-24T00:00:01Z","clockSkew":false,"labels":{"project":"castlist"},"bindings":[{"issuer":"tpm","type":"claude.session","value":"s-1","isKey":true,"sentAt":"2026-11-24T00:00:00Z","receivedAt":"2026-11-24T00:00:01Z"}],"currentBindings":[{"issuer":"tpm","type":"claude.session","value":"s-1","isKey":true,"sentAt":"2026-11-24T00:00:00Z","receivedAt":"2026-11-24T00:00:01Z"}],"events":[{"id":"e-1","actorId":"019abcde-0000-7000-8000-000000000001","type":"closed","detail":{"inferred":true,"reason":"workspace_gone"},"sentAt":"2026-11-24T00:00:00Z","receivedAt":"2026-11-24T00:00:01Z","clockSkew":false}],"settings":[{"actorId":"019abcde-0000-7000-8000-000000000001","model":"claude-opus-5-5","effort":"high","source":"requested","sentAt":"2026-11-24T00:00:00Z","receivedAt":"2026-11-24T00:00:01Z","clockSkew":false}],"currentSettings":{"actorId":"019abcde-0000-7000-8000-000000000001","model":"claude-opus-5-5","effort":"high","source":"requested","sentAt":"2026-11-24T00:00:00Z","receivedAt":"2026-11-24T00:00:01Z","clockSkew":false},"linksIn":[{"id":"l-1","type":"launched","from":"019abcde-0000-7000-8000-000000000002","to":"019abcde-0000-7000-8000-000000000001","sentAt":"2026-11-24T00:00:00Z","receivedAt":"2026-11-24T00:00:01Z","clockSkew":false}],"linksOut":[],"state":{"presence":"closed","present":false,"inferred":true,"exitedAt":"2026-11-24T00:00:01Z"},"asOf":"2026-11-24T00:00:02Z"}"""
        bodyMediaType: "application/json"
        params: {
            id: "019abcde-0000-7000-8000-000000000001"
            name: "se.1"
            kind: "ai"
            issuer: "tpm"
            sentAt: 1795478400
            receivedAt: 1795478401
            clockSkew: false
            labels: { project: "castlist" }
            bindings: [
                { issuer: "tpm", type: "claude.session", value: "s-1", isKey: true, sentAt: 1795478400, receivedAt: 1795478401 }
            ]
            currentBindings: [
                { issuer: "tpm", type: "claude.session", value: "s-1", isKey: true, sentAt: 1795478400, receivedAt: 1795478401 }
            ]
            events: [
                {
                    id: "e-1"
                    actorId: "019abcde-0000-7000-8000-000000000001"
                    type: "closed"
                    detail: { inferred: true, reason: "workspace_gone" }
                    sentAt: 1795478400
                    receivedAt: 1795478401
                    clockSkew: false
                }
            ]
            settings: [
                {
                    actorId: "019abcde-0000-7000-8000-000000000001"
                    model: "claude-opus-5-5"
                    effort: "high"
                    source: "requested"
                    sentAt: 1795478400
                    receivedAt: 1795478401
                    clockSkew: false
                }
            ]
            currentSettings: {
                actorId: "019abcde-0000-7000-8000-000000000001"
                model: "claude-opus-5-5"
                effort: "high"
                source: "requested"
                sentAt: 1795478400
                receivedAt: 1795478401
                clockSkew: false
            }
            linksIn: [
                {
                    id: "l-1"
                    type: "launched"
                    from: "019abcde-0000-7000-8000-000000000002"
                    to: "019abcde-0000-7000-8000-000000000001"
                    sentAt: 1795478400
                    receivedAt: 1795478401
                    clockSkew: false
                }
            ]
            linksOut: []
            state: { presence: "closed", present: false, inferred: true, exitedAt: 1795478401 }
            asOf: 1795478402
        }
    }
])
operation GetActor {
    input := {
        @required
        @httpLabel
        actorId: ActorId
    }

    output := {
        @required
        id: ActorId

        /// `<接頭辞>.<番号>`。
        @required
        name: String

        @required
        kind: ActorKind

        /// 出し手。
        @required
        issuer: Name

        /// 登録の送り手の時刻。
        @required
        sentAt: DateTime

        /// 登録を受けた castlist の時刻。
        @required
        receivedAt: DateTime

        /// 送り手の時刻と受けた時刻が 5 分より離れていたか。
        @required
        clockSkew: Boolean

        /// 札（登録の時に書いた物）。
        @required
        labels: Labels

        /// 付随する id の全部（受けた順）。
        @required
        bindings: ActorBindings

        /// 種類ごとの最新（種類の順）。
        @required
        currentBindings: ActorBindings

        /// 出来事の全部（受けた順）。
        @required
        events: Events

        /// model と effort の全部（受けた順）。
        @required
        settings: Settings

        /// model と effort の最新（受けた順で）。1 つも無ければ無い。
        currentSettings: Setting

        /// このアクターへの線（受けた順）。
        @required
        linksIn: Links

        /// このアクターからの線（受けた順）。
        @required
        linksOut: Links

        @required
        state: ActorState

        /// 読み始める前の castlist の時刻。
        @required
        asOf: DateTime
    }

    errors: [
        NotFound
        ValidationException
        Unavailable
        InternalError
    ]
}

/// 鍵の付随する id（`種類:値`）から、出し手の中のアクターを引く。答えは問いと同じ順で、引けない物（知らない・鍵でない・出し手が違う）は `actor` が無い。
/// 1 つでも形の違う参照があれば、全体を 400 で断る。
@http(method: "POST", uri: "/api/resolve")
@httpRequestTests([
    {
        id: "ResolveActorsRequest"
        documentation: "参照は初めの `:` で種類と値に分ける（値に `:` があってよい）"
        protocol: restJson1
        method: "POST"
        uri: "/api/resolve"
        headers: { "Content-Type": "application/json" }
        body: """
            {"issuer":"tpm","refs":["claude.session:s-1","cmux.workspace.ref:workspace:1"]}"""
        bodyMediaType: "application/json"
        params: { issuer: "tpm", refs: ["claude.session:s-1", "cmux.workspace.ref:workspace:1"] }
    }
])
@httpResponseTests([
    {
        id: "ResolveActorsResponse"
        documentation: "問いと同じ順。引けない物は actor が無い"
        protocol: restJson1
        code: 200
        headers: { "Content-Type": "application/json" }
        body: """
            {"asOf":"2026-11-24T00:00:00Z","results":[{"ref":"claude.session:s-1","actor":{"id":"019abcde-0000-7000-8000-000000000001","name":"se.1","kind":"ai"}},{"ref":"claude.session:none"}]}"""
        bodyMediaType: "application/json"
        params: {
            asOf: 1795478400
            results: [
                {
                    ref: "claude.session:s-1"
                    actor: { id: "019abcde-0000-7000-8000-000000000001", name: "se.1", kind: "ai" }
                }
                { ref: "claude.session:none" }
            ]
        }
    }
])
operation ResolveActors {
    input := {
        /// 出し手。鍵は出し手の中で引く。
        @required
        issuer: Name

        /// 引く参照（1 から 100 個）。
        @required
        refs: ActorRefs
    }

    output := {
        /// 読み始める前の castlist の時刻。
        @required
        asOf: DateTime

        /// 問いと同じ順。
        @required
        results: Resolutions
    }

    errors: [
        ValidationException
        Unavailable
        InternalError
    ]
}

/// 送り手が付ける id（出来事・線・model と effort）: 空白の無い ASCII の印字できる字で 128 字まで。
@pattern("^[\\x21-\\x7E]{1,128}$")
string SenderId

/// 出来事の種類（10 種）。
/// 居る・居ないを替えるのは entered・reentered・forked（居る）と cleared・exited・closed（居ない）。
@pattern("^(entered|reentered|forked|compacted|cleared|exited|closed|reported_in|model_unreadable|retracted)$")
string EventType

/// 線の種類: 起こした（`launched`）・後を継いだ（`continued`）。
@pattern("^(launched|continued)$")
string LinkType

/// model と effort の出: 頼んだ（`requested`）・見た（`observed`）。
@pattern("^(requested|observed)$")
string SettingSource

/// 付随する id 1 つ（行のまま）。
structure ActorBinding {
    @required
    issuer: Name

    @required
    type: Name

    @required
    value: Text

    /// 鍵か（登録の初めの物）。
    @required
    isKey: Boolean

    @required
    sentAt: DateTime

    @required
    receivedAt: DateTime
}

list ActorBindings {
    member: ActorBinding
}

/// 出来事 1 つ。
structure Event {
    @required
    id: SenderId

    @required
    actorId: ActorId

    @required
    type: EventType

    /// 送られた JSON の物（無ければ空の物）。
    @required
    detail: Document

    @required
    sentAt: DateTime

    @required
    receivedAt: DateTime

    @required
    clockSkew: Boolean
}

list Events {
    member: Event
}

/// model と effort 1 つ。
structure Setting {
    /// 送り手の id。登録の時に頼んだ物には無い。
    id: SenderId

    @required
    actorId: ActorId

    @required
    model: Name

    @required
    effort: Name

    @required
    source: SettingSource

    @required
    sentAt: DateTime

    @required
    receivedAt: DateTime

    @required
    clockSkew: Boolean
}

list Settings {
    member: Setting
}

/// 線 1 つ（`from` が `to` を起こした・`to` が `from` の後を継いだ）。
structure Link {
    @required
    id: SenderId

    @required
    type: LinkType

    @required
    from: ActorId

    @required
    to: ActorId

    @required
    sentAt: DateTime

    @required
    receivedAt: DateTime

    @required
    clockSkew: Boolean
}

list Links {
    member: Link
}

/// 今の状態。
structure ActorState {
    /// 居る・居ないを替える出来事（6 種）の最新の種類（受けた順で）。1 つも無ければ無い。
    presence: EventType

    /// 居るか（`presence` が entered・reentered・forked）。
    @required
    present: Boolean

    /// 今の停止が推し（最後の居る出来事より後の初めの退場が、`detail.inferred` が JSON の true の `closed`）か。居る間は false。
    @required
    inferred: Boolean

    /// 初めの `entered` を受けた時刻。
    enteredAt: DateTime

    /// 最後の退場（cleared・exited・closed）を受けた時刻。
    exitedAt: DateTime
}

/// 引く参照の並び（1 から 100 個）。
@length(min: 1, max: 100)
list ActorRefs {
    member: ActorRef
}

/// 参照 1 つの答え。
structure Resolution {
    /// 問いの参照のまま。
    @required
    ref: ActorRef

    /// 引けたアクター。引けなければ無い。
    actor: ActorSummary
}

list Resolutions {
    member: Resolution
}
