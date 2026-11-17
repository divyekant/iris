//! A search of a job session (`Query::run_granted`) decides each match
//! only from the granted messages and the granted items.

use std::collections::BTreeSet;

use iris_store::{Query, Store};

/// Each grant of 3 messages and 3 items (64 grants), with items of two
/// forms whose sources span the messages: each match of `form:<f>` is a
/// granted message that is a source of a granted item of form `<f>`, and
/// each such message matches.
#[test]
fn form_matches_only_through_a_granted_item() {
    let dir = std::env::temp_dir().join(format!("iris-granted-search-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    // Item 1 (record): messages 1 and 2. Item 2 (commitment): message 2.
    // Item 3 (record): messages 2 and 3.
    let sources: [(i64, &str, &[i64]); 3] = [
        (1, "record", &[1, 2]),
        (2, "commitment", &[2]),
        (3, "record", &[2, 3]),
    ];
    store
        .write(|tx| {
            tx.execute_batch(
                "INSERT INTO account (id, provider, address) VALUES (1, 'gmail', 'me@example.com');
                 INSERT INTO thread (id, account_id, provider_id) VALUES (1, 1, 't');
                 INSERT INTO message (id, account_id, thread_id, provider_id, internal_date) VALUES
                   (1, 1, 1, 'a', 1), (2, 1, 1, 'b', 2), (3, 1, 1, 'c', 3);",
            )?;
            for (i, form, ms) in sources {
                tx.execute(
                    "INSERT INTO item (id, form, account_id, status, doc, created_at, updated_at)
                     VALUES (?1, ?2, 1, 'open', '{}', 1, 1)",
                    rusqlite::params![i, form],
                )?;
                for m in ms {
                    tx.execute(
                        "INSERT INTO item_source (item_id, message_id, role) VALUES (?1, ?2, 'origin')",
                        [i, *m],
                    )?;
                }
            }
            Ok(())
        })
        .unwrap();
    let set =
        |bits: u32| -> BTreeSet<i64> { (1..=3).filter(|n| bits & (1 << (n - 1)) != 0).collect() };
    for mb in 0..8 {
        for ib in 0..8 {
            let (messages, items) = (set(mb), set(ib));
            for form in ["record", "commitment"] {
                let q = Query::parse(&format!("form:{form}")).unwrap();
                let got: BTreeSet<i64> = store
                    .read(|c| q.run_granted(c, 50, &messages, &items))
                    .unwrap()
                    .into_iter()
                    .collect();
                let want: BTreeSet<i64> = sources
                    .iter()
                    .filter(|(i, f, _)| *f == form && items.contains(i))
                    .flat_map(|(_, _, ms)| ms.iter().copied())
                    .filter(|m| messages.contains(m))
                    .collect();
                assert_eq!(got, want, "{form}, messages {messages:?}, items {items:?}");
            }
        }
    }
    // A workspace holds no labels.
    let q = Query::parse("label:INBOX").unwrap();
    assert!(q.check_granted().is_err());
    assert!(store
        .read(|c| q.run_granted(c, 50, &set(7), &set(7)))
        .is_err());
}
