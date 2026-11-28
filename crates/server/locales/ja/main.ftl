# castlist が外に出す文（日本語）。鍵は `<所>-<中身>`。英語は ../en/main.ftl（同じ鍵と、同じ値の名を持つ。試験が見る）。
# 日本語の字は、roster と担当が読む。字を替える時は、CHANGELOG に書く（roster は、いつも日本語を頼む）。

## 守りの層（src/guard.rs）

guard-host = Host は 127.0.0.1 か localhost だけを受ける
guard-origin = browser の中のページからは書けない
guard-timeout-seconds = 時間がかかりすぎた（{ $n } 秒）。少し待ってから送り直す
guard-timeout-ms = 時間がかかりすぎた（{ $n } ms）。少し待ってから送り直す
guard-too-large = 本文は { $mib } MiB まで
guard-unreadable-body = 本文を読めない

## どの操作にも共通の誤り（src/errors.rs）

internal = castlist の中で失敗した（記録を見る）
busy = 混んでいる。少し待ってから送り直す
