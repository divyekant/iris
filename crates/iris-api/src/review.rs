//! The review of the web client (specification 2, section 5.5): the open
//! changesets as JSON documents of section 5.1, and the message summaries
//! of their actions, and the decision of the person on one changeset.

use std::io::Write;

use iris_store::{format_id, parse_id, Kind, Store};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::read::message_row;
use crate::{Error, Result};

/// The most message IDs in one request of [`messages_by_id`].
pub const MAX_IDS: usize = 200;

/// The open changesets and their actions that wait for a person: `proposed`
/// or `in_review` (sections 5.2 and 5.4).
#[derive(Debug, Serialize, PartialEq, Default)]
pub struct Review {
    pub changesets: i64,
    pub actions: i64,
}

/// The changesets that the review lists. A changeset whose new revision
/// is a draft is not open: the store refuses an approval until the submit
/// (section 5.5). The list and the counts both use this condition, so the
/// brief counts only what the review shows.
pub(crate) const OPEN: &str = "c.state = 'open'";

pub(crate) fn review_counts(c: &Connection) -> rusqlite::Result<Review> {
    c.query_row(
        &format!(
            "SELECT count(DISTINCT a.changeset_id), count(*) FROM changeset_action a
             JOIN changeset c ON c.id = a.changeset_id
             WHERE {OPEN} AND a.state IN ('proposed', 'in_review')"
        ),
        [],
        |r| {
            Ok(Review {
                changesets: r.get(0)?,
                actions: r.get(1)?,
            })
        },
    )
}

pub(crate) fn changeset_error(e: iris_changeset::Error) -> Error {
    match e {
        iris_changeset::Error::Store(e) => Error::Store(e),
        other => Error::Store(iris_store::Error::Query(other.to_string())),
    }
}

fn io(e: std::io::Error) -> Error {
    Error::Store(e.into())
}

/// The count of changeset IDs that the list reads in one query.
const PAGE: i64 = 64;

/// `{"changesets": [...]}`: each open changeset, the newest first. Each
/// document streams to `out`, and the IDs come in pages of [`PAGE`], so
/// memory does not grow with the count of open changesets (section 9).
pub fn write_open_changesets(store: &Store, out: &mut impl Write) -> Result<()> {
    write_open_paged(store, out, PAGE)
}

fn write_open_paged(store: &Store, out: &mut impl Write, page: i64) -> Result<()> {
    out.write_all(b"{\"changesets\":[").map_err(io)?;
    let mut first = true;
    // Keyset on the row ID: an ID never changes, so a changeset that a new
    // revision moves in time is not listed twice or left out.
    let mut before = i64::MAX;
    loop {
        let ids: Vec<i64> = store.read(|c| {
            let mut st = c.prepare_cached(&format!(
                "SELECT c.id FROM changeset c WHERE {OPEN} AND c.id < ?1
                 ORDER BY c.id DESC LIMIT ?2"
            ))?;
            let ids = st
                .query_map([before, page], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            Ok(ids)
        })?;
        let Some(&last) = ids.last() else { break };
        for id in ids {
            if !first {
                out.write_all(b",").map_err(io)?;
            }
            first = false;
            // A changeset row is never deleted, so each listed ID has its document.
            iris_changeset::write_document(store, id, out).map_err(changeset_error)?;
        }
        before = last;
    }
    out.write_all(b"]}").map_err(io)
}

/// The decision of the person on one action, as the review showed it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub revision: i64,
    pub n: i64,
    /// `true` approves, `false` denies.
    pub approve: bool,
    /// The hash of the action that the review showed. An approval needs it
    /// (section 5.5).
    pub hash: Option<String>,
}

/// The most decisions in one request. A changeset has few actions.
pub const MAX_DECISIONS: usize = 500;

/// The reason of a denial from the review. The store needs a reason for
/// each denial (section 5.2), and the review asks for no text.
pub const DENIED: &str = "The person denied it in the review.";

/// Approve and deny the actions of one changeset in one transaction
/// (section 5.5). Each action must wait for a person (`proposed` or
/// `in_review`). If one decision is refused, for example a hash that
/// changed after the review, nothing is written, and the answer is
/// [`Error::Conflict`].
pub fn decide(store: &Store, changeset: i64, decisions: &[Decision], now_ms: i64) -> Result<()> {
    if decisions.is_empty() || decisions.len() > MAX_DECISIONS {
        return Err(Error::Query(format!("give 1 to {MAX_DECISIONS} decisions")));
    }
    let mut seen = std::collections::HashSet::new();
    for d in decisions {
        if !seen.insert((d.revision, d.n)) {
            return Err(Error::Query(format!(
                "action {}.{} is named twice",
                d.revision, d.n
            )));
        }
        if d.approve && d.hash.is_none() {
            return Err(Error::Query(
                "an approval needs the hash of the action".into(),
            ));
        }
    }
    use iris_changeset::{ActionId, By, Error as Cs, State};
    iris_changeset::transaction(store, |c| {
        require_open(c, changeset)?;
        for d in decisions {
            let action = ActionId {
                changeset,
                revision: d.revision,
                n: d.n,
            };
            let state: Option<Option<String>> = c
                .query_row(
                    "SELECT state FROM changeset_action
                     WHERE changeset_id = ?1 AND revision = ?2 AND n = ?3",
                    [changeset, d.revision, d.n],
                    |r| r.get(0),
                )
                .optional()?;
            // Only an action that waits for a person: the review shows the others
            // as decided, and the store refuses most moves from them.
            if !matches!(state.flatten().as_deref(), Some("proposed" | "in_review")) {
                return Err(Cs::Refused(format!(
                    "action {}.{} does not wait for a decision now",
                    d.revision, d.n
                )));
            }
            match &d.hash {
                Some(hash) if d.approve => iris_changeset::approve_in(c, action, hash, now_ms)?,
                _ => iris_changeset::decide_in(
                    c,
                    action,
                    State::Denied,
                    By::Person,
                    Some(DENIED),
                    now_ms,
                )?,
            }
        }
        Ok(())
    })
    .map_err(|e| conflict(e, "the action is not in this changeset"))
}

/// The person withdraws one changeset (section 5.5: "each changeset"), in
/// any state but `withdrawn`. Each open action and each draft action
/// becomes `cancelled`; applied actions stay. A second withdraw is
/// [`Error::Conflict`].
pub fn withdraw(store: &Store, changeset: i64) -> Result<()> {
    iris_changeset::withdraw(store, changeset, None)
        .map_err(|e| conflict(e, "the changeset is not in the store"))
}

/// The review shows only an open changeset (the OPEN condition of the list
/// and the counts). While a new revision is a draft, or after a withdraw or
/// the last decision, no decision of the person applies.
fn require_open(c: &Connection, changeset: i64) -> iris_changeset::Result<()> {
    let open: bool = c.query_row(
        &format!("SELECT EXISTS (SELECT 1 FROM changeset c WHERE c.id = ?1 AND {OPEN})"),
        [changeset],
        |r| r.get(0),
    )?;
    if !open {
        return Err(iris_changeset::Error::Refused(
            "the changeset is not open for review now".into(),
        ));
    }
    Ok(())
}

/// A refusal of the changeset store is a 409: the request does not agree
/// with the store now. `missing` names a `NotFound`.
fn conflict(e: iris_changeset::Error, missing: &str) -> Error {
    use iris_changeset::Error as Cs;
    match e {
        Cs::Store(e) => Error::Store(e),
        Cs::NotFound => Error::Conflict(missing.into()),
        Cs::Refused(why) | Cs::Denied(why) => Error::Conflict(why),
    }
}

/// The row ID of the changeset `id` (`cs-77`), or `None` when no changeset
/// has it.
pub fn changeset_row(store: &Store, id: &str) -> Result<Option<i64>> {
    let Some(row) = iris_changeset::parse_changeset_id(id) else {
        return Ok(None);
    };
    Ok(store.read(|c| {
        Ok(
            c.query_row("SELECT id FROM changeset WHERE id = ?1", [row], |r| {
                r.get(0)
            })
            .optional()?,
        )
    })?)
}

/// The summaries of the messages `ids` (`m-1,m-2`), in the order asked.
/// A message that is gone, or that is not in the mirror any more, is
/// `{"id", "gone": true}`, so a review shows each target that it names.
pub fn messages_by_id(store: &Store, ids: &str) -> Result<Vec<Value>> {
    if ids.split(',').count() > MAX_IDS {
        return Err(Error::Query(format!("at most {MAX_IDS} IDs")));
    }
    let rows = ids
        .split(',')
        .map(|id| match parse_id(id) {
            Some((Kind::Message, row)) => Ok(row),
            _ => Err(Error::Query(format!("not a message ID: {id:?}"))),
        })
        .collect::<Result<Vec<i64>>>()?;
    Ok(store.read(|c| {
        let mut out = Vec::with_capacity(rows.len());
        for &row in &rows {
            out.push(match message_row(c, row)? {
                Some((summary, more)) if !more.gone => {
                    let mut v = serde_json::to_value(summary).unwrap_or(Value::Null);
                    v["gone"] = false.into();
                    v
                }
                _ => json!({ "id": format_id(Kind::Message, row), "gone": true }),
            });
        }
        Ok(out)
    })?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Generated input: `parse_after` takes back each `next` value that
    /// `handled` can give, and refuses each other text (signs, zeros in
    /// front, spaces, other characters, 0, a value out of range).
    #[test]
    fn the_cursor_parser_accepts_only_canonical_row_ids() {
        let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = move || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        let canonical = |t: &str| {
            !t.is_empty()
                && t.bytes().all(|b| b.is_ascii_digit())
                && !t.starts_with('0')
                && t.parse::<i64>().is_ok()
        };
        const PARTS: [&str; 14] = [
            "0",
            "1",
            "7",
            "9",
            "42",
            "+",
            "-",
            " ",
            "x",
            "\u{e9}",
            "9223372036854775807",
            "9223372036854775808",
            "00",
            ".",
        ];
        for _ in 0..20_000 {
            // A valid row ID gives its own text, and the text gives it back.
            let id = (next() >> 1) as i64;
            if id > 0 {
                assert_eq!(parse_after(&id.to_string()), Some(id));
            }
            // Random text from parts: the parser agrees with the oracle.
            let t: String = (0..1 + next() % 4)
                .map(|_| PARTS[(next() % PARTS.len() as u64) as usize])
                .collect();
            let want = canonical(&t).then(|| t.parse::<i64>().unwrap());
            assert_eq!(parse_after(&t), want, "{t:?}");
        }
        assert_eq!(parse_after(""), None);
        assert_eq!(parse_after("0"), None);
    }

    /// The list reads the IDs in pages, and each page continues after the
    /// last ID of the page before: no changeset twice, none left out.
    #[test]
    fn the_list_pages_through_each_open_changeset() {
        let dir = std::env::temp_dir().join(format!("iris-api-pages-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Store::open(&dir).unwrap();
        let account = store.add_account("gmail", "owner@example.com").unwrap();
        let message = iris_store::NewMessage {
            provider_id: "g1".into(),
            thread_provider_id: "gt1".into(),
            ..Default::default()
        };
        store.put_messages(account, &[message]).unwrap();
        let author = iris_changeset::Author {
            kind: iris_changeset::AuthorKind::Agent,
            name: Some("Claude Code".into()),
            session: None,
            job: None,
        };
        let none = serde_json::Map::new();
        for i in 1..=7 {
            let cs = iris_changeset::create(&store, &author, i).unwrap();
            let verb = iris_changeset::Verb::Archive;
            iris_changeset::add_action(&store, cs, &author, verb, &["m-1"], &none, None).unwrap();
            // Changeset 4 stays a draft, so it is not open.
            if i != 4 {
                iris_changeset::submit(&store, cs, &author, "Tidy up.", i).unwrap();
            }
        }
        for page in [1, 2, 3, 6, 7, 64] {
            let mut out = Vec::new();
            write_open_paged(&store, &mut out, page).unwrap();
            let v: Value = serde_json::from_slice(&out).unwrap();
            let ids: Vec<&str> = v["changesets"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| c["id"].as_str().unwrap())
                .collect();
            assert_eq!(
                ids,
                ["cs-7", "cs-6", "cs-5", "cs-3", "cs-2", "cs-1"],
                "page {page}"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// One action that a rule decided without the person (specification 2,
/// section 5.4), in its state now.
#[derive(Debug, Serialize, PartialEq)]
pub struct HandledAction {
    pub changeset: String,
    pub revision: i64,
    pub n: i64,
    pub verb: String,
    /// The arguments of the verb, for example the label of `label`.
    pub args: Value,
    /// The count of messages of the action.
    pub messages: i64,
    /// The state after the rule decision: `approved`, `held`, `applied`,
    /// `failed`, `undone` or `denied`.
    pub state: String,
    /// The rule and the confirmed policy version that approved it.
    pub rule: i64,
    pub policy: i64,
    /// The reason of the author, and the reason of the decision.
    pub why: Option<String>,
    pub reason: Option<String>,
    /// The time of the decision.
    pub at: Option<String>,
}

/// One page of [`handled`].
#[derive(Debug, Serialize, PartialEq)]
pub struct HandledPage {
    /// The count of all such actions, not only of this page.
    pub count: i64,
    pub actions: Vec<HandledAction>,
    /// The `after` value of the next page, or `None` on the last page.
    pub next: Option<String>,
}

/// The row ID of an `after` value of [`handled`]: only the canonical text
/// of a positive row ID, as `next` gives it, so one ID has one cursor.
pub(crate) fn parse_after(t: &str) -> Option<i64> {
    t.parse::<i64>()
        .ok()
        .filter(|n| *n > 0 && n.to_string() == t)
}

/// The actions that a rule decided without the person, newest first. `after` is the
/// `next` value of the page before. Keyset on the row ID: a row ID never
/// changes, so a page does not repeat or skip an action.
pub fn handled(store: &Store, limit: u32, after: Option<&str>) -> Result<HandledPage> {
    if !(1..=crate::MAX_LIMIT).contains(&limit) {
        return Err(Error::Query(format!(
            "limit must be 1 to {}",
            crate::MAX_LIMIT
        )));
    }
    let before = match after {
        None => i64::MAX,
        Some(t) => parse_after(t)
            .ok_or_else(|| Error::Query(format!("after {t} is not the next value of a page")))?,
    };
    let (count, mut rows): (i64, Vec<(i64, HandledAction)>) = store.read(|c| {
        let count = c.query_row(
            &format!(
                "SELECT count(*) FROM changeset_action a WHERE {}",
                crate::read::BY_RULE
            ),
            [],
            |r| r.get(0),
        )?;
        let mut st = c.prepare_cached(&format!(
            "SELECT a.id, a.changeset_id, a.revision, a.n, a.verb,
                    (SELECT count(*) FROM action_message m WHERE m.action_id = a.id),
                    a.state, a.rule, a.policy, a.why, a.reason, a.decided_at, a.args
             FROM changeset_action a
             WHERE {} AND a.id < ?1 ORDER BY a.id DESC LIMIT ?2",
            crate::read::BY_RULE
        ))?;
        let rows = st
            .query_map(params![before, limit + 1], |r| {
                Ok((
                    r.get(0)?,
                    HandledAction {
                        changeset: iris_changeset::format_changeset_id(r.get(1)?),
                        revision: r.get(2)?,
                        n: r.get(3)?,
                        verb: r.get(4)?,
                        messages: r.get(5)?,
                        state: r.get(6)?,
                        rule: r.get(7)?,
                        policy: r.get(8)?,
                        why: r.get(9)?,
                        reason: r.get(10)?,
                        at: r.get::<_, Option<i64>>(11)?.map(crate::rfc3339),
                        // A CHECK of the store keeps `args` valid JSON.
                        args: serde_json::from_str(&r.get::<_, String>(12)?).unwrap_or(Value::Null),
                    },
                ))
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok((count, rows))
    })?;
    let more = rows.len() > limit as usize;
    rows.truncate(limit as usize);
    let next = more
        .then(|| rows.last().map(|(id, _)| id.to_string()))
        .flatten();
    Ok(HandledPage {
        count,
        actions: rows.into_iter().map(|(_, a)| a).collect(),
        next,
    })
}

/// A review comment of the person (docs/specs/2026-11-17-review-comments.md,
/// section 3). A text or a key that breaks the rules is a 400, a refusal of
/// the state is a 409. Nothing is written in either case.
pub fn comment(
    store: &Store,
    changeset: i64,
    revision: i64,
    text: &str,
    key: &str,
    now_ms: i64,
) -> Result<Value> {
    if let Some(why) = iris_changeset::check_comment(text, key) {
        return Err(Error::Query(why));
    }
    let c = iris_changeset::transaction(store, |c| {
        iris_changeset::comment_in(c, changeset, revision, text, key, now_ms)
    })
    .map_err(|e| conflict(e, "the changeset is not in the store"))?;
    Ok(comment_doc(&c))
}

/// Each review comment of `changeset`, oldest first.
pub fn comments(store: &Store, changeset: i64) -> Result<Value> {
    let mut out = None;
    store.read(|c| {
        out = Some(iris_changeset::comments_in(c, changeset));
        Ok(())
    })?;
    let all = out
        .expect("the read ran")
        .map_err(|e| conflict(e, "the changeset is not in the store"))?;
    Ok(serde_json::json!({ "comments": all.iter().map(comment_doc).collect::<Vec<_>>() }))
}

fn comment_doc(c: &iris_changeset::Comment) -> Value {
    serde_json::json!({
        "id": c.id,
        "changeset": iris_changeset::format_changeset_id(c.changeset),
        "revision": c.revision,
        "text": c.text,
        "at": crate::rfc3339(c.at_ms),
    })
}
