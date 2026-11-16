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
/// (ID, sources, times, `derived` and the `p-` ID of a sender party). The
/// check uses [`crate::Draft::finish`], the same code as the store.
pub(crate) fn valid_draft(d: &crate::Draft) -> bool {
    let ids = crate::Ids {
        message: 1,
        thread: 1,
        now_ms: 0,
    };
    validate(&d.finish(&ids, 1, "m", &mut |_, _| 1)).is_ok()
}
