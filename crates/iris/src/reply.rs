//! `iris write`: a draft of a reply in your voice (card R3-05, crate
//! `iris-voice`). The model call goes through the one budget of
//! understanding (docs/specs/2026-11-16-cost-control.md): the command makes
//! an `understand_run` row with the prompt `draft/0.1` for the counts of
//! the call, and the `Budgeted` model reserves the worst case first.

use iris_store::{parse_id, Kind, Store};
use iris_understand::batch;
use iris_understand::budget::{model_row, Budgeted, ModelRow};
use iris_understand::Model;
use rusqlite::{params, OptionalExtension};

/// Makes the model of a `model_setting` row.
pub type Connect<'a> = &'a dyn Fn(&ModelRow) -> Result<Box<dyn Model>, String>;

/// The thread of `id`: a thread (`t-42`), or the thread of a message (`m-42`).
fn thread_of(store: &Store, id: &str) -> Result<i64, String> {
    match parse_id(id) {
        Some((Kind::Thread, t)) => Ok(t),
        Some((Kind::Message, m)) => store
            .read(|c| {
                Ok(
                    c.query_row("SELECT thread_id FROM message WHERE id = ?1", [m], |r| {
                        r.get(0)
                    })
                    .optional()?,
                )
            })
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("no message {id}")),
        _ => Err(format!("{id} is not a thread (t-42) or a message (m-42)")),
    }
}

/// Writes and saves the draft, and gives the text to print.
pub fn write(
    store: &Store,
    id: &str,
    intent: Option<&str>,
    connect: Connect,
    now_ms: i64,
) -> Result<String, String> {
    let thread = thread_of(store, id)?;
    // The lock of cost control: no `iris model set` and no budget approval
    // can change the model row or the budget during the call.
    // shortcut: a running understanding run makes this command fail at once, wait for the lock if this bothers the owner.
    let _lock = batch::lock(store).map_err(|e| match e {
        batch::Error::Busy => {
            "an understanding run or a model change is in progress: try again in a minute"
                .to_string()
        }
        e => e.to_string(),
    })?;
    let row = store
        .read(|c| Ok(model_row(c)?))
        .map_err(|e| e.to_string())?
        .ok_or("No model is set. Use `iris model set`.")?;
    let model = connect(&row)?;
    let run = store
        .write(|tx| {
            tx.execute(
                "INSERT INTO understand_run(started_at, model, prompt, cost_micro_usd,
                                            unknown_cost_micro_usd)
                 VALUES (?1, ?2, ?3, ?4, ?4)",
                params![
                    now_ms,
                    row.model,
                    iris_voice::PROMPT,
                    row.price.is_some().then_some(0)
                ],
            )?;
            Ok(tx.last_insert_rowid())
        })
        .map_err(|e| e.to_string())?;
    let budgeted = Budgeted::new(model.as_ref(), store, run, Some(&row));
    let result = iris_voice::write(store, thread, intent, &budgeted, now_ms);
    let s = budgeted.state();
    // The same stop names as a run of understanding. A valid draft can
    // still have an unknown charge or a cost above its reservation.
    let stop = if s.refused || s.overrun {
        "budget"
    } else if s.unknown {
        "unknown"
    } else if s.key_refused || s.server_failed {
        "model"
    } else if result.is_ok() {
        "done"
    } else {
        "error"
    };
    // The wait after a model error is shared with understanding: only
    // evidence of the model changes it. An error of the draft itself (a
    // hostile thread, an invalid answer) is no evidence against the model.
    let wait = match stop {
        "budget" => batch::Stop::Budget,
        "unknown" => batch::Stop::Unknown,
        "model" => batch::Stop::Model,
        _ => batch::Stop::Done,
    };
    let end = store.write(|tx| {
        let ended = now_ms.max(crate::engine::now_ms());
        tx.execute(
            "UPDATE understand_run SET ended_at = ?2, stop = ?3 WHERE id = ?1",
            params![run, ended, stop],
        )?;
        let r = batch::report(tx, run)?;
        batch::end_wait(tx, &wait, &r, ended)?;
        Ok(())
    });
    if let Some(e) = budgeted.take_error() {
        return Err(format!("store: {e}"));
    }
    let saved = result.map_err(|e| match stop {
        "budget" => {
            format!("{e}\nApprove a budget first: `iris understand --estimate`, then `--approve`.")
        }
        _ => e.to_string(),
    })?;
    end.map_err(|e| e.to_string())?;
    // Spec 2 section 8.3: the labeled fields first, then the text.
    let mut out = format!(
        "Thread: t-{thread}\nBased on: m-{}\nSamples: {}\nSaved: yes\nSent: no\n",
        saved.based_on, saved.samples
    );
    if s.unknown {
        out += "Warning: the cost of the call is not known. Iris kept its worst case from the budget.\n";
    }
    if s.overrun {
        out += "Warning: the call cost more than its reservation. The budget left can be below zero.\n";
    }
    out += &format!("\n{}\n", saved.body);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use iris_model::{Reply, Stop, Usage};
    use iris_store::{Body, Header, NewMessage};

    use super::*;

    /// A model that answers a valid draft with this usage.
    struct Fake(Usage);

    impl Model for Fake {
        fn name(&self) -> &str {
            "fake"
        }

        fn ask(&self, _: &str, _: &str) -> Result<Reply, iris_model::Error> {
            Ok(Reply {
                text: r#"{"body": "Hi Ana, yes. D"}"#.into(),
                usage: self.0,
                stop: Stop::End,
            })
        }
    }

    #[test]
    fn the_call_goes_through_the_budget() {
        let dir = std::env::temp_dir().join(format!("iris-reply-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Store::open(&dir).unwrap();
        let account = store.add_account("gmail", "me@example.com").unwrap();
        store
            .put_labels(account, &[("INBOX", "INBOX", "system")])
            .unwrap();
        let ids = store
            .put_messages(
                account,
                &[NewMessage {
                    provider_id: "in".into(),
                    thread_provider_id: "t1".into(),
                    internal_date: Some(1),
                    labels: vec!["INBOX".into()],
                    header: Header {
                        from_addr: Some("ana@example.org".into()),
                        ..Default::default()
                    },
                    body: Some(Body {
                        subject: "Hi".into(),
                        sender: "ana@example.org".into(),
                        text: "Lunch?".into(),
                    }),
                    ..Default::default()
                }],
            )
            .unwrap();
        store
            .write(|tx| {
                tx.execute("UPDATE message SET understood_at = 1", [])?;
                Ok(())
            })
            .unwrap();
        // A priced model with no approved budget: the call is refused.
        store
            .write(|tx| {
                tx.execute(
                    "INSERT INTO model_setting(id, provider, base_url, model, version, price_in,
                       price_out, max_tokens, timeout_s, budget_micro_usd)
                     VALUES (1, 'anthropic', 'https://api.anthropic.com/v1', 'm', 1, 3000000,
                       15000000, 1000, 60, NULL)",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        let usage = std::cell::Cell::new(Usage {
            input_tokens: 1000,
            output_tokens: 100,
            cost_micro_usd: None,
            known: true,
        });
        let connect =
            |_: &ModelRow| -> Result<Box<dyn Model>, String> { Ok(Box::new(Fake(usage.get()))) };
        let id = format!("m-{}", ids[0]);
        let e = write(&store, &id, None, &connect, 10).unwrap_err();
        assert!(e.contains("Approve a budget"), "{e}");
        let budget = |store: &Store| {
            store
                .read(|c| {
                    Ok(
                        c.query_row("SELECT budget_micro_usd FROM model_setting", [], |r| {
                            r.get::<_, Option<i64>>(0)
                        })?,
                    )
                })
                .unwrap()
        };
        assert_eq!(budget(&store), None);

        // With a budget: the draft is saved and the cost is taken.
        store
            .write(|tx| {
                tx.execute("UPDATE model_setting SET budget_micro_usd = 1000000", [])?;
                Ok(())
            })
            .unwrap();
        let out = write(&store, "t-1", None, &connect, 20).unwrap();
        assert!(out.starts_with("Thread: t-1\nBased on: m-1\n"), "{out}");
        assert!(out.ends_with("\n\nHi Ana, yes. D\n"), "{out}");
        // 1000 tokens in at 3 USD and 100 out at 15 USD for a million.
        assert_eq!(budget(&store), Some(1_000_000 - 3_000 - 1_500));
        let runs: Vec<(String, String, i64)> = store
            .read(|c| {
                let mut st =
                    c.prepare("SELECT prompt, stop, calls FROM understand_run ORDER BY id")?;
                let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
                Ok(rows.collect::<rusqlite::Result<_>>()?)
            })
            .unwrap();
        assert_eq!(
            runs,
            [
                ("draft/0.1".into(), "budget".into(), 0),
                ("draft/0.1".into(), "done".into(), 1)
            ]
        );
        assert!(write(&store, "c-1", None, &connect, 30).is_err());

        // A valid draft with an unknown charge, then with a cost above its
        // reservation: the run row says so.
        usage.set(Usage {
            known: false,
            ..usage.get()
        });
        let out = write(&store, "t-1", None, &connect, 40).unwrap();
        assert!(out.contains("cost of the call is not known"), "{out}");
        usage.set(Usage {
            input_tokens: 1000,
            output_tokens: 10_000_000,
            cost_micro_usd: None,
            known: true,
        });
        store
            .write(|tx| {
                tx.execute("UPDATE model_setting SET budget_micro_usd = 100000000", [])?;
                Ok(())
            })
            .unwrap();
        let out = write(&store, "t-1", None, &connect, 50).unwrap();
        assert!(out.contains("more than its reservation"), "{out}");
        let stops: Vec<String> = store
            .read(|c| {
                let mut st = c.prepare("SELECT stop FROM understand_run ORDER BY id")?;
                let rows = st.query_map([], |r| r.get(0))?;
                Ok(rows.collect::<rusqlite::Result<_>>()?)
            })
            .unwrap();
        assert_eq!(stops, ["budget", "done", "unknown", "budget"]);

        // The lock of cost control is held: no call.
        let _lock = batch::lock(&store).unwrap();
        let e = write(&store, "t-1", None, &connect, 60).unwrap_err();
        assert!(e.contains("in progress"), "{e}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A model that fails with a server error, or answers a valid draft.
    struct Flaky(bool);

    impl Model for Flaky {
        fn name(&self) -> &str {
            "flaky"
        }

        fn ask(&self, _: &str, _: &str) -> Result<Reply, iris_model::Error> {
            if self.0 {
                return Err(iris_model::Error::Server(500));
            }
            Ok(Reply {
                text: r#"{"body": "ok"}"#.into(),
                usage: Usage {
                    known: true,
                    ..Usage::default()
                },
                stop: Stop::End,
            })
        }
    }

    #[test]
    fn a_draft_call_updates_the_shared_model_wait() {
        let dir = std::env::temp_dir().join(format!("iris-reply-wait-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Store::open(&dir).unwrap();
        let account = store.add_account("gmail", "me@example.com").unwrap();
        store
            .put_labels(account, &[("INBOX", "INBOX", "system")])
            .unwrap();
        store
            .put_messages(
                account,
                &[NewMessage {
                    provider_id: "in".into(),
                    thread_provider_id: "t1".into(),
                    labels: vec!["INBOX".into()],
                    body: Some(Body {
                        subject: "Hi".into(),
                        sender: "ana@example.org".into(),
                        text: "Lunch?".into(),
                    }),
                    ..Default::default()
                }],
            )
            .unwrap();
        store
            .write(|tx| {
                tx.execute("UPDATE message SET understood_at = 1", [])?;
                tx.execute(
                    "INSERT INTO model_setting(id, provider, base_url, model, version,
                       max_tokens, timeout_s)
                     VALUES (1, 'local', 'http://127.0.0.1:1/v1', 'm', 1, 1000, 60)",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        let wait = |store: &Store| {
            store
                .read(|c| {
                    Ok(c.query_row(
                        "SELECT retry_failures, retry_at IS NOT NULL FROM model_setting",
                        [],
                        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, bool>(1)?)),
                    )?)
                })
                .unwrap()
        };
        let fail = |_: &ModelRow| -> Result<Box<dyn Model>, String> { Ok(Box::new(Flaky(true))) };
        assert!(write(&store, "t-1", None, &fail, 10).is_err());
        assert_eq!(wait(&store), (1, true));
        let ok = |_: &ModelRow| -> Result<Box<dyn Model>, String> { Ok(Box::new(Flaky(false))) };
        write(&store, "t-1", None, &ok, 20).unwrap();
        assert_eq!(wait(&store), (0, false));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
