//! 頁の印（nextToken）: 中身の字を改行で繋ぎ、16 進にした物。中は読まずに、前の答えの物をそのまま渡してもらう。
//! 頭の字は検索の種類で、ほかの検索の印と分ける。

/// 崩れた印の訳。
pub const BROKEN: &str = "nextToken が読めない（前の答えの nextToken を、そのまま渡す）";

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

/// ほかの検索の印の訳（`what` は検索の名。例「アクターの検索」）。
fn other(what: &str) -> String {
    format!("nextToken が読めない（{what}の nextToken ではない）")
}

/// 印を戻し、`kind` の検索の `n` 個の中身（頭の種類を含む）を返す。崩れた物と、種類が同じで数の違う物は崩れた印、
/// 種類の違う物はほかの検索の印（`what` は今の検索の名）の訳。
pub fn read(token: &str, kind: &str, n: usize, what: &str) -> Result<Vec<String>, String> {
    let parts = decode(token).ok_or(BROKEN)?;
    if parts[0] != kind {
        return Err(other(what));
    }
    if parts.len() != n {
        return Err(BROKEN.to_owned());
    }
    Ok(parts)
}

/// 受けた順（seq）だけで続ける検索（ListSettings・ListEvents）の印。
pub fn seq_token(kind: &str, seq: i64) -> String {
    encode(&[kind, &seq.to_string()])
}

/// 受けた順だけで続ける検索の印を読み、前の頁の最後の seq を返す。崩れた物・ほかの検索の物は、その訳。
pub fn seq_after(token: &str, kind: &str, what: &str) -> Result<i64, String> {
    read(token, kind, 2, what)?[1]
        .parse()
        .map_err(|_| BROKEN.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_seq_token_reads_back_and_a_broken_or_other_one_is_refused() {
        let token = seq_token("events", 7);
        assert_eq!(seq_after(&token, "events", "出来事の検索"), Ok(7));
        assert_eq!(
            seq_after(&token, "settings", "model と effort の検索"),
            Err(other("model と effort の検索"))
        );
        assert_eq!(
            seq_after(
                &encode(&["actors", "entered", "asc", "x", "7"]),
                "events",
                "出来事の検索"
            ),
            Err(other("出来事の検索"))
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
                seq_after(broken, "events", "出来事の検索"),
                Err(BROKEN.to_owned()),
                "{broken}"
            );
        }
    }
}
