//! 要求の本文の JSON（出来事の detail）と、同じ id で同じ本文かを見る写し（body_hash）。
//!
//! 写しの形（docs/src/api.md の「データ」にも書く。替えれば、前に受けた物の送り直しが 409 になる）:
//! 1. 本文の欄を、定義の名（camelCase）の JSON の物にする。道の id は入れない（同じ id どうしでしか比べない）。
//!    - 出来事: `issuer`・`by`・`type`・`detail`・`sentAt`
//!    - 線: `issuer`・`type`・`from`・`to`・`sentAt`
//!    - model と effort: `issuer`・`by`・`model`・`effort`・`source`・`sentAt`
//!    - トークンの使用量: `issuer`・`by`・`model`・`agent`（本体の回なら欄ごと無い）・`speed`・`serviceTier`・`fromAt`・`toAt`・
//!      `turns`・`inputTokens`・`outputTokens`・`cacheReadTokens`・`cacheWrite5mTokens`・`cacheWrite1hTokens`・`webSearchRequests`・
//!      `webFetchRequests`・`costMicroUsd`・`maxContext`・`buckets`・`sentAt`。`fromAt`・`toAt` は DB に残す字（UTC・ミリ秒まで・24 字）、`buckets` は `bucket` の順に
//!      並べた `{bucket, turns, costMicroUsd}` の配列（送られた並びは問わない）
//! 2. `detail` は、無ければ（JSON の null も）空の物 `{}`。`by`・`from`・`to` は送られた参照の字（`種類:値`）のまま。
//! 3. `sentAt` は、DB に残す送り手の時刻と同じ字（UTC の RFC 3339。秒の下は要る桁だけ。例 `2026-11-24T00:00:00Z`・
//!    `2026-11-24T00:00:00.5Z`）。同じ時刻の違う書き方は、同じ字になる。
//! 4. どの深さの物も、鍵を UTF-8 の byte の順に並べ、空白を入れずに書く（[`canonical`]）。字は serde_json の書き方
//!    （`"` と `\` と制御文字だけを escape し、ほかの字はそのまま）、数も serde_json の書き方（整数はそのまま、小数は短い字）。
//! 5. その字の UTF-8 の byte の SHA-256 を、小文字の 16 進 64 字にする（[`hash`]）。
use aws_smithy_types::{Document, Number};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// 要求の Document を、JSON の値にする（JSON に無い小数（NaN など）は null。JSON から読んだ物には無い）。
pub fn json(document: &Document) -> Value {
    match document {
        Document::Null => Value::Null,
        Document::Bool(b) => Value::Bool(*b),
        Document::Number(Number::PosInt(u)) => Value::from(*u),
        Document::Number(Number::NegInt(i)) => Value::from(*i),
        Document::Number(Number::Float(f)) => {
            serde_json::Number::from_f64(*f).map_or(Value::Null, Value::Number)
        }
        Document::String(s) => Value::String(s.clone()),
        Document::Array(items) => Value::Array(items.iter().map(json).collect()),
        Document::Object(map) => {
            Value::Object(map.iter().map(|(k, v)| (k.clone(), json(v))).collect())
        }
    }
}

/// JSON の値の決まった字: どの深さの物も鍵を UTF-8 の byte の順に並べ、空白を入れない（serde_json の物の鍵の並びには頼らない）。
pub fn canonical(value: &Value) -> String {
    let mut out = String::new();
    write(value, &mut out);
    out
}

fn write(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(&String, &Value)> = map.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            out.push('{');
            for (i, (key, value)) in entries.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String(key.clone()).to_string());
                out.push(':');
                write(value, out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write(item, out);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

/// 本文の写し: 決まった字の SHA-256 を、小文字の 16 進 64 字にする。
pub fn hash(body: &Value) -> String {
    Sha256::digest(canonical(body).as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn canonical_sorts_keys_at_every_depth_without_spaces() {
        let value: Value = serde_json::from_str(
            r#"{ "z": [ { "b": 1, "a": "x\"y" } ], "a": { "d": null, "c": -1.5 }, "あ": true }"#,
        )
        .unwrap();
        assert_eq!(
            canonical(&value),
            r#"{"a":{"c":-1.5,"d":null},"z":[{"a":"x\"y","b":1}],"あ":true}"#
        );
    }

    #[test]
    fn hash_is_lowercase_hex_sha256_of_the_canonical_form() {
        // 空の物 `{}` の SHA-256。
        assert_eq!(
            hash(&json!({})),
            "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a"
        );
        assert_eq!(
            hash(&json!({ "b": 1, "a": 2 })),
            hash(&serde_json::from_str(r#"{"a":2,"b":1}"#).unwrap())
        );
    }

    #[test]
    fn documents_become_the_same_json() {
        let document = Document::Object(
            [
                ("n".to_owned(), Document::Number(Number::NegInt(-2))),
                ("f".to_owned(), Document::Number(Number::Float(0.5))),
                (
                    "l".to_owned(),
                    Document::Array(vec![Document::Null, Document::Bool(true)]),
                ),
            ]
            .into_iter()
            .collect(),
        );
        assert_eq!(
            json(&document),
            json!({ "n": -2, "f": 0.5, "l": [null, true] })
        );
    }
}
