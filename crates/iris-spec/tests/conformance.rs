//! The Rust side of the conformance kit (docs/spec/04-open-model-versions.md,
//! section 5). The Python runner checks the same fixtures with another
//! validator (docs/spec/conformance/test_run.py).

use std::fs;

use iris_spec::{json_files, kit_validator, read_json, schema_dir};
use serde_json::Value;

/// Each invalid fixture fails also when `email` accepts each text, so no
/// fixture depends on a format that validators do not agree on.
#[test]
fn each_invalid_fixture_fails_without_the_email_check() {
    for sample in json_files(&schema_dir().join("examples/invalid")) {
        let stem = sample.file_stem().unwrap().to_str().unwrap();
        let name = stem.split('-').next().unwrap();
        assert!(
            !kit_validator(name).is_valid(&read_json(&sample)),
            "{} fails only by the email format",
            sample.display()
        );
    }
}

/// `items.jsonl` holds one item of each form, as `iris export` writes it. The
/// schema of each line comes from its `form`.
#[test]
fn items_jsonl_passes_by_form() {
    let text = fs::read_to_string(schema_dir().join("examples/items.jsonl")).unwrap();
    let mut forms = Vec::new();
    for line in text.lines() {
        let doc: Value = serde_json::from_str(line).unwrap();
        let form = doc["form"].as_str().unwrap().to_owned();
        let errors: Vec<String> = kit_validator(&form)
            .iter_errors(&doc)
            .map(|e| e.to_string())
            .collect();
        assert!(errors.is_empty(), "{form}: {errors:?}");
        forms.push(form);
    }
    forms.sort();
    assert_eq!(
        forms,
        ["commitment", "conversation", "feed", "record", "signal"]
    );
}
