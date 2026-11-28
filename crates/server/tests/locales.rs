//! 訳の file（`crates/server/locales/{en,ja}/main.ftl`）と、コードが引く鍵の突き合わせ（AI-48）。
//! - 日本語と英語は、同じ鍵と、同じ値の名を持つ。
//! - 英語の文に、かなと漢字が無い。
//! - コードが引く鍵は、どちらの言語にも在り、使わない鍵は無い。
#![allow(clippy::disallowed_methods)]

use std::collections::{BTreeMap, BTreeSet};

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
