//! Typed field fragments for a single oversized metadata record.
//! The host reconstructs and verifies the record; the model never splices JSON bytes.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Field {
    pub ordinal: usize,
    pub path: Vec<String>,
    pub value: Value,
    pub start_byte: Option<usize>,
    pub end_byte: Option<usize>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub original: Value,
    pub fields: Vec<Field>,
    pub received: BTreeMap<usize, Vec<(usize, usize)>>,
    pub atoms: BTreeSet<usize>,
}
fn flatten(value: &Value, path: &mut Vec<String>, out: &mut Vec<Field>) {
    let atomic = serde_json::from_value::<super::evidence::EvidenceRef>(value.clone()).is_ok();
    match value {
        Value::Object(map) if !atomic && !map.is_empty() => {
            for (key, v) in map {
                path.push(key.clone());
                flatten(v, path, out);
                path.pop();
            }
        }
        Value::Array(items) if !items.is_empty() => {
            for (index, v) in items.iter().enumerate() {
                path.push(index.to_string());
                flatten(v, path, out);
                path.pop();
            }
        }
        _ => {
            let end = value.as_str().map(str::len);
            out.push(Field {
                ordinal: out.len(),
                path: path.clone(),
                value: value.clone(),
                start_byte: end.map(|_| 0),
                end_byte: end,
            });
        }
    }
}
impl Record {
    pub fn new(original: Value) -> Self {
        let mut fields = Vec::new();
        flatten(&original, &mut Vec::new(), &mut fields);
        Self {
            original,
            fields,
            received: BTreeMap::new(),
            atoms: BTreeSet::new(),
        }
    }
    pub fn receive(&mut self, parts: &[Field]) -> Result<bool, String> {
        for part in parts {
            let expected = self
                .fields
                .get(part.ordinal)
                .ok_or("unknown metadata field")?;
            if expected.path != part.path {
                return Err("metadata field path changed".into());
            }
            match expected.value.as_str() {
                Some(text) => {
                    let (a, b) = part
                        .start_byte
                        .zip(part.end_byte)
                        .ok_or("metadata range missing")?;
                    let fragment = part
                        .value
                        .as_str()
                        .ok_or("metadata text fragment must be a string")?;
                    if text.get(a..b) != Some(fragment) || a > b {
                        return Err("metadata fragment differs from immutable record".into());
                    }
                }
                None => {
                    if part != expected {
                        return Err("metadata value changed".into());
                    }
                }
            }
        }
        for part in parts {
            if let Some((a, b)) = part.start_byte.zip(part.end_byte) {
                self.received.entry(part.ordinal).or_default().push((a, b));
            } else {
                self.atoms.insert(part.ordinal);
            }
        }
        Ok(self.complete())
    }
    pub fn complete(&self) -> bool {
        self.fields.iter().all(|field| {
            if let Some(text) = field.value.as_str() {
                let mut ranges = self
                    .received
                    .get(&field.ordinal)
                    .cloned()
                    .unwrap_or_default();
                if ranges.is_empty() {
                    return false;
                }
                ranges.sort_unstable();
                let mut end = 0;
                for (a, b) in ranges {
                    if a > end {
                        return false;
                    }
                    end = end.max(b);
                }
                end == text.len()
            } else {
                self.atoms.contains(&field.ordinal)
            }
        })
    }
    pub fn restored(&self) -> Result<Value, String> {
        if !self.complete() {
            return Err("metadata record is not completely delivered".into());
        }
        Ok(self.original.clone())
    }
    pub fn page(&self, key: &str, fields: &[Field]) -> Value {
        let mut identity = serde_json::Map::new();
        for field in [
            "requirement_id",
            "pack_id",
            "slot_id",
            "chapter_id",
            "id",
            "structural_receipt_id",
        ] {
            if let Some(value) = self.original.get(field) {
                identity.insert(field.into(), value.clone());
            }
        }
        json!({"record_key":key,"record_identity":identity,"field_fragments":fields,"total_fields":self.fields.len(),"instruction":"These are named fields of one immutable record. Follow the host continuation; record identity and field paths are supplied by the host. Do not concatenate JSON or invent byte ranges."})
    }
}
pub fn split(field: &Field) -> Result<(Field, Field), String> {
    let text = field
        .value
        .as_str()
        .ok_or("non-text metadata field is indivisible")?;
    let cuts = text
        .char_indices()
        .map(|(i, _)| i)
        .filter(|i| *i > 0)
        .collect::<Vec<_>>();
    if cuts.is_empty() {
        return Err("metadata field cannot be split further".into());
    }
    let cut = cuts[cuts.len() / 2];
    let start = field.start_byte.ok_or("range missing")?;
    let mut left = field.clone();
    let mut right = field.clone();
    left.value = json!(&text[..cut]);
    left.end_byte = Some(start + cut);
    right.value = json!(&text[cut..]);
    right.start_byte = Some(start + cut);
    Ok((left, right))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn named_fields_reconstruct_only_after_complete_utf8_delivery() {
        let original = json!({"requirement_id":"r","description":"条款😀".repeat(10000),"condition":{"applies":true,"sources":["source-a","source-b"]},"empty":[]});
        let mut record = Record::new(original.clone());
        let long = record
            .fields
            .iter()
            .find(|f| f.path == ["description"])
            .unwrap()
            .clone();
        let (left, right) = split(&long).unwrap();
        let mut pieces = record
            .fields
            .iter()
            .filter(|f| f.ordinal != long.ordinal)
            .cloned()
            .collect::<Vec<_>>();
        pieces.push(right.clone());
        assert!(!record.receive(&pieces).unwrap());
        assert!(record.restored().is_err());
        assert!(record.receive(std::slice::from_ref(&left)).unwrap());
        assert_eq!(record.restored().unwrap(), original);
        assert!(record.receive(&[left]).unwrap());
        let mut forged = right;
        forged.path = vec!["condition".into()];
        let before = record.clone();
        assert!(record.receive(&[forged]).is_err());
        assert_eq!(record, before);
    }
}
