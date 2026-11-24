//! `iris zone`: the time zone of defer dates and due dates (decision D1).

use std::process::Command;

use iris_store::Store;

mod common;

fn iris(store: &Store, args: &[&str]) -> (bool, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_iris"))
        .args(args)
        .arg("--data")
        .arg(store.dir())
        .env("TZ", "UTC")
        .output()
        .unwrap();
    (
        out.status.success(),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

#[test]
fn the_zone_command_sets_shows_and_clears_the_time_zone() {
    let store = Store::open(&common::data_dir("zone")).unwrap();
    let (ok, out, _) = iris(&store, &["zone"]);
    assert!(ok);
    assert!(out.contains("from the computer"), "{out}");

    let (ok, out, err) = iris(&store, &["zone", "Asia/Kolkata"]);
    assert!(ok, "{err}");
    assert!(out.starts_with("Time zone: Asia/Kolkata."), "{out}");
    let (_, out, _) = iris(&store, &["zone"]);
    assert!(out.starts_with("Time zone: Asia/Kolkata."), "{out}");

    // An unknown name changes nothing.
    let (ok, _, err) = iris(&store, &["zone", "Mars/Olympus"]);
    assert!(!ok);
    assert!(err.contains("Mars/Olympus"), "{err}");
    let (_, out, _) = iris(&store, &["zone"]);
    assert!(out.starts_with("Time zone: Asia/Kolkata."), "{out}");

    let (ok, out, _) = iris(&store, &["zone", "--computer"]);
    assert!(ok);
    assert!(out.contains("from the computer"), "{out}");
}
