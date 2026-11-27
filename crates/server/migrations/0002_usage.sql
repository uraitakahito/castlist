-- アクターごとのトークンの数と費え（t-j4pp。castlist 2.1.0 から）。0001_init.sql の表と同じ決まり（足すだけ・seq・外のキーは restrict・strict）。
-- 1 行 = アクター 1 人の、時計の 1 時間（UTC の時）の内の、同じ種類（model・agent・speed・service_tier）の回のまとめ。
-- 同じアクター・同じ時・同じ種類の行は、2 つ以上在りうる（送り手が、増えた分を新しい行で送る）。
-- 読む側は、数（turns・トークン・web の検索と取得の回数・cost_micro_usd・buckets の段ごとの数）を足し、max_context は大きい方を取る。
-- id は送り手の id で、body_hash は本文の写し（同じ id で違う本文なら 409）。
-- from_at・to_at は、区間の始めの回と終わりの回の時刻（送り手の時計）。received_at と同じ形（UTC・ミリ秒まで・24 字）で残すので、
-- 字の順が時刻の順になる。同じ UTC の時の内で from_at <= to_at（API が見る）。
-- agent は手伝い（subagent）の種類の名で、本体の回なら null。
-- buckets は、文脈の長さの段ごとの数の JSON の配列（[{"bucket":n,"costMicroUsd":…,"turns":…}] を bucket の順に。
-- n 段は n×25,000 以上 (n+1)×25,000 未満のトークン、40 は 1,000,000 以上。中身の形と、和が行の数と合うことは API が見る）。
create table usage(
  seq                   integer primary key autoincrement,
  id                    text not null unique,
  actor_id              text not null references actors(id) on delete restrict on update restrict,
  model_id              integer not null references models(id) on delete restrict on update restrict,
  agent                 text check(agent is null or length(agent) >= 1),
  speed                 text not null check(length(speed) >= 1),
  service_tier          text not null check(length(service_tier) >= 1),
  from_at               text not null,
  to_at                 text not null,
  turns                 integer not null check(turns >= 1),
  input_tokens          integer not null check(input_tokens >= 0),
  output_tokens         integer not null check(output_tokens >= 0),
  cache_read_tokens     integer not null check(cache_read_tokens >= 0),
  cache_write_5m_tokens integer not null check(cache_write_5m_tokens >= 0),
  cache_write_1h_tokens integer not null check(cache_write_1h_tokens >= 0),
  web_search_requests   integer not null check(web_search_requests >= 0),
  web_fetch_requests    integer not null check(web_fetch_requests >= 0),
  cost_micro_usd        integer not null check(cost_micro_usd >= 0),
  max_context           integer not null check(max_context >= 0),
  buckets               text not null check(json_valid(buckets) and json_type(buckets) = 'array'),
  sent_at               text not null,
  received_at           text not null,
  clock_skew            integer not null check(clock_skew in (0, 1)),
  body_hash             text not null
) strict;
-- アクターごとに、受けた順で引く。
create index usage_actor on usage(actor_id, seq);
-- 区間の終わりの時刻の窓（from <= to_at < to）で引く。
create index usage_to on usage(to_at);

-- 足すだけ（0001_init.sql の trigger と同じ）。
create trigger usage_no_update before update on usage begin select raise(abort, 'append-only: usage'); end;
create trigger usage_no_delete before delete on usage begin select raise(abort, 'append-only: usage'); end;
