//! The Iris open model schemas, for tests.
//!
//! The schemas and the sample documents are in `docs/spec/schema/`. The test of
//! the samples is in `tests/schemas.rs`. A crate that writes a document of the
//! open model can check its output with [`validator`] in its own tests. The
//! `iris` program does not use this crate.

use std::fs;
use std::path::{Path, PathBuf};

use jsonschema::{Draft, Registry, Validator};
use serde_json::Value;

pub fn schema_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/spec/schema")
}

pub fn read_json(path: &Path) -> Value {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

pub fn json_files(dir: &Path) -> Vec<PathBuf> {
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
pub fn validator(name: &str) -> Validator {
    build(name, true)
}

/// The validator of the conformance kit (spec 4, section 5.4): the same as
/// [`validator`], but the `email` format accepts each text, because
/// validators do not agree on `email`. `date` and `date-time` stay checked.
pub fn kit_validator(name: &str) -> Validator {
    build(name, false)
}

fn build(name: &str, email: bool) -> Validator {
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
    let mut options = jsonschema::options()
        .with_draft(Draft::Draft202012)
        .should_validate_formats(true)
        .offline()
        .with_registry(&registry);
    if !email {
        options = options.with_format("email", |_| true);
    }
    options
        .build(&schema)
        .unwrap_or_else(|e| panic!("{name}.schema.json is not a valid schema: {e}"))
}
