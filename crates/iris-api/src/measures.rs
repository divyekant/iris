//! The measures of the manifesto, "How we measure Iris" (card R5-12). The
//! engine computes each measure from the store on this machine. Nothing
//! leaves the machine.

use iris_store::Store;
use rusqlite::params;
use serde::Serialize;

use crate::{rfc3339, Error, Result};

/// The number of the newest days of use that [`measures`] gives.
pub const DAYS: u32 = 28;

#[derive(Debug, Serialize, PartialEq)]
pub struct Measures {
    /// The newest days of use, the newest first. A day with no row is a
    /// day on which the person did not use the web client.
    pub days: Vec<Day>,
    /// From the first account to the first brief with content, or `null`
    /// before that brief.
    pub first_brief: Option<FirstBrief>,
    /// The last mirror check of each account that has one.
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
    /// The source mail that the person opened on that day.
    pub opens: i64,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct FirstBrief {
    /// The time of the first account (the connect step).
    pub connect: String,
    /// The time of the first answer of GET /api/brief with content.
    pub brief: String,
    pub seconds: i64,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Check {
    pub account: String,
    pub checked: String,
    /// The differences that the check found, or `null` when the text of
    /// the log row has no count.
    pub differences: Option<i64>,
    /// `false` when an error stopped the check before its end.
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
        let mut q = c.prepare("SELECT day, opens FROM use_day ORDER BY day DESC LIMIT ?1")?;
        let days = q
            .query_map([DAYS], |r| {
                Ok(Day {
                    day: r.get(0)?,
                    opens: r.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        let first_brief = c
            .query_row(
                "SELECT (SELECT at FROM measure_time WHERE name = 'connect'),
                        (SELECT at FROM measure_time WHERE name = 'brief')",
                [],
                |r| Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, Option<i64>>(1)?)),
            )
            .map(|times| match times {
                (Some(a), Some(b)) => Some(FirstBrief {
                    connect: rfc3339(a),
                    brief: rfc3339(b),
                    seconds: (b - a).max(0) / 1000,
                }),
                _ => None,
            })?;
        let mut q = c.prepare(
            "SELECT a.address, coalesce(l.ended_at, l.started_at), coalesce(l.result, '')
             FROM account a JOIN sync_log l ON l.id =
               (SELECT max(id) FROM sync_log WHERE account_id = a.id AND kind = 'verify')
             ORDER BY a.id",
        )?;
        let mirror = q
            .query_map([], |r| {
                let result: String = r.get(2)?;
                let (differences, complete) = verify_result(&result);
                Ok(Check {
                    account: r.get(0)?,
                    checked: rfc3339(r.get(1)?),
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
        // An undo is an action with its own ledger row: count only the
        // rows that are not the undo of another row.
        let undo = c.query_row(
            "SELECT count(*), coalesce(sum(state = 'undone'), 0) FROM ledger
             WHERE state IN ('applied', 'undone')
               AND id NOT IN (SELECT undone_by FROM ledger WHERE undone_by IS NOT NULL)",
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

/// The count and the end of the text that `iris_sync::verify` writes to
/// `sync_log.result`: "N differences, M skipped", then ", then error: ..."
/// when an error stopped the check.
fn verify_result(text: &str) -> (Option<i64>, bool) {
    let first = text.lines().next().unwrap_or("");
    let count = first
        .split_once(" differences")
        .and_then(|(n, _)| n.parse().ok());
    (count, !first.contains(", then error"))
}

/// Count one use of the web client on `day` (the local date of the person,
/// `YYYY-MM-DD`), and one source mail opened when `open`. `now_ms` is the
/// time of the engine. The day must be the date of the engine in UTC, or
/// the day before or after it: the time zone of the person moves the date
/// by at most one day.
pub fn count_use(store: &Store, day: &str, open: bool, now_ms: i64) -> Result<()> {
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
            "INSERT INTO use_day(day, opens) VALUES (?1, ?2)
             ON CONFLICT(day) DO UPDATE SET opens = opens + excluded.opens",
            params![day, i64::from(open)],
        )?;
        Ok(())
    })?;
    Ok(())
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
        assert_eq!(
            verify_result("3 differences, 1 skipped\nmessage a: other labels"),
            (Some(3), true)
        );
        assert_eq!(
            verify_result("2 differences, 0 skipped, then error: HTTP 401"),
            (Some(2), false)
        );
        assert_eq!(verify_result(""), (None, true));
        assert_eq!(verify_result("x differences"), (None, true));
    }

    #[test]
    fn rss_is_a_plausible_size() {
        // ps is on each supported system. A test binary uses more than
        // 1 MB and less than 1 GB.
        let kib = rss_kib().expect("ps gives the RSS");
        assert!((1_000..1_000_000).contains(&kib), "{kib}");
    }
}
