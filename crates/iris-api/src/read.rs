//! The read functions of the local API. The HTTP handlers and the read verbs
//! of the `iris` program call the same functions, so both give the same data.

use std::path::PathBuf;

use iris_store::{format_id, parse_id, Kind, Store};
use rusqlite::{params, types::Value as SqlValue, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value;

use crate::{Error, Result};

/// The largest page of a list, and the largest list of a brief section.
pub const MAX_LIMIT: u32 = 200;

const FORMS: [(&str, Kind); 5] = [
    ("commitment", Kind::Commitment),
    ("conversation", Kind::Conversation),
    ("record", Kind::Record),
    ("feed", Kind::Feed),
    ("signal", Kind::Signal),
];

/// The status values of each form (the schemas of `docs/spec/schema/`).
const STATUSES: [(&str, &[&str]); 5] = [
    (
        "commitment",
        &["open", "done", "declined", "deferred", "cancelled", "gone"],
    ),
    ("conversation", &["active", "quiet", "muted", "gone"]),
    ("record", &["kept", "corrected", "gone"]),
    ("feed", &["new", "read", "skipped", "gone"]),
    (
        "signal",
        &["active", "used", "dismissed", "expired", "gone"],
    ),
];

const RECORD_TYPES: [&str; 7] = [
    "receipt",
    "booking",
    "statement",
    "ticket",
    "contract",
    "event",
    "other",
];

const DAY_MS: i64 = 86_400_000;

/// A limit must be 1 to [`MAX_LIMIT`].
pub(crate) fn check_limit(limit: u32) -> Result<()> {
    if (1..=MAX_LIMIT).contains(&limit) {
        Ok(())
    } else {
        Err(Error::Query(format!("limit must be 1 to {MAX_LIMIT}")))
    }
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Status {
    /// The mode of the active policy (specification 2, section 6).
    pub mode: &'static str,
    pub accounts: Vec<Account>,
    pub messages: Counts,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Account {
    pub id: i64,
    pub address: String,
    pub provider: String,
    pub state: Option<String>,
    /// When the first sync ended. `None` while the mirror is not ready.
    pub mirror_ready_at: Option<String>,
    /// The unfinished first sync or full resync, if there is one.
    pub sync: Option<SyncState>,
    pub last_sync: Option<SyncRun>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct SyncState {
    pub kind: String,
    pub step: Option<i64>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct SyncRun {
    pub kind: String,
    pub started: String,
    pub ended: Option<String>,
    pub result: Option<String>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Counts {
    pub total: i64,
    pub fetched: i64,
    pub understood: i64,
    pub gone: i64,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Address {
    pub name: Option<String>,
    pub address: Option<String>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct MessageSummary {
    pub id: String,
    pub thread: String,
    pub date: Option<String>,
    pub from: Address,
    pub subject: Option<String>,
    pub labels: Vec<String>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Message {
    #[serde(flatten)]
    pub summary: MessageSummary,
    pub to: Option<String>,
    pub cc: Option<String>,
    /// The clean text (specification 1, section 7.2). `None` before the
    /// understanding step cleaned the message. The text is data: show it as
    /// text, never as markup.
    pub text: Option<String>,
    pub gone: bool,
    /// The hostile marks of the message (specification 1, section 6.7).
    pub hostile: Vec<String>,
    /// The kind of the sender row of the `From` address: `person`,
    /// `service`, `stranger` or `hostile` (specification 1, section 6.7). A
    /// sender with no row or no kind yet is a `stranger`. A hostile sender can
    /// send a message with no hostile mark.
    pub sender_kind: String,
    /// The items that are not gone and that have this message as a source.
    pub items: Vec<String>,
}

/// One section of the brief: the full count, and at most [`MAX_LIMIT`] items.
#[derive(Debug, Serialize, PartialEq, Default)]
pub struct Section {
    pub count: i64,
    pub items: Vec<Value>,
    /// Possible duplicates (specification 1, section 7.5): for an item of
    /// `items`, the IDs of the newer items with `duplicate_of` set to it. The
    /// brief shows the pair once, so the newer items are not in `items`.
    #[serde(skip_serializing_if = "serde_json::Map::is_empty")]
    pub duplicates: serde_json::Map<String, Value>,
}

/// A count of messages, in total and for each verb of their actions.
#[derive(Debug, Serialize, PartialEq, Default)]
pub struct Proposal {
    pub count: i64,
    pub verbs: serde_json::Map<String, Value>,
}

/// The work that Iris did without the person: the items that a `hide` rule
/// leaves out of the brief, and the messages of the actions that a rule
/// decided ([`BY_RULE`], specification 2, sections 5.4 and 6.4).
#[derive(Debug, Serialize, PartialEq, Default)]
pub struct Handled {
    /// `hidden` and the messages of `verbs` together.
    pub count: i64,
    /// The count of messages of each verb.
    pub verbs: serde_json::Map<String, Value>,
}

/// The message counts of each verb of the actions that `filter` selects
/// (columns of `changeset_action a` and `changeset c`).
fn verb_counts(c: &Connection, filter: &str) -> rusqlite::Result<Proposal> {
    let mut out = Proposal::default();
    let mut st = c.prepare_cached(&format!(
        "SELECT a.verb, count(*) FROM changeset_action a
         JOIN changeset c ON c.id = a.changeset_id
         JOIN action_message m ON m.action_id = a.id
         WHERE {filter} GROUP BY 1 ORDER BY 1"
    ))?;
    for row in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
        let (verb, n) = row?;
        out.count += n;
        out.verbs.insert(verb, n.into());
    }
    Ok(out)
}

/// The triage actions that wait for a person: `proposed` or `in_review` in
/// the open triage changesets.
const TRIAGE_OPEN: &str =
    "c.author_kind = 'triage' AND c.state = 'open' AND a.state IN ('proposed', 'in_review')";

/// The actions that a rule decided without the person, in each later state:
/// `approved`, `held`, `applied`, `failed` (some messages can have changed),
/// `undone` and `denied`. The apply keeps the decision of the rule on
/// `applied` and `failed` (schema 7, trigger `action_state`). An action in
/// `in_review` waits for the person, so the review counts it, not this.
/// An approved action that expires or is cancelled gets the decision of
/// `expiry` or `author`, and changed nothing, so it leaves this set.
pub(crate) const BY_RULE: &str = "a.decided_by = 'rule'
    AND a.state IN ('approved', 'held', 'applied', 'failed', 'undone', 'denied')";

/// The first brief (specification 1, section 8).
#[derive(Debug, Serialize, PartialEq, Default)]
pub struct Brief {
    pub you_owe: Section,
    pub owed_to_you: Section,
    pub conversations: Section,
    /// The count of each record type.
    pub records: serde_json::Map<String, Value>,
    pub feed: Section,
    pub signals: Section,
    pub strangers: Section,
    /// The senders that wait at the stranger gate, newest first, with their
    /// items of `strangers` (specification 1, section 6.7).
    pub gate: crate::gate::GatePage,
    /// The count of declined senders with items that would wait at the gate.
    pub declined: i64,
    /// The count of hostile mail of the last 7 days.
    pub hostile: i64,
    /// The count of messages that the model failed 3 times.
    pub not_understood: i64,
    /// The date of the oldest fetched message that is not gone: the brief
    /// covers the mail from this date (specification 1, section 4.2).
    pub oldest_fetched: Option<String>,
    /// The triage proposal: the messages of the actions of the stored
    /// triage changesets that wait for a person (specification 2, section 5.4).
    pub proposal: Option<Proposal>,
    /// What Iris did without the person.
    pub handled: Handled,
    /// The items of each `bundle` rule (specification 2, section 6.4), by
    /// rule ID. A bundled item is in no other section and in no count.
    pub bundles: Vec<Bundle>,
    /// The count of items that a `hide` rule leaves out of the brief.
    pub hidden: i64,
    /// The open changesets and their actions that wait for a person
    /// (specification 2, section 5.5).
    pub review: crate::review::Review,
}

/// The items of one `bundle` rule: the count, and the newest
/// [`BUNDLE_ITEMS`] items.
#[derive(Debug, Serialize, PartialEq)]
pub struct Bundle {
    pub rule: u32,
    /// The name of the rule in the active policy.
    pub name: String,
    #[serde(flatten)]
    pub section: Section,
}

/// The most items of each bundle in the brief: enough for the client to
/// name who sent them. `/api/brief/bundle-<rule>` gives all.
pub const BUNDLE_ITEMS: u32 = 20;

#[derive(Debug, Serialize, PartialEq)]
pub struct Page {
    pub items: Vec<Value>,
    /// The `before` value of the next page, or `None` on the last page.
    pub next: Option<String>,
}

/// A filter of [`items`]. Each field that is `None` matches each item.
#[derive(Debug, Default, Clone)]
pub struct Filter<'a> {
    pub form: Option<&'a str>,
    pub status: Option<&'a str>,
    /// The record type. Only for the form `record`.
    pub kind: Option<&'a str>,
    /// Give only items older than this item ID.
    pub before: Option<&'a str>,
}

pub fn status(store: &Store) -> Result<Status> {
    Ok(store.read(|c| {
        let mut st = c.prepare(
            "SELECT id, address, provider, state, mirror_ready_at, sync_kind, sync_step
             FROM account ORDER BY id",
        )?;
        let rows = st.query_map([], |r| {
            let kind: Option<String> = r.get(5)?;
            let step: Option<i64> = r.get(6)?;
            Ok(Account {
                id: r.get(0)?,
                address: r.get(1)?,
                provider: r.get(2)?,
                state: r.get(3)?,
                mirror_ready_at: r.get::<_, Option<i64>>(4)?.map(rfc3339),
                sync: kind.map(|kind| SyncState { kind, step }),
                last_sync: None,
            })
        })?;
        let mut accounts = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        for a in &mut accounts {
            a.last_sync = c
                .query_row(
                    "SELECT kind, started_at, ended_at, result FROM sync_log
                     WHERE account_id = ?1 ORDER BY id DESC LIMIT 1",
                    [a.id],
                    |r| {
                        Ok(SyncRun {
                            kind: r.get(0)?,
                            started: rfc3339(r.get(1)?),
                            ended: r.get::<_, Option<i64>>(2)?.map(rfc3339),
                            result: r.get(3)?,
                        })
                    },
                )
                .optional()?;
        }
        let messages = c.query_row(
            "SELECT count(*), count(fetched_at), count(understood_at), count(gone_at) FROM message",
            [],
            |r| {
                Ok(Counts {
                    total: r.get(0)?,
                    fetched: r.get(1)?,
                    understood: r.get(2)?,
                    gone: r.get(3)?,
                })
            },
        )?;
        Ok(Status {
            mode: iris_policy::active(c)?.policy.mode.name(),
            accounts,
            messages,
        })
    })?)
}

/// Search the messages that are not gone, newest first (specification 1,
/// section 5.6). A search text that is not valid gives [`Error::Query`].
pub fn search(store: &Store, text: &str, limit: u32) -> Result<Vec<MessageSummary>> {
    check_limit(limit)?;
    let ids = store.search(text, limit)?;
    Ok(store.read(|c| Ok(summaries(c, &ids)?))?)
}

/// The summaries of the messages `ids`, in order. A message that the sync
/// marked gone after the search is left out.
pub(crate) fn summaries(c: &Connection, ids: &[i64]) -> rusqlite::Result<Vec<MessageSummary>> {
    let mut out = Vec::with_capacity(ids.len());
    for &id in ids {
        if let Some((summary, more)) = message_row(c, id)? {
            if !more.gone {
                out.push(summary);
            }
        }
    }
    Ok(out)
}

/// One message by its `m-` ID, also when it is gone.
pub fn message(store: &Store, id: &str) -> Result<Option<Message>> {
    let row = row_of(id, Kind::Message)?;
    Ok(store.read(|c| Ok(message_in(c, row)?))?)
}

/// One message by its row ID, in the transaction of `c`.
pub(crate) fn message_in(c: &Connection, row: i64) -> rusqlite::Result<Option<Message>> {
    let Some((summary, mut more)) = message_row(c, row)? else {
        return Ok(None);
    };
    more.sender_kind = sender_kind(c, summary.from.address.as_deref())?;
    let mut st = c.prepare_cached(
        "SELECT i.id, i.form FROM item_source s JOIN item i ON i.id = s.item_id
         WHERE s.message_id = ?1 AND i.status != 'gone' ORDER BY i.id",
    )?;
    let items = st
        .query_map([row], |r| Ok(item_id(r.get(0)?, &r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(Some(Message {
        summary,
        items: items.into_iter().flatten().collect(),
        ..more
    }))
}

/// The path of the raw source (RFC 5322) of one message, or `None` if the
/// message or its content file is not there.
pub fn raw(store: &Store, id: &str) -> Result<Option<PathBuf>> {
    let row = row_of(id, Kind::Message)?;
    let sha: Option<String> = store.read(|c| {
        Ok(c.query_row(
            "SELECT content_sha256 FROM message WHERE id = ?1",
            [row],
            |r| r.get(0),
        )
        .optional()?
        .flatten())
    })?;
    Ok(sha
        .and_then(|s| store.content_path(&s))
        .filter(|p| p.is_file()))
}

/// One item by its ID (`c-`, `v-`, `r-`, `f-` or `s-`), also when it is gone.
pub fn item(store: &Store, id: &str) -> Result<Option<Value>> {
    let (kind, row) = parse_id(id).ok_or_else(|| bad_id(id))?;
    let form = FORMS
        .iter()
        .find(|(_, k)| *k == kind)
        .map(|(f, _)| *f)
        .ok_or_else(|| bad_id(id))?;
    Ok(store.read(|c| {
        Ok(c.query_row(
            "SELECT id, form, status, doc FROM item WHERE id = ?1 AND form = ?2",
            params![row, form],
            doc_of,
        )
        .optional()?)
    })?)
}

/// The items that match `filter`, newest first by the date of the origin
/// message. Items with the status
/// `gone` are left out, unless the filter asks for that status.
pub fn items(store: &Store, filter: &Filter, limit: u32) -> Result<Page> {
    if let Some(form) = filter.form {
        if !FORMS.iter().any(|(f, _)| *f == form) {
            return Err(Error::Query(format!("unknown form {form}")));
        }
    }
    if let Some(status) = filter.status {
        let known = STATUSES
            .iter()
            .filter(|(f, _)| filter.form.is_none_or(|form| form == *f))
            .any(|(_, list)| list.contains(&status));
        if !known {
            return Err(Error::Query(format!("unknown status {status}")));
        }
    }
    if let Some(kind) = filter.kind {
        if filter.form != Some("record") {
            return Err(Error::Query("type needs form=record".into()));
        }
        if !RECORD_TYPES.contains(&kind) {
            return Err(Error::Query(format!("unknown record type {kind}")));
        }
    }
    check_limit(limit)?;
    let before = match filter.before {
        // The cursor is an item ID of the form of the filter.
        Some(id) => match parse_id(id) {
            Some((kind, row))
                if FORMS
                    .iter()
                    .any(|(f, k)| *k == kind && filter.form.is_none_or(|form| form == *f)) =>
            {
                Some(row)
            }
            _ => return Err(bad_id(id)),
        },
        None => None,
    };
    // Newest by the date of the origin message: the first sync fetches newest
    // first, so the row order is not the mail order.
    let date = |t: &str| {
        format!(
            "coalesce((SELECT m.internal_date FROM item_source s JOIN message m ON m.id = s.message_id
               WHERE s.item_id = {t}.id AND s.role = 'origin'), {t}.created_at)"
        )
    };
    let (outer, inner) = (date("item"), date("i"));
    let items = store.read(|c| {
        let mut st = c.prepare(&format!(
            "SELECT id, form, status, doc FROM item
             WHERE (?1 IS NULL OR form = ?1)
               AND (status = ?2 OR (?2 IS NULL AND status != 'gone'))
               AND (?3 IS NULL OR json_extract(doc, '$.type') = ?3)
               AND (?4 IS NULL OR ({outer}, id) <
                    (SELECT {inner}, i.id FROM item i WHERE i.id = ?4))
             ORDER BY {outer} DESC, id DESC LIMIT ?5"
        ))?;
        let rows = st.query_map(
            params![filter.form, filter.status, filter.kind, before, limit + 1],
            doc_of,
        )?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    })?;
    let mut items = items;
    let next = if items.len() > limit as usize {
        items.truncate(limit as usize);
        items
            .last()
            .and_then(|d| d["id"].as_str().map(str::to_string))
    } else {
        None
    };
    Ok(Page { items, next })
}

/// The kind of the sender row of the `p-` ID at the JSON path `path` of the
/// item document `doc`. A sender with no row or no kind yet is a stranger.
/// A stranger that the person declined at the gate is `declined`.
pub(crate) fn kind_sql(doc: &str, path: &str) -> String {
    format!(
        "coalesce((SELECT CASE WHEN coalesce(kind, 'stranger') = 'stranger'
                                AND declined_at IS NOT NULL THEN 'declined' ELSE kind END
                   FROM sender WHERE id =
           CAST(substr(json_extract({doc}, '{path}'), 3) AS INTEGER)), 'stranger')"
    )
}

/// The first brief at the time `now` (milliseconds since the Unix epoch).
/// The names of the list sections of the brief, as in [`Brief`].
pub const SECTIONS: [&str; 6] = [
    "you_owe",
    "owed_to_you",
    "conversations",
    "feed",
    "signals",
    "strangers",
];

/// The condition of an item that the brief shows in its section: no `hide`
/// or `bundle` rule matches it (specification 2, section 6.4). An item with
/// no `attention` field yet counts as shown.
pub(crate) const SHOWN: &str =
    "coalesce(json_extract(doc, '$.attention.state'), 'shown') = 'shown'";

/// The filter and the order of each list section of the brief, in the
/// order of [`SECTIONS`]. Each filter leaves out hidden and bundled items.
pub(crate) fn section_sql(now: i64) -> [(String, String); 6] {
    base_sql(now).map(|(filter, order)| (format!("{} AND {SHOWN}", head(&filter)), order))
}

/// The filter of each record that the brief counts: not gone.
const RECORD: &str = "form = 'record' AND status != 'gone'";

/// The filter of the head of each pair in the place `filter`: an item of the
/// place that is not a possible duplicate of an item of the same place
/// (specification 1, section 7.5). The attention of the head decides where
/// the pair goes, so a pair counts once in the brief, whatever the attention
/// of the duplicate. The inner `item` of the subquery is the original.
pub(crate) fn head(filter: &str) -> String {
    format!(
        "(({filter}) AND NOT (duplicate_of IS NOT NULL
             AND duplicate_of IN (SELECT id FROM item WHERE {filter})))"
    )
}

/// The places of the brief before the attention: the list sections, the
/// records and the items of declined senders. No item is in two places.
fn places(now: i64) -> Vec<String> {
    base_sql(now)
        .into_iter()
        .map(|(filter, _)| filter)
        .chain([RECORD.to_string(), declined_sql()])
        .collect()
}

/// The filter of the items that would wait at the stranger gate, but whose
/// strangers the person declined: an open commitment whose counterparty
/// (you owe) or owner (owed to you) is declined, or an active conversation
/// with a declined participant and no stranger that waits. These items are
/// in no section of the brief (specification 1, section 6.7).
pub(crate) fn declined_sql() -> String {
    let counter = kind_sql("doc", "$.counterparty.sender");
    let owner = kind_sql("doc", "$.owner.sender");
    format!(
        "((form = 'commitment' AND status = 'open' AND (
             (json_extract(doc, '$.direction') = 'you_owe' AND {counter} = 'declined')
          OR (json_extract(doc, '$.direction') = 'owed_to_you' AND {owner} = 'declined')))
          OR (form = 'conversation' AND status = 'active'
              AND {} AND NOT {}))",
        participant_sql("= 'declined'"),
        participant_sql("= 'stranger'")
    )
}

/// True when the conversation document `doc` has a participant sender whose
/// kind (as [`kind_sql`] gives it) matches `test`.
fn participant_sql(test: &str) -> String {
    format!(
        "EXISTS (SELECT 1 FROM json_each(doc, '$.participants') p
           WHERE json_extract(p.value, '$.kind') = 'sender'
             AND {} {test})",
        kind_sql("p.value", "$.sender")
    )
}

/// The filter of the head of each pair of the brief, in any place.
fn heads(now: i64) -> String {
    places(now)
        .iter()
        .map(|f| head(f.as_str()))
        .collect::<Vec<_>>()
        .join(" OR ")
}

/// The filter of each possible duplicate that goes with its original: an
/// item of the brief that is not a head.
fn tails(now: i64) -> String {
    let all = places(now)
        .iter()
        .map(|f| format!("({f})"))
        .collect::<Vec<_>>()
        .join(" OR ");
    format!("({all}) AND NOT ({})", heads(now))
}

/// The filter and the order of the bundle of rule `rule`: newest first.
fn bundle_sql(now: i64, rule: u32) -> (String, String) {
    (
        format!(
            "({}) AND json_extract(doc, '$.attention.state') = 'bundled'
             AND json_extract(doc, '$.attention.rule') = {rule}",
            heads(now)
        ),
        "created_at DESC, id DESC".to_string(),
    )
}

/// The section name of the items that a `hide` rule leaves out of the brief.
pub const HIDDEN: &str = "hidden";

/// The filter and the order of the hidden items: newest first. Each item
/// has the rule and the reason in its `attention` field.
fn hidden_sql(now: i64) -> (String, String) {
    (
        format!(
            "({}) AND json_extract(doc, '$.attention.state') = 'hidden'",
            heads(now)
        ),
        "created_at DESC, id DESC".to_string(),
    )
}

/// The rule of a bundle section name: `bundle-3` is rule 3.
fn bundle_rule(name: &str) -> Option<u32> {
    let n = name.strip_prefix("bundle-")?;
    // Only the canonical form, so one bundle has one cursor name.
    n.parse().ok().filter(|r: &u32| r.to_string() == n)
}

/// The filter and the order of each list section of the brief, with no
/// attention condition.
fn base_sql(now: i64) -> [(String, String); 6] {
    let counter = kind_sql("doc", "$.counterparty.sender");
    let owner = kind_sql("doc", "$.owner.sender");
    let known = "IN ('person', 'service')";
    let stranger_participant = participant_sql("= 'stranger'");
    let commitment = "form = 'commitment' AND status = 'open'";
    // Times are compared as instants: RFC 3339 text with an offset does
    // not sort in time order. A date with no time is midnight UTC.
    let due = "unixepoch(coalesce(json_extract(doc, '$.due.at'), json_extract(doc, '$.due.date')), 'subsec')";
    let you_owe = (
        format!(
            "{commitment} AND json_extract(doc, '$.direction') = 'you_owe' AND {counter} {known}"
        ),
        format!("{due} IS NULL, {due}, created_at, id"),
    );
    let owed = (
        format!(
            "{commitment} AND json_extract(doc, '$.direction') = 'owed_to_you' AND {owner} {known}"
        ),
        "created_at, id".to_string(),
    );
    let conversations = (
        // A conversation with a stranger is only in `strangers` (dk, 2026-11-16),
        // and a conversation with a declined stranger is in no section.
        format!(
            "form = 'conversation' AND status = 'active'
             AND (json_extract(doc, '$.new') > 0 OR json_extract(doc, '$.your_turn') = 1)
             AND NOT {}",
            participant_sql("IN ('stranger', 'declined')")
        ),
        "unixepoch(json_extract(doc, '$.latest.at'), 'subsec') DESC, id DESC".to_string(),
    );
    // Group by the address of the sender row of the publisher (the address of
    // the document when there is no row), then by the sender ID.
    let feed = (
        "form = 'feed' AND status = 'new'".to_string(),
        "coalesce((SELECT address FROM sender WHERE id =
            CAST(substr(json_extract(doc, '$.publisher.sender'), 3) AS INTEGER)),
            json_extract(doc, '$.publisher.address')),
         json_extract(doc, '$.publisher.sender'),
         unixepoch(json_extract(doc, '$.published'), 'subsec') DESC, id DESC"
            .to_string(),
    );
    let signals = (
        format!(
            "form = 'signal' AND status = 'active'
             AND unixepoch(json_extract(doc, '$.expires'), 'subsec') * 1000 > {now}"
        ),
        "unixepoch(json_extract(doc, '$.expires'), 'subsec'), id".to_string(),
    );
    let strangers = (
        format!(
            "(({commitment} AND (
                 (json_extract(doc, '$.direction') = 'you_owe' AND {counter} = 'stranger')
              OR (json_extract(doc, '$.direction') = 'owed_to_you' AND {owner} = 'stranger')))
              OR (form = 'conversation' AND status = 'active' AND {stranger_participant}))"
        ),
        "created_at DESC, id DESC".to_string(),
    );
    [you_owe, owed, conversations, feed, signals, strangers]
}

/// The `next` value of a page of section `name`: the name and the sort keys
/// of the last item of the page, as hexadecimal text of a JSON array.
pub(crate) fn encode_cursor(name: &str, keys: &[SqlValue]) -> String {
    let list: Vec<Value> = std::iter::once(Value::from(name))
        .chain(keys.iter().map(|k| match k {
            SqlValue::Integer(n) => (*n).into(),
            SqlValue::Real(x) => (*x).into(),
            SqlValue::Text(t) => t.clone().into(),
            SqlValue::Null | SqlValue::Blob(_) => Value::Null,
        }))
        .collect();
    Value::from(list)
        .to_string()
        .bytes()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The sort keys in an `after` value. A value that is empty, or that is not
/// a cursor of section `name` with `count` keys, gives [`Error::Query`].
pub(crate) fn decode_cursor(token: &str, name: &str, count: usize) -> Result<Vec<SqlValue>> {
    let bad = || Error::Query(format!("after {token} is not the next value of a page"));
    if token.is_empty() || !token.len().is_multiple_of(2) || !token.is_ascii() {
        return Err(bad());
    }
    let bytes = (0..token.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&token[i..i + 2], 16))
        .collect::<std::result::Result<Vec<u8>, _>>()
        .map_err(|_| bad())?;
    let list: Vec<Value> = serde_json::from_slice(&bytes).map_err(|_| bad())?;
    if list.len() != count + 1 || list[0] != name {
        return Err(bad());
    }
    list.into_iter()
        .skip(1)
        .map(|v| match v {
            Value::Null => Ok(SqlValue::Null),
            Value::String(t) => Ok(SqlValue::Text(t)),
            Value::Number(n) => match (n.as_i64(), n.as_f64()) {
                (Some(i), _) => Ok(SqlValue::Integer(i)),
                (None, Some(x)) => Ok(SqlValue::Real(x)),
                _ => Err(bad()),
            },
            _ => Err(bad()),
        })
        .collect()
}

/// The terms of an `ORDER BY` list: each expression, and true for `DESC`.
/// A comma inside parentheses is part of its term.
fn sort_keys(order: &str) -> Vec<(String, bool)> {
    let (mut keys, mut depth, mut start) = (Vec::new(), 0, 0);
    let mut push = |term: &str| {
        let term = term.trim();
        keys.push(match term.strip_suffix(" DESC") {
            Some(k) => (k.trim().to_string(), true),
            None => (term.to_string(), false),
        });
    };
    for (i, ch) in order.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                push(&order[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    push(&order[start..]);
    keys
}

/// A condition true for a row that sorts after the row whose sort keys are
/// the parameters `?1`, `?2`, ... In SQLite, NULL sorts first with `ASC`
/// and last with `DESC`.
fn seek_sql(keys: &[(String, bool)]) -> String {
    (0..keys.len())
        .map(|i| {
            let mut terms: Vec<String> = keys[..i]
                .iter()
                .enumerate()
                .map(|(j, (k, _))| format!("({k}) IS ?{}", j + 1))
                .collect();
            let (k, desc) = &keys[i];
            let n = i + 1;
            terms.push(if *desc {
                format!("(({k}) IS NULL AND ?{n} IS NOT NULL OR ({k}) < ?{n})")
            } else {
                format!("(?{n} IS NULL AND ({k}) IS NOT NULL OR ({k}) > ?{n})")
            });
            format!("({})", terms.join(" AND "))
        })
        .collect::<Vec<_>>()
        .join(" OR ")
}

/// One page of a list section of the brief: `limit` items after the sort
/// keys `after` (from the start with `None`), in the order of the brief. Also
/// gives the sort keys of each item of the page.
fn read_section(
    c: &Connection,
    (shown, order): &(String, String),
    tails: &str,
    limit: u32,
    after: Option<&[SqlValue]>,
) -> iris_store::Result<(Section, Vec<Vec<SqlValue>>)> {
    let count = c.query_row(
        &format!("SELECT count(*) FROM item WHERE {shown}"),
        [],
        |r| r.get(0),
    )?;
    // Seek past the sort keys of the last item of the page before, so SQLite
    // keeps only `limit` rows in its sort. An item whose key moves across
    // these keys between two pages can show twice or not show.
    let keys = sort_keys(order);
    let list = keys
        .iter()
        .map(|(k, _)| k.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let seek = after.map_or_else(|| "1".to_string(), |_| seek_sql(&keys));
    let mut st = c.prepare(&format!(
        "SELECT id, form, status, doc, {list} FROM item WHERE ({shown}) AND ({seek})
         ORDER BY {order} LIMIT {limit}"
    ))?;
    let (items, page_keys): (Vec<_>, Vec<_>) = st
        .query_map(rusqlite::params_from_iter(after.unwrap_or_default()), |r| {
            let row_keys = (4..4 + keys.len())
                .map(|i| r.get::<_, SqlValue>(i))
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok((doc_of(r)?, row_keys))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .unzip();
    let mut duplicates = serde_json::Map::new();
    // A possible duplicate whose original is in the same place shows with
    // the original, not as a line of its own.
    let mut st = c.prepare(&format!(
        "SELECT id, form FROM item WHERE duplicate_of = ?1 AND ({tails}) ORDER BY id"
    ))?;
    for doc in &items {
        let Some(id) = doc["id"].as_str() else {
            continue;
        };
        let Some((_, row)) = parse_id(id) else {
            continue;
        };
        let dups: Vec<Value> = st
            .query_map([row], |r| Ok(item_id(r.get(0)?, &r.get::<_, String>(1)?)))?
            .filter_map(|d| d.transpose())
            .map(|d| d.map(Value::String))
            .collect::<rusqlite::Result<_>>()?;
        if !dups.is_empty() {
            duplicates.insert(id.to_string(), dups.into());
        }
    }
    Ok((
        Section {
            count,
            items,
            duplicates,
        },
        page_keys,
    ))
}

/// The count of each record type that is not gone and whose head matches
/// `cond`. A possible duplicate pair counts once (specification 1, section
/// 7.5).
fn count_records(c: &Connection, cond: &str) -> iris_store::Result<serde_json::Map<String, Value>> {
    let mut records = serde_json::Map::new();
    let mut st = c.prepare(&format!(
        "SELECT json_extract(doc, '$.type'), count(*) FROM item
         WHERE {} AND {cond}
         GROUP BY 1 ORDER BY 1",
        head(RECORD)
    ))?;
    for row in st.query_map([], |r| {
        Ok((r.get::<_, Option<String>>(0)?, r.get::<_, i64>(1)?))
    })? {
        let (kind, n) = row?;
        records.insert(kind.unwrap_or_else(|| "other".into()), n.into());
    }
    Ok(records)
}

/// The count of each record type that is not gone, also of the records
/// that a rule hides or bundles: the counts of the Records view.
pub fn records(store: &Store) -> Result<serde_json::Map<String, Value>> {
    Ok(store.read(|c| count_records(c, "1"))?)
}

pub fn brief(store: &Store, now: i64) -> Result<Brief> {
    let [you_owe, owed, conversations, feed, signals, strangers] = section_sql(now);
    let tails = tails(now);
    let brief = store.read(|c| {
        let section =
            |sql: &(String, String)| read_section(c, sql, &tails, MAX_LIMIT, None).map(|(s, _)| s);
        let records = count_records(c, SHOWN)?;
        let hostile = c.query_row(
            "SELECT count(*) FROM message m LEFT JOIN header h ON h.message_id = m.id
             WHERE m.gone_at IS NULL AND m.internal_date >= ?1
               AND ((m.hostile IS NOT NULL AND m.hostile NOT IN ('', '[]'))
                 OR EXISTS (SELECT 1 FROM sender s
                            WHERE s.address = h.from_addr COLLATE NOCASE AND s.kind = 'hostile'))",
            [now - 7 * DAY_MS],
            |r| r.get(0),
        )?;
        let not_understood = c.query_row(
            "SELECT count(*) FROM message
             WHERE gone_at IS NULL AND understood_at IS NULL AND tries >= 3",
            [],
            |r| r.get(0),
        )?;
        let oldest_fetched = c
            .query_row(
                "SELECT min(internal_date) FROM message
                 WHERE fetched_at IS NOT NULL AND gone_at IS NULL",
                [],
                |r| r.get::<_, Option<i64>>(0),
            )?
            .map(rfc3339);
        let all = heads(now);
        // The same filter as the list of `/api/brief/hidden`.
        let hidden = c.query_row(
            &format!("SELECT count(*) FROM item WHERE {}", hidden_sql(now).0),
            [],
            |r| r.get(0),
        )?;
        let rules: Vec<u32> = c
            .prepare(&format!(
                "SELECT DISTINCT json_extract(doc, '$.attention.rule') FROM item
                 WHERE ({all}) AND json_extract(doc, '$.attention.state') = 'bundled'
                   AND json_extract(doc, '$.attention.rule') IS NOT NULL
                 ORDER BY 1"
            ))?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let policy = iris_policy::active(c)?.policy;
        let mut bundles = Vec::new();
        for rule in rules {
            let (section, _) = read_section(c, &bundle_sql(now, rule), &tails, BUNDLE_ITEMS, None)?;
            let name = policy
                .rules
                .iter()
                .find(|r| r.id == rule)
                .map_or_else(|| format!("Rule {rule}"), |r| r.name.clone());
            bundles.push(Bundle {
                rule,
                name,
                section,
            });
        }
        let by_rule = verb_counts(c, BY_RULE)?;
        Ok(Brief {
            you_owe: section(&you_owe)?,
            owed_to_you: section(&owed)?,
            conversations: section(&conversations)?,
            records,
            feed: section(&feed)?,
            signals: section(&signals)?,
            strangers: section(&strangers)?,
            gate: crate::gate::read_gate(c, now, false, crate::gate::GATE_IN_BRIEF, None)?,
            declined: crate::gate::count_gate(c, now, true)?,
            hostile,
            not_understood,
            oldest_fetched,
            proposal: Some(verb_counts(c, TRIAGE_OPEN)?),
            handled: Handled {
                count: hidden + by_rule.count,
                verbs: by_rule.verbs,
            },
            bundles,
            hidden,
            review: crate::review::review_counts(c)?,
        })
    })?;
    Ok(brief)
}

/// One page of a list section of the brief.
#[derive(Debug, Serialize, PartialEq)]
pub struct SectionPage {
    #[serde(flatten)]
    pub section: Section,
    /// The `after` value of the next page (the ID of the last item), or
    /// `None` on the last page.
    pub next: Option<String>,
}

/// One page of the list section `name` (one of [`SECTIONS`], or
/// `bundle-<rule>` for the bundle of a rule) of the brief at the time `now`,
/// with the same filter and order as [`brief`]. `after` is
/// the ID of the last item of the page before. The page starts after the
/// place of that item in the order, also when the item left the section.
pub fn brief_section(
    store: &Store,
    now: i64,
    name: &str,
    limit: u32,
    after: Option<&str>,
) -> Result<Option<SectionPage>> {
    check_limit(limit)?;
    let sql = &match (SECTIONS.iter().position(|n| *n == name), bundle_rule(name)) {
        (Some(index), _) => section_sql(now)[index].clone(),
        (None, Some(rule)) => bundle_sql(now, rule),
        (None, None) if name == HIDDEN => hidden_sql(now),
        (None, None) => return Ok(None),
    };
    let after = after
        .map(|token| decode_cursor(token, name, sort_keys(&sql.1).len()))
        .transpose()?;
    let rule = bundle_rule(name);
    let read = store.read(|c| {
        let page = read_section(c, sql, &tails(now), limit + 1, after.as_deref())?;
        // A bundle is there while the active policy has its `bundle` rule,
        // or while an item is in it (until the next attention refresh).
        let known = |r: u32| -> iris_store::Result<bool> {
            Ok(page.0.count > 0
                || iris_policy::active(c)?
                    .policy
                    .rules
                    .iter()
                    .any(|x| x.id == r && x.effect == iris_policy::Effect::Bundle))
        };
        Ok(match rule {
            Some(r) if !known(r)? => None,
            _ => Some(page),
        })
    })?;
    let Some((mut section, mut page_keys)) = read else {
        return Ok(None);
    };
    let more = section.items.len() > limit as usize;
    section.items.truncate(limit as usize);
    page_keys.truncate(limit as usize);
    let shown: Vec<&str> = section
        .items
        .iter()
        .filter_map(|d| d["id"].as_str())
        .collect();
    section
        .duplicates
        .retain(|k, _| shown.contains(&k.as_str()));
    let next = more
        .then(|| page_keys.last().map(|k| encode_cursor(name, k)))
        .flatten();
    Ok(Some(SectionPage { section, next }))
}

/// The kind of the sender row of `address`, or `stranger` with no row or no
/// kind. Rows can differ only in case: the most cautious kind wins.
/// shortcut: the case-insensitive match scans the sender table, one scan
/// for each message detail and never for a search (about 4 ms for 100,000
/// senders). Add a `lower(address)` index to the store if it shows in a profile.
fn sender_kind(c: &Connection, address: Option<&str>) -> rusqlite::Result<String> {
    let Some(address) = address else {
        return Ok("stranger".into());
    };
    let kind: Option<String> = c
        .query_row(
            // A row with no kind is a stranger.
            "SELECT coalesce(kind, 'stranger') AS k FROM sender WHERE address = ?1 COLLATE NOCASE
             ORDER BY CASE k WHEN 'hostile' THEN 0 WHEN 'stranger' THEN 1
                      WHEN 'service' THEN 2 WHEN 'person' THEN 3 ELSE 4 END
             LIMIT 1",
            [address],
            |r| r.get(0),
        )
        .optional()?;
    Ok(kind.unwrap_or_else(|| "stranger".into()))
}

/// The summary of message `row`, and the other fields of [`Message`]
/// with no summary and no items.
pub(crate) fn message_row(
    c: &Connection,
    row: i64,
) -> rusqlite::Result<Option<(MessageSummary, Message)>> {
    let found = c
        .query_row(
            "SELECT m.thread_id, coalesce(h.date, m.internal_date), h.from_name, h.from_addr,
                    h.subject, h.to_addrs, h.cc_addrs, b.text, m.gone_at IS NOT NULL, m.hostile
             FROM message m LEFT JOIN header h ON h.message_id = m.id
             LEFT JOIN body b ON b.message_id = m.id WHERE m.id = ?1",
            [row],
            |r| {
                let hostile: Option<String> = r.get(9)?;
                Ok((
                    MessageSummary {
                        id: format_id(Kind::Message, row),
                        thread: format_id(Kind::Thread, r.get(0)?),
                        date: r.get::<_, Option<i64>>(1)?.map(rfc3339),
                        from: Address {
                            name: r.get(2)?,
                            address: r.get(3)?,
                        },
                        subject: r.get(4)?,
                        labels: Vec::new(),
                    },
                    Message {
                        summary: empty_summary(),
                        to: r.get(5)?,
                        cc: r.get(6)?,
                        text: r.get(7)?,
                        gone: r.get(8)?,
                        hostile: hostile
                            .and_then(|h| serde_json::from_str(&h).ok())
                            .unwrap_or_default(),
                        sender_kind: String::new(),
                        items: Vec::new(),
                    },
                ))
            },
        )
        .optional()?;
    let Some((mut summary, more)) = found else {
        return Ok(None);
    };
    let mut st = c.prepare_cached(
        "SELECT l.name FROM message_label ml JOIN label l ON l.id = ml.label_id
         WHERE ml.message_id = ?1 ORDER BY l.kind, l.name",
    )?;
    summary.labels = st
        .query_map([row], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(Some((summary, more)))
}

fn empty_summary() -> MessageSummary {
    MessageSummary {
        id: String::new(),
        thread: String::new(),
        date: None,
        from: Address {
            name: None,
            address: None,
        },
        subject: None,
        labels: Vec::new(),
    }
}

/// The document of an item row (id, form, status, doc). The row is the true
/// copy of the ID and the status, so they replace the values of the document.
pub(crate) fn doc_of(r: &rusqlite::Row) -> rusqlite::Result<Value> {
    let id: i64 = r.get(0)?;
    let form: String = r.get(1)?;
    let text: String = r.get(3)?;
    let mut doc: Value = serde_json::from_str(&text).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e))
    })?;
    if let Some(map) = doc.as_object_mut() {
        if let Some(id) = item_id(id, &form) {
            map.insert("id".into(), id.into());
        }
        map.insert("status".into(), Value::String(r.get(2)?));
    }
    Ok(doc)
}

fn item_id(row: i64, form: &str) -> Option<String> {
    FORMS
        .iter()
        .find(|(f, _)| *f == form)
        .map(|(_, k)| format_id(*k, row))
}

pub(crate) fn row_of(id: &str, kind: Kind) -> Result<i64> {
    match parse_id(id) {
        Some((k, row)) if k == kind => Ok(row),
        _ => Err(bad_id(id)),
    }
}

fn bad_id(id: &str) -> Error {
    Error::Query(format!("not a valid ID: {id}"))
}

/// Milliseconds since the Unix epoch as RFC 3339 in UTC, to the second.
pub fn rfc3339(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let rem = secs.rem_euclid(86_400);
    // Days since 1970-01-01 to the civil date: H. Hinnant's `civil_from_days`.
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bundle_name_is_canonical_or_no_bundle() {
        // Property: `bundle_rule(s)` is `Some(r)` exactly when `s` is
        // `bundle-{r}` for a `u32` `r`. Each rule has one name.
        for r in [0, 1, 9, 10, 99, u32::MAX - 1, u32::MAX] {
            assert_eq!(bundle_rule(&format!("bundle-{r}")), Some(r));
        }
        let parts = [
            "bundle-",
            "bundle",
            "-",
            "+",
            "0",
            "1",
            "9",
            "42",
            "4294967295",
            "4294967296",
            " ",
            "\u{0663}",
            "é",
            "%33",
            "x",
            "/",
            "",
        ];
        let mut x = 0x9E37_79B9_7F4A_7C15u64;
        for _ in 0..20_000 {
            let mut name = String::new();
            for _ in 0..(x % 5) {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                name.push_str(parts[(x % parts.len() as u64) as usize]);
            }
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let canonical = name
                .strip_prefix("bundle-")
                .and_then(|n| n.parse::<u32>().ok())
                .filter(|r| format!("bundle-{r}") == name);
            assert_eq!(bundle_rule(&name), canonical, "{name:?}");
            if let Some(r) = bundle_rule(&name) {
                assert_eq!(format!("bundle-{r}"), name);
            }
        }
    }

    #[test]
    fn summaries_leave_out_a_message_that_became_gone() {
        let dir = std::env::temp_dir().join(format!("iris-api-gone-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Store::open(&dir).unwrap();
        let acc = store.add_account("gmail", "owner@example.com").unwrap();
        let msg = |id: &str| iris_store::NewMessage {
            provider_id: id.into(),
            thread_provider_id: "t".into(),
            ..Default::default()
        };
        let ids = store.put_messages(acc, &[msg("a"), msg("b")]).unwrap();
        // As if the sync marked `a` gone between the search and the read.
        store
            .write(|tx| Ok(tx.execute("UPDATE message SET gone_at = 1 WHERE id = ?1", [ids[0]])?))
            .unwrap();
        let got = store.read(|c| Ok(summaries(c, &ids)?)).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].id, format_id(Kind::Message, ids[1]));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rfc3339_gives_utc_dates() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(951_782_400_000), "2000-02-29T00:00:00Z");
        assert_eq!(rfc3339(1_763_283_731_999), "2025-11-16T09:02:11Z");
        assert_eq!(rfc3339(-1000), "1969-12-31T23:59:59Z");
    }
}
