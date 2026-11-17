use std::fs;
use std::path::PathBuf;

use iris_store::{Body, Error, Header, NewMessage, Store};

/// A new empty folder for one test, removed at the end.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("iris-store-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn msg(
    id: &str,
    thread: &str,
    date: i64,
    labels: &[&str],
    subject: &str,
    text: &str,
) -> NewMessage {
    NewMessage {
        provider_id: id.into(),
        thread_provider_id: thread.into(),
        internal_date: Some(date),
        labels: labels.iter().map(|l| l.to_string()).collect(),
        header: Header {
            from_addr: Some(format!("{id}@example.com")),
            from_name: Some("Ana Lee".into()),
            to_addrs: Some("owner@example.com".into()),
            subject: Some(subject.into()),
            ..Header::default()
        },
        body: Some(Body {
            subject: subject.into(),
            sender: format!("Ana Lee {id}@example.com"),
            text: text.into(),
        }),
        ..NewMessage::default()
    }
}

fn setup(dir: &TempDir) -> (Store, i64) {
    let store = Store::open(&dir.0).unwrap();
    let acc = store.add_account("gmail", "owner@example.com").unwrap();
    store
        .put_labels(
            acc,
            &[("INBOX", "INBOX", "system"), ("Label_1", "Taxes", "user")],
        )
        .unwrap();
    (store, acc)
}

#[cfg(unix)]
fn mode(p: &std::path::Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(p).unwrap().permissions().mode() & 0o777
}

#[test]
fn open_makes_the_data_folder() {
    let dir = TempDir::new("open");
    let store = Store::open(&dir.0).unwrap();
    assert!(dir.0.join("iris.db").is_file());
    assert!(dir.0.join("content").is_dir());
    assert_eq!(fs::read(dir.0.join("key")).unwrap().len(), 32);
    #[cfg(unix)]
    {
        assert_eq!(mode(&dir.0), 0o700);
        assert_eq!(mode(&dir.0.join("key")), 0o600);
        assert_eq!(mode(&dir.0.join("content")), 0o700);
    }
    store
        .read(|c| {
            assert_eq!(iris_store::schema_version(c)?, 35);
            let journal: String = c.query_row("PRAGMA journal_mode", [], |r| r.get(0))?;
            assert_eq!(journal, "wal");
            let cache: i64 = c.query_row("PRAGMA cache_size", [], |r| r.get(0))?;
            assert_eq!(cache, -2048);
            let mmap: i64 = c.query_row("PRAGMA mmap_size", [], |r| r.get(0))?;
            assert_eq!(mmap, 0);
            Ok(())
        })
        .unwrap();
    store
        .write(|tx| {
            let sync: i64 = tx.query_row("PRAGMA synchronous", [], |r| r.get(0))?;
            assert_eq!(sync, 2, "FULL");
            let fk: i64 = tx.query_row("PRAGMA foreign_keys", [], |r| r.get(0))?;
            assert_eq!(fk, 1);
            Ok(())
        })
        .unwrap();
}

#[test]
fn secrets_survive_a_new_start_and_need_the_key() {
    let dir = TempDir::new("secret");
    let sealed = Store::open(&dir.0).unwrap().seal(b"refresh-token").unwrap();
    assert!(!sealed.windows(13).any(|w| w == b"refresh-token"));
    let store = Store::open(&dir.0).unwrap();
    assert_eq!(store.unseal(&sealed).unwrap(), b"refresh-token");

    let mut bad = sealed.clone();
    *bad.last_mut().unwrap() ^= 1;
    assert!(matches!(store.unseal(&bad), Err(Error::Key(_))));
    assert!(matches!(store.unseal(&sealed[..10]), Err(Error::Key(_))));

    let other = TempDir::new("secret-other");
    assert!(Store::open(&other.0).unwrap().unseal(&sealed).is_err());

    drop(store);
    fs::write(dir.0.join("key"), b"short").unwrap();
    assert!(matches!(Store::open(&dir.0), Err(Error::Key(_))));
}

#[test]
fn refuses_an_unknown_schema() {
    let dir = TempDir::new("newer");
    let store = Store::open(&dir.0).unwrap();
    store
        .write(|tx| Ok(tx.pragma_update(None, "user_version", 99)?))
        .unwrap();
    drop(store);
    assert!(matches!(Store::open(&dir.0), Err(Error::UnknownSchema(99))));
    // SQLite accepts a negative version. The store must refuse it too.
    let conn = rusqlite::Connection::open(dir.0.join("iris.db")).unwrap();
    conn.pragma_update(None, "user_version", -1).unwrap();
    drop(conn);
    assert!(matches!(Store::open(&dir.0), Err(Error::UnknownSchema(-1))));
}

#[test]
fn search_words_phrases_and_filters() {
    let dir = TempDir::new("search");
    let (store, acc) = setup(&dir);
    let day = 86_400_000;
    let t0 = 1_735_689_600_000; // 2025-01-01
    let ids = store
        .put_messages(
            acc,
            &[
                msg(
                    "a",
                    "t1",
                    t0,
                    &["INBOX"],
                    "Invoice 42",
                    "Please pay the invoice by Friday.",
                ),
                msg(
                    "b",
                    "t1",
                    t0 + day,
                    &["INBOX", "Label_1"],
                    "Re: Invoice 42",
                    "Paid. Café receipt attached.",
                ),
                msg(
                    "c",
                    "t2",
                    t0 + 2 * day,
                    &[],
                    "Lunch",
                    "Friday lunch at noon?",
                ),
            ],
        )
        .unwrap();
    let [a, b, c] = ids[..] else { panic!() };
    let s = |q: &str| store.search(q, 10).unwrap();

    assert_eq!(s("invoice"), [b, a], "newest first");
    assert_eq!(s("friday"), [c, a]);
    assert_eq!(s("\"pay the invoice\""), [a]);
    assert_eq!(s("\"invoice pay\""), Vec::<i64>::new());
    assert_eq!(s("cafe"), [b], "diacritics removed");
    assert_eq!(s("inv"), Vec::<i64>::new(), "a word is not a prefix search");
    assert_eq!(s("label:taxes"), [b]);
    assert_eq!(s("label:INBOX invoice"), [b, a]);
    assert_eq!(s("from:c@example"), [c]);
    assert_eq!(s("from:\"ana lee\" friday"), [c, a]);
    assert_eq!(s("to:owner"), [c, b, a]);
    assert_eq!(s("FROM:x"), Vec::<i64>::new(), "filter names are lowercase");
    // `%` and `_` are plain characters in a filter value, not LIKE patterns.
    assert_eq!(s("from:%"), Vec::<i64>::new());
    assert_eq!(s("from:_@"), Vec::<i64>::new());
    assert_eq!(s("after:2025-01-02"), [c, b]);
    assert_eq!(s("before:2025-01-02"), [a]);
    assert_eq!(s(""), [c, b, a]);
    assert_eq!(store.search("invoice", 1).unwrap(), [b]);
    // FTS5 operators are plain words.
    assert_eq!(s("invoice OR lunch"), Vec::<i64>::new());
    assert_eq!(s("NEAR(invoice friday) * ^pay \""), Vec::<i64>::new());

    // form: uses the items of the message.
    store
        .write(|tx| {
            tx.execute(
                "INSERT INTO item(form, account_id, status, doc, created_at, updated_at) VALUES ('record', ?1, 'open', '{}', 0, 0)",
                [acc],
            )?;
            tx.execute(
                "INSERT INTO item_source(item_id, message_id, role) VALUES (last_insert_rowid(), ?1, 'origin')",
                [b],
            )?;
            Ok(())
        })
        .unwrap();
    assert_eq!(s("form:record"), [b]);
    assert_eq!(s("form:feed"), Vec::<i64>::new());

    // A message that is gone is not in the results.
    store
        .write(|tx| Ok(tx.execute("UPDATE message SET gone_at = 1 WHERE id = ?1", [a])?))
        .unwrap();
    assert_eq!(s("invoice"), [b]);
}

#[test]
fn put_again_keeps_the_id_and_updates_the_index() {
    let dir = TempDir::new("update");
    let (store, acc) = setup(&dir);
    let first = store
        .put_messages(
            acc,
            &[msg("a", "t1", 1, &["INBOX", "Label_1"], "Old", "apple")],
        )
        .unwrap();
    let again = store
        .put_messages(acc, &[msg("a", "t1", 1, &["INBOX"], "New", "banana")])
        .unwrap();
    assert_eq!(first, again);
    assert_eq!(store.search("apple", 10).unwrap(), Vec::<i64>::new());
    assert_eq!(store.search("banana", 10).unwrap(), first);
    assert_eq!(store.search("label:taxes", 10).unwrap(), Vec::<i64>::new());

    store
        .put_body(
            first[0],
            &Body {
                subject: "x".into(),
                sender: "y".into(),
                text: "cherry".into(),
            },
        )
        .unwrap();
    assert_eq!(store.search("banana", 10).unwrap(), Vec::<i64>::new());
    assert_eq!(store.search("cherry", 10).unwrap(), first);
}

#[test]
fn an_unknown_label_changes_nothing_for_that_label() {
    // Specification 1, section 4.3: a label that labels.list does not have
    // changes nothing for that label. The rest of the batch is written.
    let dir = TempDir::new("label");
    let (store, acc) = setup(&dir);
    let ids = store
        .put_messages(
            acc,
            &[
                msg("a", "t1", 1, &["INBOX"], "s", "apple"),
                msg("b", "t1", 2, &["Label_9", "INBOX"], "s", "apple"),
            ],
        )
        .unwrap();
    assert_eq!(
        store.search("label:INBOX apple", 10).unwrap(),
        [ids[1], ids[0]]
    );
    let n: i64 = store
        .read(|c| Ok(c.query_row("SELECT count(*) FROM message_label", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(n, 2);
}

#[test]
fn migration_of_a_version_0_database_with_data_makes_a_backup() {
    let dir = TempDir::new("v0");
    fs::create_dir_all(&dir.0).unwrap();
    let conn = rusqlite::Connection::open(dir.0.join("iris.db")).unwrap();
    conn.execute_batch("CREATE TABLE old(x); INSERT INTO old VALUES (7);")
        .unwrap();
    drop(conn);
    // The key of an earlier start. Without it, the store refuses the folder.
    fs::write(dir.0.join("key"), [7u8; 32]).unwrap();
    Store::open(&dir.0).unwrap();
    let backups: Vec<_> = fs::read_dir(&dir.0)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.to_string_lossy().ends_with(".bak"))
        .collect();
    assert_eq!(backups.len(), 1, "{backups:?}");
    let copy = rusqlite::Connection::open(&backups[0]).unwrap();
    let x: i64 = copy
        .query_row("SELECT x FROM old", [], |r| r.get(0))
        .unwrap();
    assert_eq!(x, 7);

    // A new empty database needs no backup.
    let empty = TempDir::new("v0-empty");
    Store::open(&empty.0).unwrap();
    assert!(!fs::read_dir(&empty.0).unwrap().any(|e| e
        .unwrap()
        .path()
        .to_string_lossy()
        .ends_with(".bak")));
}

#[test]
fn row_ids_are_not_used_again() {
    let dir = TempDir::new("rowid");
    let (store, acc) = setup(&dir);
    let a = store
        .put_messages(acc, &[msg("a", "t1", 1, &[], "s", "x")])
        .unwrap()[0];
    store
        .write(|tx| Ok(tx.execute("DELETE FROM message WHERE id = ?1", [a])?))
        .unwrap();
    let b = store
        .put_messages(acc, &[msg("b", "t1", 1, &[], "s", "x")])
        .unwrap()[0];
    assert!(b > a);
    assert_eq!(
        iris_store::format_id(iris_store::Kind::Message, b),
        format!("m-{b}")
    );
}

#[test]
fn content_files_are_named_by_hash() {
    let dir = TempDir::new("content");
    let store = Store::open(&dir.0).unwrap();
    let sha = store.put_content(b"Subject: hi\r\n\r\nhello\r\n").unwrap();
    assert_eq!(sha.len(), 64);
    let path = store.content_path(&sha).unwrap();
    assert!(path.starts_with(dir.0.join("content").join(&sha[..2])));
    assert_eq!(fs::read(&path).unwrap(), b"Subject: hi\r\n\r\nhello\r\n");
    assert_eq!(
        store.put_content(b"Subject: hi\r\n\r\nhello\r\n").unwrap(),
        sha
    );

    // A fetch again repairs a damaged file.
    fs::write(&path, b"damaged").unwrap();
    store.put_content(b"Subject: hi\r\n\r\nhello\r\n").unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"Subject: hi\r\n\r\nhello\r\n");

    // A writer that drops before finish leaves no file.
    let mut w = store.content_writer().unwrap();
    std::io::Write::write_all(&mut w, b"half").unwrap();
    drop(w);
    let left: Vec<_> = fs::read_dir(dir.0.join("content"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .filter(|n| n.to_string_lossy().starts_with(".tmp"))
        .collect();
    assert!(left.is_empty(), "{left:?}");

    assert_eq!(store.content_path("../../etc/passwd"), None);
    assert_eq!(store.content_path(&sha.to_uppercase()), None);
}

#[test]
fn the_store_cuts_a_long_text_before_the_index() {
    let dir = TempDir::new("cut");
    let (store, acc) = setup(&dir);
    let long = format!("{} tailword", "é".repeat(iris_store::TEXT_LIMIT));
    let mut m = msg("a", "t1", 1, &[], &"s".repeat(5_000), &long);
    m.body.as_mut().unwrap().sender = "x".repeat(5_000);
    let id = store.put_messages(acc, &[m]).unwrap()[0];
    let (text, subject, sender): (String, String, String) = store
        .read(|c| {
            Ok(c.query_row(
                "SELECT text, subject, sender FROM body WHERE message_id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?)
        })
        .unwrap();
    assert_eq!(
        text,
        format!(
            "{}{}",
            "é".repeat(iris_store::TEXT_LIMIT),
            iris_store::CUT_MARK
        )
    );
    assert_eq!(subject.chars().count(), iris_store::FIELD_LIMIT);
    assert_eq!(sender.chars().count(), iris_store::FIELD_LIMIT);
    assert_eq!(store.search("tailword", 10).unwrap(), Vec::<i64>::new());

    // A text that the clean step cut already (limit plus mark) stays the same.
    store
        .put_body(
            id,
            &Body {
                subject: "s".into(),
                sender: "x".into(),
                text: text.clone(),
            },
        )
        .unwrap();
    let again: String = store
        .read(|c| {
            Ok(
                c.query_row("SELECT text FROM body WHERE message_id = ?1", [id], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap();
    assert_eq!(again, text);
}

#[cfg(unix)]
#[test]
fn open_makes_an_existing_data_folder_private() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new("existing");
    fs::create_dir_all(dir.0.join("content")).unwrap();
    for p in [dir.0.clone(), dir.0.join("content")] {
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    }
    Store::open(&dir.0).unwrap();
    assert_eq!(mode(&dir.0), 0o700);
    assert_eq!(mode(&dir.0.join("content")), 0o700);
}

#[test]
fn a_message_found_again_is_not_gone() {
    let dir = TempDir::new("found");
    let (store, acc) = setup(&dir);
    let a = store
        .put_messages(acc, &[msg("a", "t1", 1, &[], "s", "apple")])
        .unwrap()[0];
    store
        .write(|tx| {
            Ok(tx.execute(
                "UPDATE message SET gone_at = 5, understood_at = 4 WHERE id = ?1",
                [a],
            )?)
        })
        .unwrap();
    assert_eq!(store.search("apple", 10).unwrap(), Vec::<i64>::new());
    let again = store
        .put_messages(acc, &[msg("a", "t1", 1, &[], "s", "apple")])
        .unwrap()[0];
    assert_eq!(again, a);
    assert_eq!(store.search("apple", 10).unwrap(), [a]);
    // Specification 1, section 4.4: its items run understanding again.
    let understood: Option<i64> = store
        .read(|c| {
            Ok(c.query_row(
                "SELECT understood_at FROM message WHERE id = ?1",
                [a],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(understood, None);
}

#[test]
fn an_interrupted_first_key_creation_does_not_block_the_start() {
    let dir = TempDir::new("keytmp");
    fs::create_dir_all(&dir.0).unwrap();
    // A crash during the first start left a part of a temporary key file.
    fs::write(dir.0.join("key.tmp"), b"part").unwrap();
    let store = Store::open(&dir.0).unwrap();
    assert_eq!(fs::read(dir.0.join("key")).unwrap().len(), 32);
    let sealed = store.seal(b"x").unwrap();
    drop(store);
    assert_eq!(Store::open(&dir.0).unwrap().unseal(&sealed).unwrap(), b"x");
}

#[test]
fn starts_at_the_same_time_share_one_key_and_one_schema() {
    let dir = TempDir::new("race");
    let barrier = std::sync::Barrier::new(8);
    let sealed: Vec<Vec<u8>> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let (dir, barrier) = (&dir.0, &barrier);
                s.spawn(move || {
                    barrier.wait();
                    let store = Store::open(dir).unwrap();
                    store.seal(format!("secret {i}").as_bytes()).unwrap()
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let store = Store::open(&dir.0).unwrap();
    for (i, s) in sealed.iter().enumerate() {
        assert_eq!(store.unseal(s).unwrap(), format!("secret {i}").as_bytes());
    }
}

#[cfg(unix)]
#[test]
fn open_makes_an_existing_key_private() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new("keymode");
    drop(Store::open(&dir.0).unwrap());
    let key = dir.0.join("key");
    fs::set_permissions(&key, fs::Permissions::from_mode(0o644)).unwrap();
    Store::open(&dir.0).unwrap();
    assert_eq!(mode(&key), 0o600);
}

#[test]
fn new_content_without_a_body_removes_the_old_text() {
    let dir = TempDir::new("newcontent");
    let (store, acc) = setup(&dir);
    let mut m = msg("a", "t1", 1, &[], "s", "apple");
    m.content_sha256 = Some(store.put_content(b"raw a").unwrap());
    let a = store.put_messages(acc, &[m.clone()]).unwrap()[0];
    store
        .write(|tx| Ok(tx.execute("UPDATE message SET understood_at = 4 WHERE id = ?1", [a])?))
        .unwrap();

    // The same content again keeps the text and the understanding.
    m.body = None;
    store.put_messages(acc, &[m.clone()]).unwrap();
    assert_eq!(store.search("apple", 10).unwrap(), [a]);

    // New content with no new text: the old text must not stay searchable.
    m.content_sha256 = Some(store.put_content(b"raw b").unwrap());
    store.put_messages(acc, &[m]).unwrap();
    assert_eq!(store.search("apple", 10).unwrap(), Vec::<i64>::new());
    let (bodies, understood): (i64, Option<i64>) = store
        .read(|c| {
            Ok(c.query_row(
                "SELECT (SELECT count(*) FROM body WHERE message_id = ?1), understood_at FROM message WHERE id = ?1",
                [a],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap();
    assert_eq!((bodies, understood), (0, None));
}

#[test]
fn a_new_sent_or_draft_label_clears_the_understanding() {
    let dir = TempDir::new("sentlabel");
    let (store, acc) = setup(&dir);
    store
        .put_labels(
            acc,
            &[("SENT", "SENT", "system"), ("DRAFT", "DRAFT", "system")],
        )
        .unwrap();
    let understood = |store: &Store, a: i64| -> Option<i64> {
        store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT understood_at FROM message WHERE id = ?1",
                    [a],
                    |r| r.get(0),
                )?)
            })
            .unwrap()
    };
    let mut m = msg("a", "t1", 1, &["INBOX"], "s", "apple");
    m.content_sha256 = Some(store.put_content(b"raw a").unwrap());
    let a = store.put_messages(acc, &[m.clone()]).unwrap()[0];
    let mark = |store: &Store| {
        store
            .write(|tx| Ok(tx.execute("UPDATE message SET understood_at = 4 WHERE id = ?1", [a])?))
            .unwrap();
    };
    mark(&store);
    // Another label that is not SENT or DRAFT keeps the understanding.
    m.labels = vec!["INBOX".into(), "Label_1".into()];
    store.put_messages(acc, &[m.clone()]).unwrap();
    assert_eq!(understood(&store, a), Some(4));
    // The same bytes with SENT: the sender of the message is you now.
    m.labels = vec!["SENT".into()];
    store.put_messages(acc, &[m.clone()]).unwrap();
    assert_eq!(understood(&store, a), None);
    mark(&store);
    m.labels = vec!["SENT".into(), "DRAFT".into()];
    store.put_messages(acc, &[m.clone()]).unwrap();
    assert_eq!(understood(&store, a), None);
}

#[test]
fn the_same_content_with_an_empty_header_keeps_the_saved_header() {
    let dir = TempDir::new("keepheader");
    let (store, acc) = setup(&dir);
    let mut m = msg("a", "t1", 1, &[], "Taxes 2026", "apple");
    m.content_sha256 = Some(store.put_content(b"raw a").unwrap());
    let a = store.put_messages(acc, &[m.clone()]).unwrap()[0];
    store
        .write(|tx| Ok(tx.execute("UPDATE message SET understood_at = 4 WHERE id = ?1", [a])?))
        .unwrap();
    let saved = |store: &Store| -> (Option<String>, Option<i64>) {
        store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT h.subject, m.understood_at FROM message m
                     JOIN header h ON h.message_id = m.id WHERE m.id = ?1",
                    [a],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .unwrap()
    };

    // A repair: the same bytes, fetched again, with no parsed header.
    m.header = Header::default();
    m.body = None;
    store.put_messages(acc, &[m.clone()]).unwrap();
    assert_eq!(saved(&store), (Some("Taxes 2026".into()), Some(4)));

    // New bytes: the old header is wrong, and understanding runs again.
    m.content_sha256 = Some(store.put_content(b"raw b").unwrap());
    store.put_messages(acc, &[m]).unwrap();
    assert_eq!(saved(&store), (None, None));
}

#[test]
fn a_missing_key_with_a_database_is_an_error() {
    let dir = TempDir::new("nokey");
    drop(Store::open(&dir.0).unwrap());
    fs::remove_file(dir.0.join("key")).unwrap();
    assert!(matches!(Store::open(&dir.0), Err(Error::Key(_))));
    assert!(!dir.0.join("key").exists(), "no new key");
}

#[test]
fn dry_write_gives_the_result_and_writes_nothing() {
    let dir = TempDir::new("dry");
    let store = Store::open(&dir.0).unwrap();
    let n = store
        .dry_write(|tx| {
            tx.execute(
                "INSERT INTO account(provider, address) VALUES ('gmail', 'a@x.example')",
                [],
            )?;
            Ok(tx.query_row("SELECT count(*) FROM account", [], |r| r.get::<_, i64>(0))?)
        })
        .unwrap();
    assert_eq!(n, 1);
    let after: i64 = store
        .read(|c| Ok(c.query_row("SELECT count(*) FROM account", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(after, 0);
}

#[test]
fn a_deferred_commitment_opens_on_its_date() {
    let dir = TempDir::new("deferred");
    let (store, acc) = setup(&dir);
    // 2026-11-16T12:00:00.007Z
    let now = 1_794_830_400_007;
    let add = |status: &str, before: Option<&str>, doc: &str| -> i64 {
        store
            .write(|tx| {
                Ok(tx.query_row(
                    "INSERT INTO item(form, account_id, status, status_before_gone, doc, created_at, updated_at)
                     VALUES ('commitment', ?1, ?2, ?3, ?4, 0, 0) RETURNING id",
                    rusqlite::params![acc, status, before, doc],
                    |r| r.get(0),
                )?)
            })
            .unwrap()
    };
    let past = add(
        "deferred",
        None,
        r#"{"status":"deferred","updated":"2026-11-01T09:00:00.000Z","deferred_until":"2026-11-15","kind":"task"}"#,
    );
    let today = add(
        "deferred",
        None,
        r#"{"status":"deferred","deferred_until":"2026-11-16"}"#,
    );
    let later = add(
        "deferred",
        None,
        r#"{"status":"deferred","deferred_until":"2026-11-17"}"#,
    );
    let gone = add(
        "gone",
        Some("deferred"),
        r#"{"status":"deferred","deferred_until":"2026-11-15"}"#,
    );

    assert_eq!(store.reopen_deferred(now).unwrap(), 2);
    let row = |id: i64| -> (String, String, i64) {
        store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT status, doc, updated_at FROM item WHERE id = ?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )?)
            })
            .unwrap()
    };
    let (status, doc, at) = row(past);
    assert_eq!(status, "open");
    assert_eq!(
        doc,
        r#"{"status":"open","updated":"2026-11-16T12:00:00.007Z","kind":"task"}"#
    );
    assert_eq!(at, now);
    assert_eq!(row(today).0, "open");
    assert_eq!(row(later).0, "deferred");
    assert_eq!(row(gone).0, "gone");
    // A second run finds nothing to change.
    assert_eq!(store.reopen_deferred(now).unwrap(), 0);
}

#[test]
fn a_sender_lookup_without_case_uses_an_index() {
    // Invariant: each `address = ? COLLATE NOCASE` lookup of a sender is an
    // index search, not a scan of the sender table.
    let dir = std::env::temp_dir().join(format!("iris-store-nocase-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let plan: Vec<String> = store
        .read(|c| {
            Ok(c.prepare(
                "EXPLAIN QUERY PLAN SELECT kind FROM sender WHERE address = ?1 COLLATE NOCASE",
            )?
            .query_map(["A@x.example"], |r| r.get(3))?
            .collect::<rusqlite::Result<_>>()?)
        })
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        plan.iter()
            .any(|p| p.contains("USING INDEX sender_address_nocase")),
        "{plan:?}"
    );
}
