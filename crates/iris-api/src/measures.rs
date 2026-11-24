//! The measures of the manifesto, "How we measure Iris" (card R5-12). The
//! engine computes each measure from the store on this machine. Nothing
//! leaves the machine.

use std::sync::{Arc, Mutex};

use axum::extract::{Request, State};
use axum::http::Method;
use axum::middleware::Next;
use axum::response::Response;
use iris_store::Store;
use rusqlite::{params, OptionalExtension};
use serde::Serialize;

use crate::{rfc3339, Error, Result};

/// The header of each request of the web client: the local date of the
/// person, `YYYY-MM-DD`.
pub const DAY: &str = "x-iris-day";

/// The number of the newest days of use that [`measures`] gives.
pub const DAYS: u32 = 28;

#[derive(Debug, Serialize, PartialEq)]
pub struct Measures {
    /// The newest days of use, the newest first. A day with no row is a
    /// day on which the person did not use the web client.
    pub days: Vec<Day>,
    /// From the first account to the first brief with content, or `null`
    /// before that brief. Each field is `null` when the store is older than
    /// the measure, so the times are not known.
    pub first_brief: Option<FirstBrief>,
    /// The last mirror check of each account. An account with no check
    /// has `checked: null`.
    pub mirror: Vec<Check>,
    pub memory: Memory,
    /// The decisions of the person on the actions of an agent or of triage.
    pub approvals: Approvals,
    /// The applied actions, and the actions of them that an undo reversed.
    pub undo: Undo,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Day {
    /// The local date of the person, `YYYY-MM-DD`.
    pub day: String,
    /// The source mails that the person opened on that day. A mail that
    /// the person opened twice on one day counts once.
    pub opens: i64,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct FirstBrief {
    /// The time of the first account (the connect step).
    pub connect: Option<String>,
    /// The time of the first brief with content.
    pub brief: Option<String>,
    pub seconds: Option<i64>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Check {
    pub account: String,
    /// The time of the last check, or `null` when the account has none.
    pub checked: Option<String>,
    /// The differences that the check found, or `null` when there is no
    /// check or the text of the log row has no count.
    pub differences: Option<i64>,
    /// `false` when there is no check, or when an error stopped the check
    /// before its end.
    pub complete: bool,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Memory {
    /// The resident memory of the engine, or `null` when the system does
    /// not give it.
    pub rss_kib: Option<i64>,
    pub messages: i64,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Approvals {
    pub decided: i64,
    pub approved: i64,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Undo {
    pub applied: i64,
    pub undone: i64,
}

/// The measures now. `rss_kib` is the resident memory of the engine.
pub fn measures(store: &Store, rss_kib: Option<i64>) -> Result<Measures> {
    Ok(store.read(|c| {
        let mut q = c.prepare(
            "SELECT day, (SELECT count(*) FROM use_open o WHERE o.day = d.day)
             FROM use_day d ORDER BY day DESC LIMIT ?1",
        )?;
        let days = q
            .query_map([DAYS], |r| {
                Ok(Day {
                    day: r.get(0)?,
                    opens: r.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        // The brief row is there only after the first brief, or when the
        // store is older than the measure (its time is NULL).
        let first_brief = c
            .query_row(
                "SELECT (SELECT at FROM measure_time WHERE name = 'connect'), at
                 FROM measure_time WHERE name = 'brief'",
                [],
                |r| Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, Option<i64>>(1)?)),
            )
            .optional()?
            .map(|(a, b)| FirstBrief {
                connect: a.map(rfc3339),
                brief: b.map(rfc3339),
                seconds: a.zip(b).map(|(a, b)| brief_seconds(b - a)),
            });
        let mut q = c.prepare(
            "SELECT a.address, coalesce(l.ended_at, l.started_at), l.result
             FROM account a LEFT JOIN sync_log l ON l.id =
               (SELECT max(id) FROM sync_log WHERE account_id = a.id AND kind = 'verify')
             ORDER BY a.id",
        )?;
        let mirror = q
            .query_map([], |r| {
                let checked: Option<i64> = r.get(1)?;
                let (differences, complete) = match checked {
                    Some(_) => verify_result(&r.get::<_, Option<String>>(2)?.unwrap_or_default()),
                    None => (None, false),
                };
                Ok(Check {
                    account: r.get(0)?,
                    checked: checked.map(rfc3339),
                    differences,
                    complete,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        let messages = c.query_row("SELECT count(*) FROM message", [], |r| r.get(0))?;
        let approvals = c.query_row(
            "SELECT count(*), coalesce(sum(approved), 0) FROM person_decision",
            [],
            |r| {
                Ok(Approvals {
                    decided: r.get(0)?,
                    approved: r.get(1)?,
                })
            },
        )?;
        // An undo is an action with its own ledger row, one for each step
        // (migration 23): count only the rows that are not a step of an undo.
        let undo = c.query_row(
            "SELECT count(*), coalesce(sum(state = 'undone'), 0) FROM ledger
             WHERE state IN ('applied', 'undone')
               AND action_id NOT IN (SELECT action_id FROM ledger_undo)",
            [],
            |r| {
                Ok(Undo {
                    applied: r.get(0)?,
                    undone: r.get(1)?,
                })
            },
        )?;
        Ok(Measures {
            days,
            first_brief,
            mirror,
            memory: Memory { rss_kib, messages },
            approvals,
            undo,
        })
    })?)
}

/// Write the time of the first brief with content, if the brief has
/// content at `now_ms` and no time is written. The engine calls it every
/// 10 seconds (the attention thread of `iris serve`) and after `iris
/// mirror` and `iris understand`, so the time does not wait for a person
/// to open the brief.
pub fn check_brief(store: &Store, now_ms: i64) -> Result<()> {
    if known_brief(store)? {
        return Ok(());
    }
    mark_brief(store, &crate::brief(store, now_ms)?, now_ms)
}

fn known_brief(store: &Store) -> Result<bool> {
    Ok(store.read(|c| {
        Ok(c.query_row(
            "SELECT EXISTS (SELECT 1 FROM measure_time WHERE name = 'brief')",
            [],
            |r| r.get::<_, bool>(0),
        )?)
    })?)
}

/// Write the time of the first brief with content, once. `brief` is the
/// answer of GET /api/brief at `now_ms`: content is an item of a section
/// (the brief leaves out expired signals and hidden items), a record or a
/// bundle.
pub fn mark_brief(store: &Store, brief: &crate::Brief, now_ms: i64) -> Result<()> {
    let sections = [
        &brief.you_owe,
        &brief.owed_to_you,
        &brief.conversations,
        &brief.feed,
        &brief.signals,
        &brief.strangers,
    ];
    let content = sections.iter().any(|s| s.count > 0)
        || brief.records.values().any(|n| n.as_i64().unwrap_or(0) > 0)
        || brief.bundles.iter().any(|b| b.section.count > 0);
    if !content {
        return Ok(());
    }
    if !known_brief(store)? {
        store.write(|tx| {
            tx.execute(
                "INSERT INTO measure_time(name, at) SELECT 'brief', ?1
                 WHERE NOT EXISTS (SELECT 1 FROM measure_time WHERE name = 'brief')",
                [now_ms],
            )?;
            Ok(())
        })?;
    }
    Ok(())
}

/// Whole seconds of a time in milliseconds, rounded up: a time just over
/// the target must not show as met.
fn brief_seconds(ms: i64) -> i64 {
    (ms.max(0) + 999) / 1000
}

/// The count and the end of the text that `iris_sync::verify` writes to
/// `sync_log.result`: "N differences, M skipped", then ", then error: ..."
/// when an error stopped the check. The check is complete only with no
/// error and 0 skipped messages: a skipped message was not compared.
fn verify_result(text: &str) -> (Option<i64>, bool) {
    let first = text.lines().next().unwrap_or("");
    let count = first
        .split_once(" differences")
        .and_then(|(n, _)| n.parse().ok());
    let skipped = first
        .split_once(" skipped")
        .and_then(|(n, _)| n.rsplit(' ').next()?.parse::<i64>().ok());
    let none_skipped = skipped == Some(0) || !first.contains(" skipped");
    (count, none_skipped && !first.contains(", then error"))
}

/// Count one use of the web client on `day` (the local date of the person,
/// `YYYY-MM-DD`), and the open of the source mail `open` (an `m-` ID). A
/// second count of the same use or open on the same day changes nothing,
/// so a retry is safe. `now_ms` is the time of the engine. The day must be
/// the date of the engine in UTC, or the day before or after it: the time
/// zone of the person moves the date by at most one day.
pub fn count_use(store: &Store, day: &str, open: Option<&str>, now_ms: i64) -> Result<()> {
    let open = open
        .map(|id| crate::read::row_of(id, iris_store::Kind::Message))
        .transpose()?;
    let digits = day.len() == 10
        && day.bytes().enumerate().all(|(i, b)| {
            if i == 4 || i == 7 {
                b == b'-'
            } else {
                b.is_ascii_digit()
            }
        });
    let near = digits
        && store.read(|c| {
            Ok(c.query_row(
                "SELECT date(?1) IS ?1
                   AND ?1 BETWEEN date(?2 / 1000, 'unixepoch', '-1 day')
                              AND date(?2 / 1000, 'unixepoch', '+1 day')",
                params![day, now_ms],
                |r| r.get::<_, bool>(0),
            )?)
        })?;
    if !near {
        return Err(Error::Query(format!(
            "the day must be a date (YYYY-MM-DD) of today: {day}"
        )));
    }
    store.write(|tx| {
        tx.execute(
            "INSERT INTO use_day(day) VALUES (?1) ON CONFLICT DO NOTHING",
            [day],
        )?;
        if let Some(message) = open {
            tx.execute(
                "INSERT INTO use_open(day, message_id) VALUES (?1, ?2) ON CONFLICT DO NOTHING",
                params![day, message],
            )?;
        }
        Ok(())
    })?;
    Ok(())
}

/// The state of [`count`]: the store, and the last day that a use was
/// counted, so that only the first request of a day writes.
#[derive(Clone)]
pub struct Counter {
    store: Arc<Store>,
    day: Arc<Mutex<Days>>,
}

/// The days of [`Counter`]: the last day that a use was counted, and each
/// day that a request counts now.
#[derive(Default)]
struct Days {
    last: String,
    counting: std::collections::HashSet<String>,
}

impl Counter {
    pub fn new(store: Arc<Store>) -> Self {
        Self {
            store,
            day: Arc::default(),
        }
    }
}

/// Count the measures of each request of the web client that has the
/// header [`DAY`]: a use of that day before the request runs (so a read of
/// the measures includes it), and an open when `GET /api/messages/{id}`
/// (the mail view) answers with success. A day that [`count_use`] refuses
/// counts nothing, and the request runs as usual: the measures never stop
/// the web client.
pub async fn count(State(c): State<Counter>, req: Request, next: Next) -> Response {
    let day = req
        .headers()
        .get(DAY)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let open = (req.method() == Method::GET)
        .then(|| req.uri().path().strip_prefix("/api/messages/"))
        .flatten()
        .filter(|id| !id.contains('/'))
        .map(str::to_owned);
    // A read of the measures always counts its own day, unless the store
    // has that day already: it must not pass a count that another request
    // has not written yet. The count is idempotent.
    let reads_measures = req.uri().path() == "/api/measures";
    if let Some(day) = &day {
        // The lock is held only to read and change the state, never during
        // a store call (#700). One request counts each day that is not
        // counted yet. Another request of that day skips the count while
        // the first one counts it, and does not wait for the store. A
        // request of another day counts its own.
        let (first, mine) = {
            let mut d = c.day.lock().unwrap_or_else(|e| e.into_inner());
            let first = d.last != *day && d.counting.insert(day.clone());
            (first, first || d.last != *day && reads_measures)
        };
        if mine {
            let ok = count_use(&c.store, day, None, crate::now_ms()).is_ok();
            let mut d = c.day.lock().unwrap_or_else(|e| e.into_inner());
            if first {
                d.counting.remove(day);
            }
            if ok {
                d.last = day.clone();
            }
        }
    }
    let res = next.run(req).await;
    if let (Some(day), Some(id)) = (day, open) {
        if res.status().is_success() {
            let _ = count_use(&c.store, &day, Some(&id), crate::now_ms());
        }
    }
    res
}

/// The resident memory of this process in KiB, from `ps`, as the budget
/// test reads it (`crates/iris/tests/budget.rs`). `ps` has the same
/// option on macOS and Linux.
pub fn rss_kib() -> Option<i64> {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_result_reads_the_text_of_the_verify() {
        assert_eq!(verify_result("0 differences, 0 skipped"), (Some(0), true));
        // A skipped message was not compared: the check is not complete.
        assert_eq!(verify_result("0 differences, 2 skipped"), (Some(0), false));
        assert_eq!(
            verify_result("3 differences, 1 skipped\nmessage a: other labels"),
            (Some(3), false)
        );
        assert_eq!(verify_result("0 differences, x skipped"), (Some(0), false));
        assert_eq!(
            verify_result("2 differences, 0 skipped, then error: HTTP 401"),
            (Some(2), false)
        );
        assert_eq!(verify_result(""), (None, true));
        assert_eq!(verify_result("x differences"), (None, true));
    }

    /// Property: the shown seconds are never under the real time, so a
    /// time over the target never shows as met.
    #[test]
    fn brief_seconds_round_up() {
        for ms in [
            0, 1, 999, 1000, 1001, 599_999, 600_000, 600_001, 600_999, 601_000,
        ] {
            let s = brief_seconds(ms);
            assert!(s * 1000 >= ms && s * 1000 < ms + 1000, "{ms} {s}");
        }
        assert_eq!(brief_seconds(600_001), 601);
        assert_eq!(brief_seconds(-5), 0);
    }

    #[test]
    fn rss_is_a_plausible_size() {
        // ps is on each supported system. A test binary uses more than
        // 1 MB and less than 1 GB.
        let kib = rss_kib().expect("ps gives the RSS");
        assert!((1_000..1_000_000).contains(&kib), "{kib}");
    }
}
