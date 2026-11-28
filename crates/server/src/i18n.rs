//! 外に出す文の言語（AI-48）。
//!
//! - 文は、言語ごとの file（`crates/server/locales/{en,ja}/main.ftl`。Fluent）に置き、コードは鍵で引く（[`text`]・[`text_with`]）。
//!   file は build の時に binary へ埋める（image に入るのは binary だけ）。
//! - 言語は、頼みの `Accept-Language` の並びを前から見て、初めに当たった `ja`・`en`（`ja-JP` は `ja`）。重み（`;q=`）は見ない。
//!   header が無い・`*`・どちらでもない言語は、既定の英語（[`DEFAULT`]）。
//! - いちばん外に近い層（[`scope`]）が言語を決め、その頼みを捌く間だけ覚える（tokio の `task_local`）。守りの層も handler も
//!   DB の仕事も、その頼みの task の内で動くので、どこで [`text`] を呼んでも同じ言語になる。
//!   **答え（Response）を返す前に字にする**（本文が読まれるのは、この scope を抜けた後）。
//! - ここを通らない文: 定義の形の検査（生成した骨）が返す 400 の文（英語のまま）と、起こす時の誤り（DB を開けない・移行に落ちた。
//!   運ぶ人が読む物で、日本語のまま）。
//! - 記録（log）は、いつも英語で書く。[`text`] で作った字（頼みの言語）を、記録に書かない（要る時は [`text_in`] で英語を名指す）。
//! - 頼みの無い所（起こす binary の使い方の行）は、環境の言語で出す（[`from_env`]）。
use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::Display;

use axum::extract::Request;
use axum::http::header;
use axum::middleware::Next;
use axum::response::Response;
use fluent_langneg::{NegotiationStrategy, accepted_languages, negotiate_languages};
use fluent_templates::fluent_bundle::FluentValue;
use fluent_templates::{LanguageIdentifier, Loader, langid};

fluent_templates::static_loader! {
    static LOCALES = {
        locales: "./locales",
        fallback_language: "en",
        // 差し込む値の前後に、向きの見えない印（U+2068・U+2069）を足さない（日本語と英語には要らず、字を比べる側を壊す）。
        customise: |bundle| bundle.set_use_isolating(false),
    };
}

static EN: LanguageIdentifier = langid!("en");
static JA: LanguageIdentifier = langid!("ja");

/// 文の言語。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    En,
    Ja,
}

/// 言語を付けない頼みに返す言語（Product Manager の決め。AI-48）。
pub const DEFAULT: Lang = Lang::En;

impl Lang {
    fn id(self) -> &'static LanguageIdentifier {
        match self {
            Lang::En => &EN,
            Lang::Ja => &JA,
        }
    }
}

tokio::task_local! {
    /// 今の頼みの言語。[`scope`] の内でだけ在る。
    static ASKED: Lang;
}

/// `Accept-Language` の値から言語を選ぶ。並びを前から見て、初めに当たった `ja`・`en`。無い・当たらなければ [`DEFAULT`]。
pub fn negotiate(accept_language: Option<&str>) -> Lang {
    let Some(value) = accept_language else {
        return DEFAULT;
    };
    let requested = accepted_languages::parse(value);
    let available = [EN.clone(), JA.clone()];
    let chosen = negotiate_languages(
        &requested,
        &available,
        Some(DEFAULT.id()),
        NegotiationStrategy::Lookup,
    );
    match chosen.first() {
        Some(lang) if **lang == JA => Lang::Ja,
        Some(lang) if **lang == EN => Lang::En,
        _ => DEFAULT,
    }
}

/// 頼みの言語を決め、その頼みを捌く間、[`text`] がその言語で引くようにする（守りの層より外に掛ける）。
pub async fn scope(request: Request, next: Next) -> Response {
    let lang = negotiate(
        request
            .headers()
            .get(header::ACCEPT_LANGUAGE)
            .and_then(|value| value.to_str().ok()),
    );
    ASKED.scope(lang, next.run(request)).await
}

/// 今の頼みの言語。[`scope`] の外では [`DEFAULT`]。
pub fn asked() -> Lang {
    ASKED.try_with(|lang| *lang).unwrap_or(DEFAULT)
}

/// 鍵の文を、今の頼みの言語で返す。
pub fn text(key: &str) -> String {
    text_with(key, &[])
}

/// 鍵の文に値を入れて、今の頼みの言語で返す。値は字にして入れる（数も字のまま。言語ごとの桁の区切りを付けない）。
pub fn text_with(key: &str, values: &[(&'static str, &dyn Display)]) -> String {
    text_in(asked(), key, values)
}

/// 鍵の文に値を入れて、名指した言語で返す（記録に書く字は、いつも英語にする）。鍵が無ければ、鍵の字（試験が、無い鍵を止める）。
pub fn text_in(lang: Lang, key: &str, values: &[(&'static str, &dyn Display)]) -> String {
    let args: HashMap<Cow<'static, str>, FluentValue<'static>> = values
        .iter()
        .map(|(name, value)| (Cow::Borrowed(*name), FluentValue::from(value.to_string())))
        .collect();
    LOCALES
        .try_lookup_with_args(lang.id(), key, &args)
        .unwrap_or_else(|| key.to_owned())
}

/// 環境の言語（頼みの無い所: 起こす binary の使い方の行）。`LC_ALL` → `LC_MESSAGES` → `LANG` の順に見て、初めに決まっている物
/// （`ja_JP.UTF-8` は `ja`）。空・`C`・`POSIX`（`C.UTF-8` も）は、決めていない物として読み飛ばす。どれも無い・`ja`・`en` の
/// どちらでもなければ [`DEFAULT`]。`var` は環境変数を読む関数。
pub fn from_env(var: impl Fn(&str) -> Option<String>) -> Lang {
    let set = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|name| var(name))
        .map(|value| {
            // `ja_JP.UTF-8@modifier` の、言語と地域だけを取る。
            let locale = value.split(['.', '@']).next().unwrap_or_default();
            locale.replace('_', "-")
        })
        .find(|locale| !matches!(locale.as_str(), "" | "C" | "POSIX"));
    set.map_or(DEFAULT, |locale| negotiate(Some(&locale)))
}

/// 試験が、言語を決めて関数を呼ぶ（頼みを通さない単体の試験）。
#[cfg(test)]
pub fn in_lang<R>(lang: Lang, run: impl FnOnce() -> R) -> R {
    ASKED.sync_scope(lang, run)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 言語を付けない頼みは英語() {
        assert_eq!(negotiate(None), Lang::En);
        assert_eq!(negotiate(Some("")), Lang::En);
        assert_eq!(negotiate(Some("*")), Lang::En);
        assert_eq!(asked(), Lang::En, "scope の外");
    }

    #[test]
    fn 並びを前から見て_初めに当たった言語を選ぶ() {
        for (value, want) in [
            ("ja", Lang::Ja),
            ("ja-JP", Lang::Ja),
            ("JA", Lang::Ja),
            ("ja,en;q=0.9", Lang::Ja),
            ("en", Lang::En),
            ("en-US,ja;q=0.8", Lang::En),
            ("en-GB", Lang::En),
            // どちらでもない言語は飛ばし、後ろの当たる言語を選ぶ。
            ("fr-CH, fr;q=0.9, ja;q=0.8, en;q=0.7", Lang::Ja),
            // 当たる言語が無ければ既定。
            ("fr", Lang::En),
            ("zh-TW, ko", Lang::En),
            // 重みは見ない（並びの順）。
            ("en;q=0.1, ja", Lang::En),
            // 読めない字。
            ("???", Lang::En),
        ] {
            assert_eq!(negotiate(Some(value)), want, "{value}");
        }
    }

    #[test]
    fn 値を入れた文に_見えない印が入らず_数は字のまま() {
        let ja = text_in(Lang::Ja, "guard-timeout-seconds", &[("n", &12345)]);
        assert_eq!(ja, "時間がかかりすぎた（12345 秒）。少し待ってから送り直す");
        let en = text_in(Lang::En, "guard-timeout-seconds", &[("n", &12345)]);
        assert_eq!(en, "Took too long (12345 s). Wait a moment and send again");
    }

    #[test]
    fn 言語を決めた内では_その言語で引く() {
        assert_eq!(
            in_lang(Lang::Ja, || text("busy")),
            "混んでいる。少し待ってから送り直す"
        );
        assert_eq!(text("busy"), "Busy. Wait a moment and send again");
    }
}
