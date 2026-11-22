-- 送り手の時刻の列の名を at から sent_at にする（PM の答え。at だけでは誰の時刻か分からないため）。
-- 列の名を替えるだけで、行は書き替えない（台帳は足すだけのまま。trigger は update と delete の行を断る物で、
-- 列の名の替えには当たらない）。view（actor_current_settings・actor_current_bindings）の中の参照も、SQLite が
-- 一緒に替えるので、view の出す列も sent_at になる。
-- この移行を当てた後は、前の版（0.1.x）では開けない（前の版の SQL は at を読む）。戻す時は、上げる前の写しから。
alter table actors rename column at to sent_at;
alter table bindings rename column at to sent_at;
alter table links rename column at to sent_at;
alter table events rename column at to sent_at;
alter table settings rename column at to sent_at;
