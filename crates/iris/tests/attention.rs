//! One turn of the attention loop of the engine (issue #733).

use iris_store::Store;

fn col(store: &Store, sql: &str) -> i64 {
    store
        .read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
        .unwrap()
}

/// A gate answer changes a sender kind while no understanding run is
/// active. One turn computes the conversation of the marked thread again
/// and empties the `talk_due` queue.
#[test]
fn a_turn_updates_the_conversations_that_a_kind_change_marked() {
    let dir = std::env::temp_dir().join(format!("iris-attention-turn-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let account = store.add_account("gmail", "owner@example.com").unwrap();
    store
        .write(|tx| {
            tx.execute(
                "INSERT INTO thread(account_id, provider_id) VALUES (?1, 't1')",
                [account],
            )?;
            tx.execute(
                "INSERT INTO message(account_id, thread_id, provider_id, understood_at)
                 VALUES (?1, 1, 'm1', 1)",
                [account],
            )?;
            tx.execute(
                "INSERT INTO header(message_id, date, from_addr) VALUES (1, 1, 'ana@example.org')",
                [],
            )?;
            // The mark of an understanding run that stopped before its end.
            tx.execute("INSERT INTO talk_due(thread_id) VALUES (1)", [])?;
            Ok(())
        })
        .unwrap();
    let mut mark = None;
    iris::engine::attention_turn(&store, &mut mark);
    let conversation = "SELECT count(*) FROM item WHERE form = 'conversation' AND status <> 'gone'";
    assert_eq!(
        col(&store, conversation),
        1,
        "a stranger starts a conversation"
    );
    assert_eq!(col(&store, "SELECT count(*) FROM talk_due"), 0);

    // The answer of a gate: Ana is hostile. Hostile mail does not count.
    store
        .write(|tx| {
            tx.execute(
                "INSERT INTO sender(address, kind, decided_by) VALUES ('ana@example.org', 'hostile', 'person')
                 ON CONFLICT(address) DO UPDATE SET kind = 'hostile', decided_by = 'person'",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    assert_eq!(col(&store, "SELECT count(*) FROM talk_due"), 1);
    iris::engine::attention_turn(&store, &mut mark);
    assert_eq!(col(&store, "SELECT count(*) FROM talk_due"), 0);
    assert_eq!(col(&store, conversation), 0, "the conversation is gone");
    let _ = std::fs::remove_dir_all(&dir);
}
