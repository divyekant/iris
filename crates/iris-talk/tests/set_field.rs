//! Property test of `set_field`, the one rule that applies a correction
//! path, on random documents and random paths, with a fixed seed so that a
//! failure repeats.

use iris_talk::set_field;
use serde_json::{json, Value};

/// A small xorshift generator.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n.max(1)
    }
}

const KEYS: &[&str] = &["a", "b", "0", "1", "2", "00", "01", "-1", "+1", "", " "];

fn doc(rng: &mut Rng, depth: u32) -> Value {
    match if depth == 0 {
        rng.below(3)
    } else {
        rng.below(5)
    } {
        0 => Value::Null,
        1 => json!(rng.below(9)),
        2 => json!("x"),
        3 => Value::Array((0..rng.below(3)).map(|_| doc(rng, depth - 1)).collect()),
        _ => {
            let mut m = serde_json::Map::new();
            for _ in 0..rng.below(4) {
                let k = KEYS[rng.below(KEYS.len() as u64) as usize];
                m.insert(k.into(), doc(rng, depth - 1));
            }
            Value::Object(m)
        }
    }
}

/// The kind of each value on the path in `doc`, as far as the path exists.
fn kinds(doc: &Value, steps: &[&str]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut at = Some(doc);
    for s in steps {
        let Some(v) = at else { break };
        out.push(match v {
            Value::Array(_) => 1,
            Value::Object(_) => 2,
            _ => 0,
        });
        at = v.pointer(&format!("/{s}"));
    }
    out
}

#[test]
fn set_field_sets_the_path_or_changes_nothing() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut set = 0;
    for _ in 0..20_000 {
        let before = json!({"top": doc(&mut rng, 3)});
        let steps: Vec<&str> = std::iter::once("top")
            .chain((0..rng.below(4)).map(|_| KEYS[rng.below(KEYS.len() as u64) as usize]))
            .collect();
        let path = steps.join(".");
        let mut after = before.clone();
        let value = json!({"new": true});
        if set_field(&mut after, &path, value.clone()) {
            set += 1;
            assert!(steps.iter().all(|s| !s.is_empty()), "{path}");
            let pointer = format!("/{}", steps.join("/"));
            assert_eq!(after.pointer(&pointer), Some(&value), "{path} in {before}");
            // No array or object on the path changes its kind.
            let old = kinds(&before, &steps[..steps.len() - 1]);
            let new = kinds(&after, &steps[..steps.len() - 1]);
            assert_eq!(new[..old.len()], old[..], "{path} in {before}");
        } else {
            assert_eq!(after, before, "a refused path changes nothing: {path}");
        }
    }
    assert!(set > 1_000, "the test sets enough paths: {set}");
}

#[test]
fn set_field_examples() {
    let mut d = json!({"fields": {"items": [{"name": "a"}, {"name": "b"}]}});
    assert!(set_field(&mut d, "fields.items.1.name", json!("c")));
    assert_eq!(d["fields"]["items"][1]["name"], "c");
    assert!(!set_field(&mut d, "fields.items.2.name", json!("c")));
    assert!(!set_field(&mut d, "fields.items.01", json!("c")));
    assert!(!set_field(&mut d, "fields..x", json!("c")));
    assert!(set_field(&mut d, "due.date", json!("2026-01-09")));
    assert_eq!(d["due"], json!({"date": "2026-01-09"}));
    assert!(!set_field(&mut d, "due.date.day", json!(9)));
}
