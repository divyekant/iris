//! Notices by rule (card R5-08, `docs/specs/2026-11-16-notices.md`): one
//! notice for each new mail that a `notify` rule of the active policy
//! matches. Job notices (`docs/specs/2026-11-25-job-notices.md`): one
//! notice for each move of a job to `needs_you`, which a trigger of the
//! store adds in the transaction of the move. The engine shows each new
//! notice in the system, and the web client shows each notice that is open.

use iris_store::{Result, Store};
use rusqlite::{params, Connection, OptionalExtension};

/// A mail is new for one hour after its fetch.
pub const WINDOW_MS: i64 = 3_600_000;

/// A mail that the understanding reads late is a candidate up to this time
/// after its fetch. After that, the interrupt comes too late to help.
pub const LATE_MS: i64 = 24 * WINDOW_MS;

/// When a model is set and can read the mail (`iris_store::model_can_read!`),
/// the mail waits up to this time after its fetch for understanding, so that
/// a `phishing` mark that only the model finds can stop the notice first
/// (issue #321). A mail that the model cannot read does not wait. After the
/// wait, the notice shows [`UNCHECKED_TITLE`] and [`UNCHECKED_TEXT`], not
/// the words of the mail (issue #1045).
pub const MODEL_WAIT_MS: i64 = 5 * 60_000;

/// The title of a notice of a mail whose understanding did not finish ([`checked_sql`]).
pub const UNCHECKED_TITLE: &str = "Mail not checked yet";
/// The text of a notice of a mail whose understanding did not finish.
pub const UNCHECKED_TEXT: &str = "Iris shows the sender and the subject after the phishing check.";

/// The most characters of the title and of the text of a notice.
pub const WORDS_MAX: usize = 200;

/// The most notices that [`list`] gives.
pub const LIST_MAX: i64 = 50;

/// One notice: a rule notice (a mail and a rule) or a job notice (a job).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub id: i64,
    /// The mail of a rule notice.
    pub message_id: Option<i64>,
    /// The rule of a rule notice.
    pub rule: Option<u32>,
    /// The job of a job notice.
    pub job_id: Option<i64>,
    /// The name of the rule, or `Job due` or `Job question`.
    pub rule_name: String,
    /// The sender, or `j-<n>`, one line.
    pub title: String,
    /// The subject, or the goal of the job, one line.
    pub text: String,
    /// Milliseconds since 1970-01-01 UTC.
    pub created_at: i64,
    /// False for a rule notice whose mail is not checked now
    /// ([`checked_sql`], issue #1045): `title` and `text` are then
    /// [`UNCHECKED_TITLE`] and [`UNCHECKED_TEXT`], and each output that adds
    /// words of the mail to the notice (a sender, a subject) must leave them
    /// out. True for a job notice.
    pub checked: bool,
}

/// Adds a notice for each new mail that a `notify` rule of the active
/// policy matches, at the time `now_ms`, and gives the new notices. A mail
/// is a candidate when it came into the mirror, or the understanding wrote
/// its items, [`WINDOW_MS`] or less before `now_ms`, so that a rule on a form
/// waits for the model, up to [`LATE_MS`] after the fetch. Its date must be at most [`WINDOW_MS`] before the
/// fetch, so that the old mail of a sync gives no notice. A mail with no `body` row waits: the
/// hostile check writes with the body. A mail with a `link_check` row waits
/// too: its link check did not run yet (migration 20). When a model is set,
/// a mail that understanding did not read yet waits up to [`MODEL_WAIT_MS`]
/// after its fetch, for a model `phishing` mark, while the model can read
/// it (`iris_store::model_can_read!`). The notice of a mail whose
/// understanding did not finish shows no words of the mail, until it
/// finishes ([`checked_sql`], [`shown_words`]). Each guard reads the store in the
/// transaction that writes the notice. A notice keeps the rule and the
/// policy version of its time.
pub fn refresh(store: &Store, now_ms: i64) -> Result<Vec<Notice>> {
    store.write(|tx| {
        let active = crate::active(tx)?;
        let Some(version) = active.version else {
            return Ok(Vec::new());
        };
        if !active
            .policy
            .rules
            .iter()
            .any(|r| r.effect == crate::Effect::Notify)
        {
            return Ok(Vec::new());
        }
        let ids: Vec<i64> = tx
            .prepare(concat!(
                "SELECT m.id FROM message m
                 WHERE (m.fetched_at >= ?1 OR (m.understood_at >= ?1 AND m.fetched_at >= ?3))
                   AND m.gone_at IS NULL
                   AND m.internal_date >= m.fetched_at - ?2
                   AND EXISTS (SELECT 1 FROM body b WHERE b.message_id = m.id)
                   AND NOT ", iris_store::may_be_hostile!(), "
                   AND NOT ", iris_store::hostile_mail!(), "
                   AND NOT EXISTS (SELECT 1 FROM notice n WHERE n.message_id = m.id)
                   AND NOT EXISTS (SELECT 1 FROM message_label ml JOIN label l ON l.id = ml.label_id
                       WHERE ml.message_id = m.id AND l.provider_id IN ('SPAM', 'TRASH'))
                   AND NOT ", iris_store::draft_or_copy!("m"), "
                   AND (m.understood_at IS NOT NULL
                        OR NOT EXISTS (SELECT 1 FROM model_setting)
                        OR m.fetched_at <= ?4
                        OR NOT ", iris_store::model_can_read!(), ")
                 ORDER BY m.id",
            ))?
            .query_map(
                params![now_ms - WINDOW_MS, WINDOW_MS, now_ms - LATE_MS, now_ms - MODEL_WAIT_MS],
                |r| r.get(0),
            )?
            .collect::<rusqlite::Result<_>>()?;
        let now = now_ms.div_euclid(1000);
        let mut out = Vec::new();
        for id in ids {
            let Some(f) = crate::facts(tx, id, now)? else {
                continue;
            };
            let Some(rule) = active.policy.notice(&f.message(), now) else {
                continue;
            };
            let rule_name = active
                .policy
                .rules
                .iter()
                .find(|r| r.id == rule)
                .map_or(String::new(), |r| r.name.clone());
            // The row keeps the words of the mail. Each read shows them only
            // while the mail is checked ([`shown_words`]).
            let (title, text) = words(tx, id)?;
            let checked = checked_mail(tx, id)?;
            let (shown_title, shown_text) = if checked {
                (title.clone(), text.clone())
            } else {
                (UNCHECKED_TITLE.to_string(), UNCHECKED_TEXT.to_string())
            };
            let added = tx.execute(
                "INSERT INTO notice(message_id, rule, rule_name, policy_version, title, text, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) ON CONFLICT(message_id) DO NOTHING",
                params![id, rule, rule_name, version, title, text, now_ms],
            )?;
            if added > 0 {
                out.push(Notice {
                    id: tx.last_insert_rowid(),
                    message_id: Some(id),
                    rule: Some(rule),
                    job_id: None,
                    rule_name,
                    title: shown_title,
                    text: shown_text,
                    created_at: now_ms,
                    checked,
                });
            }
        }
        Ok(out)
    })
}

/// The SQL condition "the notice of the message with the row ID `id` (an
/// SQL term) can show the words of the mail": no model is set, or the
/// understanding of the message finished
/// (`understood_at`). [`shown_words`] and [`checked_mail`] read it. The
/// understanding finishes after the model read the
/// mail, or with no model call when a fixed rule completes it or the
/// sender is a declined stranger (spec 1 section 7.3, decision D29): the
/// same end of understanding that guard 4 waits for (issue #321).
fn checked_sql(id: &str) -> String {
    format!(
        "(NOT EXISTS (SELECT 1 FROM model_setting)
          OR EXISTS (SELECT 1 FROM message cm WHERE cm.id = {id} AND cm.understood_at IS NOT NULL))"
    )
}

/// True if the notice of message `id` can show the words of the mail
/// ([`checked_sql`]).
fn checked_mail(tx: &Connection, id: i64) -> Result<bool> {
    Ok(tx.query_row(&format!("SELECT {}", checked_sql("?1")), [id], |r| r.get(0))?)
}

/// The sender and the subject of a message, one line each.
fn words(tx: &Connection, id: i64) -> Result<(String, String)> {
    let (name, addr, subject): (Option<String>, Option<String>, Option<String>) = tx
        .query_row(
            "SELECT h.from_name, h.from_addr, coalesce(h.subject, b.subject)
             FROM message m LEFT JOIN header h ON h.message_id = m.id
             LEFT JOIN body b ON b.message_id = m.id WHERE m.id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?
        .unwrap_or_default();
    let pick = |v: Option<String>| v.map(|s| line(&s)).filter(|s| !s.is_empty());
    let title = pick(name)
        .or_else(|| pick(addr))
        .unwrap_or_else(|| "Unknown sender".into());
    let text = pick(subject).unwrap_or_else(|| "(No subject)".into());
    Ok((title, text))
}

/// One line of at most [`WORDS_MAX`] characters: each control character and
/// each run of white space becomes one space.
pub fn line(s: &str) -> String {
    let one = one_line(s);
    if one.chars().count() <= WORDS_MAX {
        one
    } else {
        one.chars().take(WORDS_MAX - 3).collect::<String>() + "..."
    }
}

/// One line with no cut: each control character and each run of white
/// space becomes one space.
pub fn one_line(s: &str) -> String {
    s.split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The SQL condition "the notice is open": not done, its mail is not
/// hostile mail now, and a job notice is for the newest event of its job (an
/// answer, a wait or an end closes it with no write). A mark that came after the notice (a model `phishing`
/// mark, a `hostile` sender, the label `SPAM`, a pending link check) hides
/// the notice, so a notice never points to phishing (decision D33). The
/// content cleanup of gone mail marks such a notice done before it deletes
/// the evidence (`Store::clean_gone`). Each read of notices and the system
/// notice ([`is_open`]) use it.
const OPEN: &str = concat!(
    "done_at IS NULL AND NOT EXISTS (SELECT 1 FROM message m WHERE m.id = notice.message_id AND (",
    iris_store::hostile_mail!(),
    " OR ",
    iris_store::may_be_hostile!(),
    ")) AND (notice.job_event_id IS NULL OR notice.job_event_id = \
     (SELECT max(e.id) FROM job_event e WHERE e.job_id = notice.job_id))"
);

/// The SQL condition "the notice interrupts": a rule notice, or a job
/// notice of a move by the engine (`Job due`). A question of an agent
/// (`Job question`) waits in the list with no system notice: Iris interrupts
/// only by a rule of the person or on the due date of a job (`PRODUCT.md`).
/// The term reads `by` of the event, not the name, because a rule name is
/// text of the person.
const INTERRUPTS: &str = "(notice.job_event_id IS NULL OR EXISTS (SELECT 1 FROM job_event e \
     WHERE e.id = notice.job_event_id AND e.by = 'engine'))";

/// True while notice `id` is open ([`OPEN`]). The engine checks it again
/// just before it shows a system notice, because a notice can wait in the
/// queue while the mail becomes hostile.
pub fn is_open(conn: &Connection, id: i64) -> Result<bool> {
    Ok(conn.query_row(
        &format!("SELECT EXISTS (SELECT 1 FROM notice WHERE id = ?1 AND {OPEN})"),
        [id],
        |r| r.get(0),
    )?)
}

/// The notices that are open, newest first, at most [`LIST_MAX`].
pub fn list(conn: &Connection) -> Result<Vec<Notice>> {
    select(
        conn,
        &format!("WHERE {OPEN} ORDER BY id DESC LIMIT ?1"),
        params![LIST_MAX],
    )
}

/// The notices that are open and interrupt ([`INTERRUPTS`]) with a row ID
/// larger than `after`, oldest first, at most `limit` (the system notices of
/// the Mac app, `docs/specs/2026-11-17-mac-app.md`).
pub fn after(conn: &Connection, after: i64, limit: i64) -> Result<Vec<Notice>> {
    select(
        conn,
        &format!("WHERE {OPEN} AND {INTERRUPTS} AND id > ?1 ORDER BY id LIMIT ?2"),
        params![after, limit],
    )
}

/// The open job notices that interrupt (`Job due`) with a row ID larger
/// than `after`, oldest first, at most [`LIST_MAX`]: the engine reads the new
/// ones each turn for the system notice.
pub fn jobs_after(conn: &Connection, after: i64) -> Result<Vec<Notice>> {
    select(
        conn,
        &format!(
            "WHERE {OPEN} AND {INTERRUPTS} AND job_event_id IS NOT NULL AND id > ?1 ORDER BY id LIMIT ?2"
        ),
        params![after, LIST_MAX],
    )
}

/// The count of notices that are open, and the largest row ID of all
/// notices (0 with no notice).
pub fn counts(conn: &Connection) -> Result<(i64, i64)> {
    Ok(conn.query_row(
        &format!("SELECT count(*) FILTER (WHERE {OPEN}), coalesce(max(id), 0) FROM notice"),
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?)
}

/// The columns of the title and the text that a read of the notices shows: the words of
/// the row while the mail of a rule notice is [`checked_sql`] now, else
/// [`UNCHECKED_TITLE`] and [`UNCHECKED_TEXT`]. The check runs at each read,
/// so a model that is set after the notice, or a notice from before
/// issue #1045, shows no words of a mail whose understanding did not finish.
/// A job notice shows its words.
fn shown_words() -> String {
    format!(
        "(notice.message_id IS NULL OR {}), title, text",
        checked_sql("notice.message_id")
    )
}

fn select(conn: &Connection, rest: &str, p: impl rusqlite::Params) -> Result<Vec<Notice>> {
    let rows = conn
        .prepare(&format!(
            "SELECT id, message_id, rule, job_id, rule_name, {}, created_at FROM notice {rest}",
            shown_words()
        ))?
        .query_map(p, |r| {
            let job_id: Option<i64> = r.get(3)?;
            // The goal of a job is the words of the person: one line here,
            // with no cut (the store keeps at most 2,000 characters). The
            // system notice cuts its own text.
            let one = |s: String| if job_id.is_some() { one_line(&s) } else { s };
            let checked: bool = r.get(5)?;
            let (title, text) = if checked {
                (one(r.get(6)?), one(r.get(7)?))
            } else {
                (UNCHECKED_TITLE.to_string(), UNCHECKED_TEXT.to_string())
            };
            Ok(Notice {
                id: r.get(0)?,
                message_id: r.get(1)?,
                rule: r.get(2)?,
                job_id,
                rule_name: r.get(4)?,
                title,
                text,
                created_at: r.get(8)?,
                checked,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// Marks notice `id` done at `now_ms`. False if no notice has the ID. A
/// notice that is done already stays as it is.
pub fn done(store: &Store, id: i64, now_ms: i64) -> Result<bool> {
    store.write(|tx| {
        tx.execute(
            "UPDATE notice SET done_at = ?2 WHERE id = ?1 AND done_at IS NULL",
            params![id, now_ms],
        )?;
        Ok(tx
            .query_row("SELECT 1 FROM notice WHERE id = ?1", [id], |_| Ok(()))
            .optional()?
            .is_some())
    })
}
