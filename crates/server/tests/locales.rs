//! 訳の file（`crates/server/locales/{en,ja}/main.ftl`）と、コードが引く鍵の突き合わせ（AI-48）。
//! - 日本語と英語は、同じ鍵と、同じ値の名を持つ。
//! - 英語の文に、かなと漢字が無い。
//! - コードが引く鍵は、どちらの言語にも在り、使わない鍵は無い。
//! - 日本語の文は、鍵へ移す前の字と 1 字も違わない（[`JA`] の写し。roster と担当が読む字を、移す時に変えないため）。
#![allow(clippy::disallowed_methods)]

use std::collections::{BTreeMap, BTreeSet};

use castlist::i18n::{Lang, text_in};

/// 訳の file の「鍵 → 文」。注（#）と空の行を除く（文は 1 行に書く決まり）。
fn messages(lang: &str) -> BTreeMap<String, String> {
    let path = format!("{}/locales/{lang}/main.ftl", env!("CARGO_MANIFEST_DIR"));
    let source = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    source
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .map(|line| {
            let (key, value) = line
                .split_once(" = ")
                .unwrap_or_else(|| panic!("{path}: 「鍵 = 文」の 1 行でない: {line}"));
            (key.to_owned(), value.to_owned())
        })
        .collect()
}

/// 文の中の値の名（`{ $name }`）。
fn variables(message: &str) -> BTreeSet<String> {
    message
        .split("{ $")
        .skip(1)
        .filter_map(|rest| rest.split_once(" }").map(|(name, _)| name.to_owned()))
        .collect()
}

#[test]
fn 日本語と英語は_同じ鍵と_同じ値の名を持つ() {
    let (en, ja) = (messages("en"), messages("ja"));
    assert_eq!(
        en.keys().collect::<Vec<_>>(),
        ja.keys().collect::<Vec<_>>(),
        "鍵がそろわない"
    );
    for (key, message) in &en {
        assert_eq!(variables(message), variables(&ja[key]), "{key}");
    }
}

#[test]
fn 英語の文に_かなと漢字が無い() {
    for (key, message) in messages("en") {
        let japanese = message.chars().any(|c| {
            matches!(c, '\u{3040}'..='\u{30ff}' | '\u{4e00}'..='\u{9fff}' | '\u{ff00}'..='\u{ffef}' | '\u{3000}'..='\u{303f}')
        });
        assert!(!japanese, "{key}: {message}");
    }
}

/// コードが引く鍵: `text(`・`text_with(`・`text_in(<言語>,` のすぐ後ろに書いた字（鍵は、いつも字でじかに書く決まり）。
/// src の単体の試験の中の鍵も入る。
fn used_keys() -> BTreeSet<String> {
    let dir = format!("{}/src", env!("CARGO_MANIFEST_DIR"));
    let mut keys = BTreeSet::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let source = std::fs::read_to_string(&path).unwrap();
        for call in ["text(", "text_with(", "text_in("] {
            for rest in source.split(call).skip(1) {
                let rest = rest.trim_start();
                let rest = ["Lang::En,", "Lang::Ja,", "lang,"]
                    .iter()
                    .find_map(|lang| rest.strip_prefix(lang))
                    .unwrap_or(rest)
                    .trim_start();
                // 鍵の形（英小文字・数字・-）の字だけを拾う（この関数の中の字のような、鍵でない物を除く）。
                let key = rest
                    .strip_prefix('"')
                    .and_then(|r| r.split_once('"'))
                    .map(|(key, _)| key)
                    .filter(|key| {
                        !key.is_empty()
                            && key
                                .chars()
                                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
                    });
                if let Some(key) = key {
                    keys.insert(key.to_owned());
                }
            }
        }
    }
    keys
}

#[test]
fn コードが引く鍵は_どちらの言語にも在り_使わない鍵は無い() {
    let used = used_keys();
    let known: BTreeSet<String> = messages("en").into_keys().collect();
    let missing: Vec<_> = used.difference(&known).collect();
    assert!(missing.is_empty(), "訳の file に無い鍵: {missing:?}");
    let unused: Vec<_> = known.difference(&used).collect();
    assert!(unused.is_empty(), "コードが引かない鍵: {unused:?}");
}

/// 鍵へ移す前（castlist 2.1.1）の日本語の字の写し: 鍵・見本の値・その値を入れた字。
/// 字を替える時は、ここと CHANGELOG を一緒に直す（roster は、この日本語を担当にそのまま出す）。
const JA: &[(&str, &[(&str, &str)], &str)] = &[
    (
        "guard-host",
        &[],
        "Host は 127.0.0.1 か localhost だけを受ける",
    ),
    ("guard-origin", &[], "browser の中のページからは書けない"),
    (
        "guard-timeout-seconds",
        &[("n", "5")],
        "時間がかかりすぎた（5 秒）。少し待ってから送り直す",
    ),
    (
        "guard-timeout-ms",
        &[("n", "200")],
        "時間がかかりすぎた（200 ms）。少し待ってから送り直す",
    ),
    ("guard-too-large", &[("mib", "2")], "本文は 2 MiB まで"),
    ("guard-unreadable-body", &[], "本文を読めない"),
    ("internal", &[], "castlist の中で失敗した（記録を見る）"),
    ("busy", &[], "混んでいる。少し待ってから送り直す"),
    ("actor-not-found", &[("id", "a-1")], "アクター a-1 は無い"),
    ("event-detail-object", &[], "detail は JSON の物（{…}）"),
    (
        "actors-need-filter",
        &[],
        "絞る条件を 1 つ以上付ける（label・binding・activeFrom・activeTo・active=true・model・effort・launchedBy・skewed のどれか）",
    ),
    (
        "actors-at-needs-binding",
        &[],
        "at は binding と一緒に付ける（稼働した期間で探すなら activeFrom・activeTo）",
    ),
    (
        "actors-token-other-order",
        &[],
        "nextToken は、同じ sort と order の答えの物を渡す",
    ),
    ("usage-span-too-long", &[], "from から to までは 31 日まで"),
    (
        "token-broken",
        &[],
        "nextToken が読めない（前の答えの nextToken を、そのまま渡す）",
    ),
    (
        "token-other-actors",
        &[],
        "nextToken が読めない（アクターの検索の nextToken ではない）",
    ),
    (
        "token-other-events",
        &[],
        "nextToken が読めない（出来事の検索の nextToken ではない）",
    ),
    (
        "token-other-settings",
        &[],
        "nextToken が読めない（model と effort の検索の nextToken ではない）",
    ),
    (
        "token-other-usage",
        &[],
        "nextToken が読めない（使用量の検索の nextToken ではない）",
    ),
    (
        "usage-from-after-to",
        &[("fromAt", "F"), ("toAt", "T")],
        "fromAt（F）は、toAt（T）より後にしない",
    ),
    (
        "usage-not-same-hour",
        &[("fromAt", "F"), ("toAt", "T")],
        "fromAt（F）と toAt（T）は、同じ UTC の時（年月日と時が同じ）の内にする",
    ),
    (
        "usage-bucket-twice",
        &[("bucket", "3")],
        "buckets に、同じ bucket（3）が 2 つある",
    ),
    (
        "usage-bucket-sum",
        &[("name", "turns"), ("sum", "4"), ("row", "5")],
        "buckets の turns の和（4）が、行の turns（5）と合わない",
    ),
    (
        "setting-time-range",
        &[],
        "time は 0001 年から 9999 年までの時刻",
    ),
    (
        "time-range",
        &[("name", "sentAt")],
        "sentAt は 0000 年から 9999 年までの時刻",
    ),
    (
        "only-true",
        &[("name", "skewed")],
        "skewed は true だけ（絞らないなら付けない）",
    ),
    ("sent-at-out-of-range", &[], "sentAt が扱える時刻の外"),
    (
        "register-needs-binding",
        &[],
        "bindings に 1 つ以上（初めの物が鍵）",
    ),
    (
        "conflict-event",
        &[("id", "e-1")],
        "同じ id（e-1）の出来事が、違う中身で既にある",
    ),
    (
        "conflict-link",
        &[("id", "l-1")],
        "同じ id（l-1）の線が、違う中身で既にある",
    ),
    (
        "conflict-setting",
        &[("id", "s-1")],
        "同じ id（s-1）の model と effort が、違う中身で既にある",
    ),
    (
        "conflict-usage",
        &[("id", "u-1")],
        "同じ id（u-1）のトークンの使用量が、違う中身で既にある",
    ),
    (
        "ref-not-found",
        &[
            ("field", "by"),
            ("ref", "session:s-9"),
            ("issuer", "roster"),
        ],
        "by のアクター（session:s-9）が、出し手 roster の鍵の付随する id に無い",
    ),
    (
        "link-same-actor",
        &[("actor", "se.1")],
        "from と to が同じアクター（se.1）",
    ),
    (
        "link-already-launched",
        &[("actor", "se.1")],
        "se.1 は、もう別の線で起こされている",
    ),
    (
        "usage",
        &[],
        "使い方: castlist-server（環境変数 CASTLIST_DB・CASTLIST_LISTEN で起きる）",
    ),
];

#[test]
fn 日本語の文は_鍵へ移す前の字と_1_字も違わない() {
    for (key, values, want) in JA {
        let values: Vec<(&'static str, &dyn std::fmt::Display)> = values
            .iter()
            .map(|(name, value)| (*name, value as &dyn std::fmt::Display))
            .collect();
        assert_eq!(text_in(Lang::Ja, key, &values), *want, "{key}");
    }
}

#[test]
fn 写しの表は_訳の_file_の鍵をみな持つ() {
    let table: BTreeSet<String> = JA.iter().map(|(key, _, _)| (*key).to_owned()).collect();
    let known: BTreeSet<String> = messages("ja").into_keys().collect();
    assert_eq!(table, known);
}
