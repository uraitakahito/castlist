# Messages castlist returns to callers (English). Keys are `<where>-<what>`. Japanese lives in ../ja/main.ftl
# (same keys and the same variable names; a test checks both). Wording follows callboard's glossary (docs/src/ui.md).

## Guard layer (src/guard.rs)

guard-host = Only Host 127.0.0.1 or localhost is accepted
guard-origin = Pages in the browser cannot write
guard-timeout-seconds = Took too long ({ $n } s). Wait a moment and send again
guard-timeout-ms = Took too long ({ $n } ms). Wait a moment and send again
guard-too-large = The body is limited to { $mib } MiB
guard-unreadable-body = Cannot read the body

## Errors common to every operation (src/errors.rs)

internal = castlist failed internally (see the log)
busy = Busy. Wait a moment and send again
