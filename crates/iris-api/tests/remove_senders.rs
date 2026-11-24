//! The sender kinds after the delete of a removed account
//! (docs/specs/2026-11-24-remove-account.md, step 5). Invariant: after the
//! delete, each engine sender row agrees with the evidence that stays in
//! all accounts, as a new refresh derives it.

use iris_store::{Header, NewMessage, Store};

const NOW: i64 = 1_800_000_000_000;

fn mail(store: &Store, id: &str, label: &str, from: &str, to: &str) -> (NewMessage, Vec<u8>) {
    let raw = format!("From: {from}\r\nTo: {to}\r\nSubject: s{id}\r\n\r\nhello\r\n").into_bytes();
    let m = NewMessage {
        provider_id: id.into(),
        thread_provider_id: format!("t-{id}"),
        content_sha256: Some(store.put_content(&raw).unwrap()),
        labels: vec![label.into()],
        header: Header {
            date: Some(NOW),
            from_addr: Some(from.into()),
            to_addrs: Some(to.into()),
            subject: Some(format!("s{id}")),
            ..Default::default()
        },
        ..Default::default()
    };
    (m, raw)
}

fn senders(store: &Store) -> Vec<(String, String)> {
    store
        .read(|c| {
            let mut q = c.prepare("SELECT address, kind FROM sender ORDER BY address")?;
            let v = q
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?;
            Ok(v)
        })
        .unwrap()
}

#[test]
fn the_delete_keeps_each_sender_kind_that_the_evidence_of_another_account_supports() {
    let dir = std::env::temp_dir().join(format!("iris-api-remove-senders-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let a = store.add_account("gmail", "a@example.org").unwrap();
    let b = store.add_account("gmail", "b@example.org").unwrap();
    let mut all = Vec::new();
    for (acc, list) in [
        // Account a: sent mail to Dana and Eve, and mail from Dana.
        (a, vec![("1", "SENT", "a@example.org", "dana@example.com,eve@example.com"),
                 ("2", "INBOX", "dana@example.com", "a@example.org")]),
        // Account b: sent mail to Dana only.
        (b, vec![("1", "SENT", "b@example.org", "dana@example.com")]),
    ] {
        store
            .put_labels(acc, &[("INBOX", "INBOX", "system"), ("SENT", "SENT", "system")])
            .unwrap();
        let (msgs, raws): (Vec<_>, Vec<_>) = list
            .iter()
            .map(|(id, label, from, to)| mail(&store, id, label, from, to))
            .unzip();
        let ids = store.put_messages(acc, &msgs).unwrap();
        all.extend(ids.into_iter().zip(raws));
    }
    store
        .write(|tx| {
            for (id, raw) in &all {
                iris_senders::decide(tx, *id, raw, NOW)?;
            }
            Ok(())
        })
        .unwrap();
    let before = senders(&store);
    assert!(before.contains(&("dana@example.com".into(), "person".into())), "{before:?}");
    assert!(before.contains(&("eve@example.com".into(), "person".into())), "{before:?}");

    store.remove_account("a@example.org", NOW).unwrap();
    iris_api::delete_removed(&store, Some(a)).unwrap().unwrap();
    let after = senders(&store);
    // Dana stays a person: account b sent mail to Dana. Eve was known only
    // through the mail of account a, so the row goes.
    assert!(after.contains(&("dana@example.com".into(), "person".into())), "{after:?}");
    assert!(!after.iter().any(|(a, _)| a == "eve@example.com"), "{after:?}");
    // A new refresh changes nothing: the rows agree with the evidence.
    store
        .write(|tx| Ok(iris_senders::refresh(tx, NOW + 1)?))
        .unwrap();
    assert_eq!(senders(&store), after);
    let _ = std::fs::remove_dir_all(&dir);
}
