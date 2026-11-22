-- 監査の検索（src/search.rs）のための索引。0001 に既にある物（labels(key, value, actor_id)・
-- bindings(type, value, received_at)・events(type, received_at)・events(actor_id, type, seq)・
-- links(to_actor, type)・links(from_actor, type, seq)・settings(model_id, received_at)・
-- settings(effort_id, received_at)・settings(actor_id, seq)・actors(received_at, id)）は、ここで重ねない。

-- 送り手の時計がずれた行（skewed=true）。行の id（seq）が後ろに付くので、clock_skew = 1 の行を受けた順に引ける。
create index actors_skew on actors(clock_skew);
create index events_skew on events(clock_skew);
create index settings_skew on settings(clock_skew);

-- 受けた時刻の窓（from・to）だけで、出来事と model と effort を引く。
create index events_received on events(received_at);
create index settings_received on settings(received_at);
