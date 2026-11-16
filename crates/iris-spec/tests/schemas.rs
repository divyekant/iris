//! Validates the sample documents in `docs/spec/schema/examples/` against the
//! open model schemas. Each file in `examples/` must pass. Each file in
//! `examples/invalid/` must fail. The name of a sample starts with the name of
//! its schema, for example `commitment-payment.json` uses
//! `commitment.schema.json`.

use std::path::Path;

use iris_spec::{json_files, read_json, schema_dir, validator};

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
        "changeset",
        "job",
        "matter",
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
