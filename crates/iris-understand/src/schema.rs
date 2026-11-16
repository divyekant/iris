//! Step 8: validate each item against the open model schemas in
//! `docs/spec/schema/`. The schemas are in the program, and the validator is
//! offline, so a `$ref` never goes to the network.

use std::sync::OnceLock;

use jsonschema::{Draft, Registry, Validator};
use serde_json::Value;

const COMMON: &str = include_str!("../../../docs/spec/schema/common.schema.json");
const FORMS: [(&str, &str); 5] = [
    (
        "commitment",
        include_str!("../../../docs/spec/schema/commitment.schema.json"),
    ),
    (
        "conversation",
        include_str!("../../../docs/spec/schema/conversation.schema.json"),
    ),
    (
        "record",
        include_str!("../../../docs/spec/schema/record.schema.json"),
    ),
    (
        "feed",
        include_str!("../../../docs/spec/schema/feed.schema.json"),
    ),
    (
        "signal",
        include_str!("../../../docs/spec/schema/signal.schema.json"),
    ),
];

fn validators() -> &'static [(&'static str, Validator)] {
    static V: OnceLock<Vec<(&str, Validator)>> = OnceLock::new();
    V.get_or_init(|| {
        let common: Value = serde_json::from_str(COMMON).expect("common schema is JSON");
        let registry = Registry::new()
            .add("urn:iris:model:0:common", common)
            .expect("a valid $id")
            .prepare()
            .expect("the schemas resolve");
        FORMS
            .iter()
            .map(|(name, text)| {
                let schema: Value = serde_json::from_str(text).expect("form schema is JSON");
                let v = jsonschema::options()
                    .with_draft(Draft::Draft202012)
                    .should_validate_formats(true)
                    .offline()
                    .with_registry(&registry)
                    .build(&schema)
                    .expect("a valid form schema");
                (*name, v)
            })
            .collect()
    })
}

/// `Ok` if `item` is valid for the schema of its `form`. The error names the
/// first problem. It can hold values of the item, so it never goes to a model.
pub fn validate(item: &Value) -> Result<(), String> {
    let form = item["form"].as_str().unwrap_or("");
    let (_, v) = validators()
        .iter()
        .find(|(n, _)| *n == form)
        .ok_or_else(|| format!("unknown form {form:?}"))?;
    v.validate(item).map_err(|e| e.to_string())
}

/// True if a draft is valid when the store adds the fields that it owns
/// (ID, sources, times, `derived` and the `p-` ID of a sender party).
pub(crate) fn valid_draft(d: &crate::Draft) -> bool {
    let mut v = d.item.clone();
    for key in ["owner", "counterparty", "issuer", "publisher"] {
        if v[key]["kind"] == "sender" {
            v[key]["sender"] = Value::from("p-1");
        }
    }
    if let Some(Value::Array(list)) = v.pointer_mut("/fields/parties") {
        for p in list.iter_mut().filter(|p| p["kind"] == "sender") {
            p["sender"] = Value::from("p-1");
        }
    }
    let prefix = d.form().chars().next().unwrap_or('x');
    let derived = match d.by {
        crate::By::Rule(r) => serde_json::json!({"by": "rule", "rule": r}),
        crate::By::Model => serde_json::json!({"by": "model", "model": "m", "prompt": "p"}),
    };
    v["model"] = Value::from("iris/0");
    v["id"] = Value::from(format!("{prefix}-1"));
    v["sources"] = serde_json::json!([{"message": "m-1", "thread": "t-1", "role": "origin", "quote": d.quote}]);
    v["created"] = Value::from("2026-01-01T00:00:00Z");
    v["updated"] = Value::from("2026-01-01T00:00:00Z");
    v["derived"] = derived;
    validate(&v).is_ok()
}
