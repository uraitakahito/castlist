//! 要求の本文の JSON（出来事の detail）。
use aws_smithy_types::{Document, Number};
use serde_json::Value;

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
