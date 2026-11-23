-- アクターの台帳（新しい server。AI-36）。今の server の移行（migrations/ の 0001〜0003）を当てた後の形を、1 つにまとめた物。
-- 表と列の名は今の物のまま（今の DB の行を、この形へ移すため）。
-- どの表も足すだけ（書き替えも消しもしない。下の trigger が断る）。
-- 時刻: sent_at は送り手の時刻（RFC 3339）。received_at は castlist の時計（UTC・ミリ秒まで・24 字）で、
-- 書く transaction の中で、どの表の最新よりも前にならないように決める（crates/server/src/db.rs）。
-- 「最新」は seq（大きい方）だけで決める。sent_at では決めない。時刻の窓で探すのは received_at。
-- clock_skew: sent_at と received_at が 5 分より離れていれば 1（受けはする）。
-- 外のキーは、消す・替える時に断る（restrict）。連れて消す（cascade）・空にする（set null）は使わない。

-- アクター。id は castlist が出す UUID v7。名は <prefix>.<n>（n は出し手と接頭辞ごとに 1 から）。
create table actors(
  seq         integer primary key autoincrement,
  id          text not null unique,
  issuer      text not null,
  kind        text not null check(kind in ('ai', 'human')),
  prefix      text not null,
  n           integer not null check(n >= 1),
  name        text not null,
  sent_at     text not null,
  received_at text not null,
  clock_skew  integer not null check(clock_skew in (0, 1)),
  unique(issuer, name),
  unique(issuer, prefix, n)
) strict;
create index actors_received on actors(received_at, id);

-- 札（project など）。登録の時に 1 度だけ書く。
create table labels(
  seq         integer primary key autoincrement,
  actor_id    text not null references actors(id) on delete restrict on update restrict,
  key         text not null,
  value       text not null,
  received_at text not null,
  unique(actor_id, key)
) strict;
create index labels_key_value on labels(key, value, actor_id);

-- 付随する id（claude.session・tpm.person・cmux.workspace.ref など）。
-- 鍵（登録の初めの物）は 1 つのアクターにだけ付く。鍵でない物は、そのアクターの同じ種類の最新と違う時だけ足す。
create table bindings(
  seq         integer primary key autoincrement,
  actor_id    text not null references actors(id) on delete restrict on update restrict,
  issuer      text not null,
  type        text not null,
  value       text not null,
  is_key      integer not null check(is_key in (0, 1)),
  sent_at     text not null,
  received_at text not null
) strict;
create unique index bindings_key on bindings(issuer, type, value) where is_key = 1;
create index bindings_actor_type on bindings(actor_id, type, seq);
create index bindings_type_value on bindings(type, value, received_at);

-- 線: 誰が誰を起こしたか（launched）・誰の後を継いだか（continued）。起こされるのは 1 度だけ。
create table links(
  seq         integer primary key autoincrement,
  id          text not null unique,
  type        text not null check(type in ('launched', 'continued')),
  from_actor  text not null references actors(id) on delete restrict on update restrict,
  to_actor    text not null references actors(id) on delete restrict on update restrict,
  sent_at     text not null,
  received_at text not null,
  clock_skew  integer not null check(clock_skew in (0, 1)),
  body_hash   text not null
) strict;
create unique index links_launched on links(to_actor) where type = 'launched';
create index links_to on links(to_actor, type);
create index links_from on links(from_actor, type, seq);

-- 出来事: 登場・退場など。
create table events(
  seq         integer primary key autoincrement,
  id          text not null unique,
  actor_id    text not null references actors(id) on delete restrict on update restrict,
  type        text not null check(type in ('entered', 'reentered', 'forked', 'compacted', 'cleared',
                                           'exited', 'closed', 'reported_in', 'model_unreadable', 'retracted')),
  detail      text not null default '{}' check(json_valid(detail) and json_type(detail) = 'object'),
  sent_at     text not null,
  received_at text not null,
  clock_skew  integer not null check(clock_skew in (0, 1)),
  body_hash   text not null
) strict;
create index events_actor_type on events(actor_id, type, seq);
create index events_type_received on events(type, received_at);

-- model と effort の名の一覧（master）。知らない名は、初めて来た時に足す。
create table models(
  id            integer primary key autoincrement,
  name          text not null unique check(length(name) between 1 and 64 and name not glob '*[^a-z0-9.-]*'),
  first_seen_at text not null
) strict;
create table efforts(
  id            integer primary key autoincrement,
  name          text not null unique check(length(name) between 1 and 64 and name not glob '*[^a-z0-9.-]*'),
  first_seen_at text not null
) strict;
insert into efforts(name, first_seen_at)
select column1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
from (values ('low'), ('medium'), ('high'), ('xhigh'), ('max'));

-- model と effort: 頼んだ物（requested・登録の時）と、見た物（observed）。
-- id は送り手の送りの id（登録の時の物には無い）。body_hash は id のある物だけ。
create table settings(
  seq         integer primary key autoincrement,
  id          text unique,
  actor_id    text not null references actors(id) on delete restrict on update restrict,
  model_id    integer not null references models(id) on delete restrict on update restrict,
  effort_id   integer not null references efforts(id) on delete restrict on update restrict,
  source      text not null check(source in ('requested', 'observed')),
  sent_at     text not null,
  received_at text not null,
  clock_skew  integer not null check(clock_skew in (0, 1)),
  body_hash   text
) strict;
create index settings_actor on settings(actor_id, seq);
create index settings_model on settings(model_id, received_at);
create index settings_effort on settings(effort_id, received_at);

-- 監査の検索のための索引（今の server の 0002）。上の索引と重なる物は足さない。
-- 送り手の時計がずれた行（skewed=true）。行の id（seq）が後ろに付くので、clock_skew = 1 の行を受けた順に引ける。
create index actors_skew on actors(clock_skew);
create index events_skew on events(clock_skew);
create index settings_skew on settings(clock_skew);
-- 受けた時刻の窓（from・to）だけで、出来事と model と effort を引く。
create index events_received on events(received_at);
create index settings_received on settings(received_at);

-- 足すだけ: 書き替え（update・on conflict do update）と消し（delete・insert or replace）を断る。
-- insert or replace の消しで trigger が動くよう、接続ごとに recursive_triggers を入れている（crates/server/src/db.rs）。
create trigger actors_no_update before update on actors begin select raise(abort, 'append-only: actors'); end;
create trigger actors_no_delete before delete on actors begin select raise(abort, 'append-only: actors'); end;
create trigger labels_no_update before update on labels begin select raise(abort, 'append-only: labels'); end;
create trigger labels_no_delete before delete on labels begin select raise(abort, 'append-only: labels'); end;
create trigger bindings_no_update before update on bindings begin select raise(abort, 'append-only: bindings'); end;
create trigger bindings_no_delete before delete on bindings begin select raise(abort, 'append-only: bindings'); end;
create trigger links_no_update before update on links begin select raise(abort, 'append-only: links'); end;
create trigger links_no_delete before delete on links begin select raise(abort, 'append-only: links'); end;
create trigger events_no_update before update on events begin select raise(abort, 'append-only: events'); end;
create trigger events_no_delete before delete on events begin select raise(abort, 'append-only: events'); end;
create trigger models_no_update before update on models begin select raise(abort, 'append-only: models'); end;
create trigger models_no_delete before delete on models begin select raise(abort, 'append-only: models'); end;
create trigger efforts_no_update before update on efforts begin select raise(abort, 'append-only: efforts'); end;
create trigger efforts_no_delete before delete on efforts begin select raise(abort, 'append-only: efforts'); end;
create trigger settings_no_update before update on settings begin select raise(abort, 'append-only: settings'); end;
create trigger settings_no_delete before delete on settings begin select raise(abort, 'append-only: settings'); end;

-- 居るか: 居る・居ないを替える出来事の最新（seq で）。
-- entered・reentered・forked なら居る。cleared・exited・closed なら居ない。
-- 索引を使うよう、どれもアクター 1 つごとの「order by seq desc limit 1」で書く。
create view actor_state as
select actor_id, presence,
       coalesce(presence in ('entered', 'reentered', 'forked'), 0) as present,
       entered_at, exited_at
from (
  select a.id as actor_id,
         (select e.type from events e
          where e.actor_id = a.id
            and e.type in ('entered', 'reentered', 'forked', 'cleared', 'exited', 'closed')
          order by e.seq desc limit 1) as presence,
         (select e.received_at from events e
          where e.actor_id = a.id and e.type = 'entered'
          order by e.seq limit 1) as entered_at,
         (select e.received_at from events e
          where e.actor_id = a.id and e.type in ('cleared', 'exited', 'closed')
          order by e.seq desc limit 1) as exited_at
  from actors a
);

-- 今の model と effort: アクターごとの settings の最新（seq で）。
create view actor_current_settings as
select a.id as actor_id, s.seq, m.name as model, f.name as effort, s.source, s.sent_at, s.received_at
from actors a
join settings s on s.seq = (select s2.seq from settings s2 where s2.actor_id = a.id order by s2.seq desc limit 1)
join models m on m.id = s.model_id
join efforts f on f.id = s.effort_id;

-- 今の付随する id: アクターと種類ごとの最新（seq で）。
create view actor_current_bindings as
select b.actor_id, b.issuer, b.type, b.value, b.is_key, b.sent_at, b.received_at, b.seq
from bindings b
where b.seq = (select b2.seq from bindings b2
               where b2.actor_id = b.actor_id and b2.type = b.type
               order by b2.seq desc limit 1);
