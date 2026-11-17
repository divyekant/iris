//! The memory of a job between sessions (docs/specs/2026-11-17-job-memory.md).

use std::fs;
use std::path::{Path, PathBuf};

use iris_job::{create, finish, notes, By, End, NOTES_MAX};
use iris_store::Store;
use iris_workspace::{build, read_notes, save_notes, Notes};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("iris-notes-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const NOW: i64 = 1_763_283_422_000;

fn setup(tmp: &TempDir) -> (Store, i64, PathBuf) {
    let store = Store::open(&tmp.0.join("data")).unwrap();
    let job = create(&store, "Get the signed contract from Dana.", None, NOW).unwrap();
    let ws = tmp.0.join("ws");
    build(&store, job, &ws).unwrap();
    (store, job, ws)
}

/// A small deterministic generator (xorshift), so a failure repeats.
fn bytes(seed: &mut u64, len: usize) -> Vec<u8> {
    (0..len)
        .map(|_| {
            *seed ^= *seed << 13;
            *seed ^= *seed >> 7;
            *seed ^= *seed << 17;
            *seed as u8
        })
        .collect()
}

#[test]
fn notes_come_back_byte_for_byte_after_the_workspace_is_lost() {
    let tmp = TempDir::new("lost");
    let (store, job, ws) = setup(&tmp);
    let mut seed = 0x9e37_79b9_7f4a_7c15;
    let mut lens = vec![0, 1, 2, NOTES_MAX - 1, NOTES_MAX];
    lens.extend((0..40).map(|i| (i * 1637) % NOTES_MAX));
    for len in lens {
        // Any bytes: not UTF-8, NUL, CR, no line end at the end.
        let text = bytes(&mut seed, len);
        fs::write(ws.join("notes.md"), &text).unwrap();
        assert_eq!(save_notes(&store, job, &ws, NOW).unwrap(), Notes::Saved);
        assert_eq!(notes(&store, job).unwrap().as_deref(), Some(&text[..]));
        fs::remove_dir_all(&ws).unwrap();
        build(&store, job, &ws).unwrap();
        assert_eq!(fs::read(ws.join("notes.md")).unwrap(), text, "len {len}");
    }
}

#[test]
fn a_build_saves_the_notes_and_keeps_the_file() {
    let tmp = TempDir::new("build");
    let (store, job, ws) = setup(&tmp);
    // The first build saves before it makes the file: no copy yet.
    assert_eq!(notes(&store, job).unwrap(), None);
    fs::write(ws.join("notes.md"), b"Dana has the contract.").unwrap();
    build(&store, job, &ws).unwrap();
    assert_eq!(
        notes(&store, job).unwrap().as_deref(),
        Some(&b"Dana has the contract."[..])
    );
    assert_eq!(fs::read(ws.join("notes.md")).unwrap(), b"Dana has the contract.");
}

#[test]
fn a_file_over_the_limit_keeps_the_last_copy() {
    let tmp = TempDir::new("large");
    let (store, job, ws) = setup(&tmp);
    fs::write(ws.join("notes.md"), b"good").unwrap();
    save_notes(&store, job, &ws, NOW).unwrap();
    let large = vec![b'a'; NOTES_MAX + 1];
    fs::write(ws.join("notes.md"), &large).unwrap();
    assert_eq!(save_notes(&store, job, &ws, NOW).unwrap(), Notes::TooLarge);
    assert_eq!(notes(&store, job).unwrap().as_deref(), Some(&b"good"[..]));
    // The build does not stop, and the file stays.
    build(&store, job, &ws).unwrap();
    assert_eq!(fs::read(ws.join("notes.md")).unwrap(), large);
    assert!(iris_job::save_notes(&store, job, &large, NOW).is_err());
    // A path that cannot open for another reason.
    assert!(matches!(
        read_notes(&ws.join("notes.md")),
        Err(Notes::Unreadable(_))
    ));
    assert_eq!(notes(&store, job).unwrap().as_deref(), Some(&b"good"[..]));
}

#[test]
fn no_copy_and_no_file_gives_the_heading() {
    let tmp = TempDir::new("none");
    let store = Store::open(&tmp.0.join("data")).unwrap();
    let job = create(&store, "A goal.", None, NOW).unwrap();
    assert_eq!(notes(&store, job).unwrap(), None);
    let ws = tmp.0.join("ws");
    assert_eq!(save_notes(&store, job, &ws, NOW).unwrap(), Notes::Absent);
    build(&store, job, &ws).unwrap();
    assert_eq!(fs::read(ws.join("notes.md")).unwrap(), b"# Notes\n");
}

#[cfg(unix)]
fn assert_not_a_file(store: &Store, job: i64, ws: &Path) {
    assert_eq!(save_notes(store, job, ws, NOW).unwrap(), Notes::NotAFile);
    assert_eq!(notes(store, job).unwrap().as_deref(), Some(&b"good"[..]));
    // The build keeps the path as the agent left it.
    build(store, job, ws).unwrap();
    assert_eq!(notes(store, job).unwrap().as_deref(), Some(&b"good"[..]));
}

#[cfg(unix)]
#[test]
fn a_link_a_folder_or_a_fifo_keeps_the_last_copy() {
    let tmp = TempDir::new("odd");
    let (store, job, ws) = setup(&tmp);
    fs::write(ws.join("notes.md"), b"good").unwrap();
    save_notes(&store, job, &ws, NOW).unwrap();

    let secret = tmp.0.join("secret");
    fs::write(&secret, b"SECRET-OUTSIDE").unwrap();
    fs::remove_file(ws.join("notes.md")).unwrap();
    std::os::unix::fs::symlink(&secret, ws.join("notes.md")).unwrap();
    assert_not_a_file(&store, job, &ws);
    assert!(fs::symlink_metadata(ws.join("notes.md")).unwrap().file_type().is_symlink());

    fs::remove_file(ws.join("notes.md")).unwrap();
    fs::create_dir(ws.join("notes.md")).unwrap();
    assert_not_a_file(&store, job, &ws);

    fs::remove_dir(ws.join("notes.md")).unwrap();
    let path = std::ffi::CString::new(ws.join("notes.md").as_os_str().as_encoded_bytes()).unwrap();
    // A FIFO with no writer: a blocking open would wait for ever.
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    assert_not_a_file(&store, job, &ws);
}

#[test]
fn the_copy_ends_with_the_job_on_each_path_to_done() {
    for (by, end) in [
        (By::Agent, End::Complete),
        (By::Person, End::Complete),
        (By::Person, End::Cancel),
    ] {
        let tmp = TempDir::new("done");
        let (store, job, ws) = setup(&tmp);
        fs::write(ws.join("notes.md"), b"Quote of a mail.").unwrap();
        save_notes(&store, job, &ws, NOW).unwrap();
        // The person deleted the folder first: no reap or build reads it.
        fs::remove_dir_all(&ws).unwrap();
        finish(&store, job, by, end, None, NOW + 1).unwrap();
        assert_eq!(notes(&store, job).unwrap(), None, "{by:?} {end:?}");
        // No later save writes a copy.
        build(&store, job, &ws).unwrap();
        assert_eq!(fs::read(ws.join("notes.md")).unwrap(), b"# Notes\n");
        assert_eq!(save_notes(&store, job, &ws, NOW + 2).unwrap(), Notes::Done);
        assert!(!iris_job::save_notes(&store, job, b"x", NOW + 2).unwrap());
        assert_eq!(notes(&store, job).unwrap(), None);
    }
}
