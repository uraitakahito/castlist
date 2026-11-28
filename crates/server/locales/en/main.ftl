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

## Actors (src/actor.rs, lineage.rs, actor_settings.rs, register.rs)

actor-not-found = Actor { $id } does not exist
register-needs-binding = bindings needs at least one entry (the first one is the key)
setting-time-range = time must be a time between year 0001 and year 9999

## Time and filter arguments (src/lib.rs)

time-range = { $name } must be a time between year 0000 and year 9999
only-true = { $name } accepts only true (omit it to not filter)
sent-at-out-of-range = sentAt is outside the supported time range

## Actor search (src/list_actors.rs)

actors-need-filter = Give at least one filter (one of label, binding, activeFrom, activeTo, active=true, model, effort, launchedBy, skewed)
actors-at-needs-binding = at must be given together with binding (to search by active period, use activeFrom and activeTo)
actors-token-other-order = Pass a nextToken from an answer with the same sort and order

## Page tokens (src/token.rs). One message per search, because word order differs between languages

token-broken = Cannot read nextToken (pass the nextToken from the previous answer unchanged)
token-other-actors = Cannot read nextToken (it is not a nextToken of the actor search)
token-other-events = Cannot read nextToken (it is not a nextToken of the event search)
token-other-settings = Cannot read nextToken (it is not a nextToken of the model and effort search)
token-other-usage = Cannot read nextToken (it is not a nextToken of the usage search)

## Putting events, links, model and effort, usage (src/event.rs, db.rs, usage.rs, list_usage.rs)

event-detail-object = detail must be a JSON object ({"{"}…{"}"})
ref-not-found = The actor in { $field } ({ $ref }) is not among the key bindings of issuer { $issuer }
conflict-event = An event with the same id ({ $id }) already exists with different content
conflict-link = A link with the same id ({ $id }) already exists with different content
conflict-setting = A model and effort record with the same id ({ $id }) already exists with different content
conflict-usage = A token usage record with the same id ({ $id }) already exists with different content
link-same-actor = from and to are the same actor ({ $actor })
link-already-launched = { $actor } has already been launched by another link
usage-span-too-long = The range from from to to is limited to 31 days
usage-from-after-to = fromAt ({ $fromAt }) must not be after toAt ({ $toAt })
usage-not-same-hour = fromAt ({ $fromAt }) and toAt ({ $toAt }) must be within the same UTC hour (same date and hour)
usage-bucket-twice = buckets has the same bucket ({ $bucket }) twice
usage-bucket-sum = The sum of { $name } in buckets ({ $sum }) does not match the row's { $name } ({ $row })

## The binary (src/main.rs). There is no request, so it follows the environment (LC_ALL, then LC_MESSAGES, then LANG)

usage = Usage: castlist-server (starts from the environment variables CASTLIST_DB and CASTLIST_LISTEN)
