//! Validates the sample documents in `docs/spec/schema/examples/` against the
//! open model schemas. Each file in `examples/` must pass. Each file in
//! `examples/invalid/` must fail. The name of a sample starts with the name of
//! its schema, for example `commitment-payment.json` uses
//! `commitment.schema.json`.

use std::fs;
use std::path::{Path, PathBuf};

use jsonschema::{Draft, Registry, Validator};
use serde_json::Value;

fn schema_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/spec/schema")
}

fn read_json(path: &Path) -> Value {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn json_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    files.sort();
    files
}

/// Builds one validator for the schema `<name>.schema.json`. Each schema of
/// the folder is in the registry under its `$id`, and the validator is offline,
/// so a `$ref` never goes to the network.
fn validator(name: &str) -> Validator {
    let dir = schema_dir();
    let mut builder = Registry::new();
    for path in json_files(&dir) {
        let schema = read_json(&path);
        let id = schema["$id"]
            .as_str()
            .expect("each schema has an $id")
            .to_owned();
        builder = builder.add(id, schema).expect("a valid $id");
    }
    let registry = builder.prepare().expect("the schemas resolve");
    let schema = read_json(&dir.join(format!("{name}.schema.json")));
    jsonschema::options()
        .with_draft(Draft::Draft202012)
        .should_validate_formats(true)
        .offline()
        .with_registry(&registry)
        .build(&schema)
        .unwrap_or_else(|e| panic!("{name}.schema.json is not a valid schema: {e}"))
}

fn schema_name(sample: &Path) -> String {
    let stem = sample.file_stem().unwrap().to_str().unwrap();
    stem.split('-').next().unwrap().to_owned()
}

#[test]
fn valid_samples_pass() {
    let samples = json_files(&schema_dir().join("examples"));
    assert!(
        samples.len() >= 6,
        "expected a sample for each form and the sender"
    );
    for sample in samples {
        let errors: Vec<String> = validator(&schema_name(&sample))
            .iter_errors(&read_json(&sample))
            .map(|e| format!("{} at {}", e, e.instance_path()))
            .collect();
        assert!(
            errors.is_empty(),
            "{} must pass: {errors:#?}",
            sample.display()
        );
    }
}

#[test]
fn invalid_samples_fail() {
    let samples = json_files(&schema_dir().join("examples/invalid"));
    assert!(!samples.is_empty(), "expected invalid samples");
    for sample in samples {
        let valid = validator(&schema_name(&sample)).is_valid(&read_json(&sample));
        assert!(!valid, "{} must fail", sample.display());
    }
}

#[test]
fn each_form_has_a_schema_and_a_sample() {
    let dir = schema_dir();
    for name in [
        "commitment",
        "conversation",
        "record",
        "feed",
        "signal",
        "sender",
    ] {
        let schema = read_json(&dir.join(format!("{name}.schema.json")));
        assert_eq!(schema["$id"], format!("urn:iris:model:0:{name}"));
        assert!(
            schema["title"].as_str().unwrap().contains("(draft)"),
            "{name}.schema.json must be marked draft"
        );
        let has_sample = json_files(&dir.join("examples"))
            .iter()
            .any(|sample| schema_name(sample) == name);
        assert!(has_sample, "{name} has no sample in examples/");
    }
}
