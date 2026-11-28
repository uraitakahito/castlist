//! 頁の印（nextToken）: 中身の字を改行で繋ぎ、16 進にした物。中は読まずに、前の答えの物をそのまま渡してもらう。
//! 頭の字は検索の種類で、ほかの検索の印と分ける。

use crate::i18n::text;

/// 崩れた印の訳。
pub fn broken() -> String {
    text("token-broken")
}

/// 印を使う検索（ほかの検索の印を渡された時の文を、検索ごとに引く。言語で語順が違うので、文を分ける）。
#[derive(Clone, Copy)]
pub enum Search {
    Actors,
    Events,
    Settings,
    Usage,
}

impl Search {
    /// ほかの検索の印の訳（今の検索が self）。
    fn other(self) -> String {
        match self {
            Search::Actors => text("token-other-actors"),
            Search::Events => text("token-other-events"),
            Search::Settings => text("token-other-settings"),
            Search::Usage => text("token-other-usage"),
        }
    }
}

/// 字を改行で繋ぎ、16 進にする。
pub fn encode(parts: &[&str]) -> String {
    parts
        .join("\n")
        .bytes()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// 16 進を戻し、改行で分ける。崩れていれば（空の物も）None。
pub fn decode(token: &str) -> Option<Vec<String>> {
    if token.is_empty()
        || !token.len().is_multiple_of(2)
        || !token.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return None;
    }
    let bytes = (0..token.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&token[i..i + 2], 16))
        .collect::<Result<Vec<u8>, _>>()
        .ok()?;
    let text = String::from_utf8(bytes).ok()?;
    Some(text.split('\n').map(str::to_owned).collect())
}

/// 印を戻し、`kind` の検索の `n` 個の中身（頭の種類を含む）を返す。崩れた物と、種類が同じで数の違う物は崩れた印、
/// 種類の違う物はほかの検索の印（`search` は今の検索）の訳。
pub fn read(token: &str, kind: &str, n: usize, search: Search) -> Result<Vec<String>, String> {
    let parts = decode(token).ok_or_else(broken)?;
    if parts[0] != kind {
        return Err(search.other());
    }
    if parts.len() != n {
        return Err(broken());
    }
    Ok(parts)
}

/// 受けた順（seq）だけで続ける検索（ListSettings・ListEvents）の印。
pub fn seq_token(kind: &str, seq: i64) -> String {
    encode(&[kind, &seq.to_string()])
}

/// 受けた順だけで続ける検索の印を読み、前の頁の最後の seq を返す。崩れた物・ほかの検索の物は、その訳。
pub fn seq_after(token: &str, kind: &str, search: Search) -> Result<i64, String> {
    read(token, kind, 2, search)?[1]
        .parse()
        .map_err(|_| broken())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{Lang, in_lang};

    const BROKEN: &str = "nextToken が読めない（前の答えの nextToken を、そのまま渡す）";

    #[test]
    fn a_seq_token_reads_back_and_a_broken_or_other_one_is_refused() {
        in_lang(Lang::Ja, || {
            let token = seq_token("events", 7);
            assert_eq!(seq_after(&token, "events", Search::Events), Ok(7));
            assert_eq!(
                seq_after(&token, "settings", Search::Settings),
                Err(
                    "nextToken が読めない（model と effort の検索の nextToken ではない）"
                        .to_owned()
                )
            );
            assert_eq!(
                seq_after(
                    &encode(&["actors", "entered", "asc", "x", "7"]),
                    "events",
                    Search::Events
                ),
                Err("nextToken が読めない（出来事の検索の nextToken ではない）".to_owned())
            );
            // 種類が同じで、中身の数が違う物は崩れた印。
            for broken in [
                "zz",
                "abc",
                "ff",
                "",
                &encode(&["events", "x"]),
                &encode(&["events", "1", "2"]),
                &encode(&["events"]),
            ] {
                assert_eq!(
                    seq_after(broken, "events", Search::Events),
                    Err(BROKEN.to_owned()),
                    "{broken}"
                );
            }
        });
    }

    #[test]
    fn 言語を決めない所では_英語の文() {
        assert_eq!(
            seq_after("ff", "events", Search::Events),
            Err(
                "Cannot read nextToken (pass the nextToken from the previous answer unchanged)"
                    .to_owned()
            )
        );
    }
}
