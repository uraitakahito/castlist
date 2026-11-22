// ci/check-sql.sh が当てるべき SQL（CI が「落ちる」ことを見る。build には入らない）。
const A: &str = "update actors set name = 'x' where seq = 1";
const B: &str = "DELETE FROM events where id = 'e-1'";
const C: &str = "insert or replace into labels(actor_id, key, value) values('a', 'k', 'v')";
const D: &str = "Replace Into models(name) values('m')";
const E: &str = "insert into settings(id) values('s') on conflict(id) do
    update set source = excluded.source";
const F: &str = "update
    bindings
    set value = 'v'";
