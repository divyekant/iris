//! Card R5-10: each hostile sample of the synthetic test set goes to
//! quarantine, and no other sample does. Quarantine is the treatment of
//! `hostile` mail (spec 1 section 6.7): the sender kind is `hostile`, or the
//! message has a hostile mark. The fake model never says `phishing`, so the
//! fixed checks alone must find the hostile samples.
//!
//! The run is the engine run: the batch runner of the cost-control spec,
//! newest thread first, with the sender refresh after each batch.

use std::path::PathBuf;

use iris_model::{Error as ModelError, Reply, Stop, Usage};
use iris_store::{NewMessage, Store};
use iris_synth::{Mailbox, Sender, OWNER};
use iris_understand::batch::{self, Limits};
use iris_understand::budget::ModelRow;
use iris_understand::Model;

const NOW: i64 = 1_767_225_600_000; // 2026-01-01

struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A model that finds nothing, and never says `phishing`.
struct Blind;

impl Model for Blind {
    fn name(&self) -> &str {
        "fake-1"
    }

    fn ask(&self, _: &str, _: &str) -> Result<Reply, ModelError> {
        Ok(Reply {
            stop: Stop::End,
            text: r#"{"items": [], "phishing": false}"#.into(),
            usage: Usage::default(),
        })
    }
}

#[test]
fn each_hostile_sample_goes_to_quarantine_and_no_other_sample_does() {
    let dir = TempDir(std::env::temp_dir().join(format!("iris-quarantine-{}", std::process::id())));
    let _ = std::fs::remove_dir_all(&dir.0);
    let store = Store::open(&dir.0).unwrap();
    let acc = store.add_account("gmail", OWNER).unwrap();
    store
        .put_labels(
            acc,
            &[("INBOX", "INBOX", "system"), ("SENT", "SENT", "system")],
        )
        .unwrap();
    store
        .write(|tx| {
            tx.execute(
                "INSERT INTO model_setting(id, provider, base_url, model, version, max_tokens,
                   timeout_s) VALUES (1, 'local', 'http://127.0.0.1:1/v1', 'fake-1', 1, 100, 30)",
                [],
            )?;
            Ok(())
        })
        .unwrap();

    let mb = Mailbox::new(3, 3_000);
    let mut ids = Vec::with_capacity(mb.len());
    let all: Vec<usize> = (0..mb.len()).collect();
    for chunk in all.chunks(1000) {
        let batch: Vec<NewMessage> = chunk
            .iter()
            .map(|&i| {
                let t = mb.truth(i);
                let raw = mb.raw(i);
                let label = if t.sender == Sender::Owner {
                    "SENT"
                } else {
                    "INBOX"
                };
                NewMessage {
                    provider_id: format!("m{i}"),
                    thread_provider_id: format!("t{}", t.thread),
                    content_sha256: Some(store.put_content(&raw).unwrap()),
                    size: Some(raw.len() as i64),
                    internal_date: Some(t.date * 1000),
                    labels: vec![label.to_string()],
                    ..NewMessage::default()
                }
            })
            .collect();
        ids.extend(store.put_messages(acc, &batch).unwrap());
    }

    let connect = |_: &ModelRow| -> Result<Box<dyn Model>, String> { Ok(Box::new(Blind)) };
    // A sender that becomes hostile late makes its mail pending again, so
    // the runs go on until one run reads no message.
    for _ in 0..10 {
        let r = batch::run(&store, &connect, &Limits::default(), NOW).unwrap();
        assert!(matches!(r.stop.as_deref(), None | Some("done")), "{r:?}");
        if r.messages == 0 {
            break;
        }
    }

    let (mut hostile, mut missed, mut wrong) = (0, Vec::new(), Vec::new());
    store
        .read(|c| {
            let mut st = c.prepare(
                "SELECT coalesce(m.hostile, '[]') <> '[]'
                     OR coalesce((SELECT s.kind = 'hostile' FROM header h
                                  JOIN sender s ON s.address = h.from_addr
                                  WHERE h.message_id = m.id), 0),
                        m.understood_at IS NOT NULL,
                        (SELECT count(*) FROM item_source src JOIN item i ON i.id = src.item_id
                         WHERE src.message_id = m.id AND src.role = 'origin'
                           AND i.form <> 'conversation')
                 FROM message m WHERE m.id = ?1",
            )?;
            for (i, id) in ids.iter().enumerate() {
                let (quarantined, understood, items): (bool, bool, i64) =
                    st.query_row([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
                assert!(understood, "message {i} is not understood");
                let t = mb.truth(i);
                if t.hostile() {
                    hostile += 1;
                    if !quarantined || items > 0 {
                        missed.push((i, t.traits));
                    }
                } else if quarantined {
                    wrong.push((i, t.sender));
                }
            }
            Ok(())
        })
        .unwrap();
    assert!(hostile >= 30, "the sample has hostile mail: {hostile}");
    assert!(
        missed.is_empty(),
        "{} of {hostile} hostile samples are not in quarantine: {missed:?}",
        missed.len()
    );
    assert!(wrong.is_empty(), "other samples in quarantine: {wrong:?}");
}
