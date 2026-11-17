//! The search text of `iris search` (specification 1, section 5.6).
//!
//! The text has words, phrases in quotes, and the filters `from:`, `to:`,
//! `after:`, `before:`, `label:` and `form:`. A filter value can be in quotes:
//! `from:"Ana Lee"`. Each word and phrase must be in the message. The
//! filters become SQL conditions with bound values. Each word and phrase
//! becomes one FTS5 string in quotes, so no text of a person or an agent can
//! be an FTS5 operator.

use rusqlite::types::Value;
use rusqlite::Connection;

use crate::{Error, Result};

const FILTERS: [&str; 6] = ["from", "to", "after", "before", "label", "form"];
const FORMS: [&str; 5] = ["commitment", "conversation", "record", "feed", "signal"];

#[derive(Clone, Debug, PartialEq, Eq)]
enum Filter {
    From(String),
    To(String),
    After(i64),
    Before(i64),
    Label(String),
    Form(String),
}

/// A parsed search.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Query {
    /// Words and phrases, as the person wrote them.
    terms: Vec<String>,
    filters: Vec<Filter>,
}

impl Query {
    pub fn parse(text: &str) -> Result<Query> {
        let mut q = Query::default();
        let mut rest = text.trim_start();
        while !rest.is_empty() {
            let filter = FILTERS.iter().find_map(|n| {
                let tail = rest.strip_prefix(n)?.strip_prefix(':')?;
                Some((*n, tail))
            });
            let Some((name, tail)) = filter else {
                // A word, a phrase, or a word with an unknown filter name.
                let (token, _, tail) = next_token(rest);
                rest = tail.trim_start();
                q.terms.push(token);
                continue;
            };
            let (value, _, tail) = next_token(tail);
            rest = tail.trim_start();
            if value.is_empty() {
                return Err(Error::Query(format!("the filter {name}: has no value")));
            }
            q.filters.push(match name {
                "from" => Filter::From(value),
                "to" => Filter::To(value),
                "after" => Filter::After(day_ms(&value)?),
                "before" => Filter::Before(day_ms(&value)?),
                "label" => Filter::Label(value),
                _ => {
                    let form = value.to_ascii_lowercase();
                    if !FORMS.contains(&form.as_str()) {
                        return Err(Error::Query(format!(
                            "form: takes one of {}",
                            FORMS.join(", ")
                        )));
                    }
                    Filter::Form(form)
                }
            });
        }
        Ok(q)
    }

    /// The FTS5 query: each term as one string in quotes. An FTS5 string ends
    /// at a `"`, and `""` is a `"` inside the string.
    fn fts(&self) -> Option<String> {
        let parts: Vec<String> = self
            .terms
            .iter()
            .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
            .collect();
        (!parts.is_empty()).then(|| parts.join(" "))
    }

    pub(crate) fn run(&self, conn: &Connection, limit: u32) -> Result<Vec<i64>> {
        self.run_with(conn, limit, None)
    }

    /// [`Query::run`] for a job session: over the granted `messages` only,
    /// and `form:` counts only the granted `items`. A session decides each
    /// match only from what its workspace holds, so `label:` is refused: a
    /// workspace holds no labels. The IDs go in as JSON arrays, so the
    /// query has one bound value for each set of IDs.
    pub fn run_granted(
        &self,
        conn: &Connection,
        limit: u32,
        messages: &std::collections::BTreeSet<i64>,
        items: &std::collections::BTreeSet<i64>,
    ) -> Result<Vec<i64>> {
        self.check_granted()?;
        self.run_with(conn, limit, Some((messages, items)))
    }

    /// Refuse a query that a job session cannot run: `label:`, because a
    /// workspace holds no labels.
    pub fn check_granted(&self) -> Result<()> {
        if self.filters.iter().any(|f| matches!(f, Filter::Label(_))) {
            return Err(Error::Query(
                "label: is not a filter of a job session: a workspace holds no labels".into(),
            ));
        }
        Ok(())
    }

    fn run_with(
        &self,
        conn: &Connection,
        limit: u32,
        grant: Option<(
            &std::collections::BTreeSet<i64>,
            &std::collections::BTreeSet<i64>,
        )>,
    ) -> Result<Vec<i64>> {
        let json = |ids: &std::collections::BTreeSet<i64>| -> Value {
            let list: Vec<String> = ids.iter().map(i64::to_string).collect();
            format!("[{}]", list.join(",")).into()
        };
        let (mut sql, mut args) = self.select(grant.map(|(_, items)| json(items)));
        if let Some((messages, _)) = grant {
            args.push(json(messages));
            sql.push_str(&format!(
                " AND m.id IN (SELECT value FROM json_each(?{}))",
                args.len()
            ));
        }
        sql.push_str(" ORDER BY m.internal_date DESC, m.id DESC LIMIT ");
        sql.push_str(&limit.to_string());
        let mut st = conn.prepare(&sql)?;
        let rows = st.query_map(rusqlite::params_from_iter(args), |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The first message that matches and arrived for a watch (card R5-07):
    /// a row ID above `after`, a date from `since`, clean text and no
    /// `link_check` row (so the hostile checks ran), no hostile mark or
    /// hostile sender, and no label `SENT` or
    /// `DRAFT`. The lowest row ID is the first arrival.
    pub(crate) fn first_arrival(
        &self,
        conn: &Connection,
        after: i64,
        since: i64,
    ) -> Result<Option<i64>> {
        let (mut sql, mut args) = self.select(None);
        args.push(after.into());
        args.push(since.into());
        let (a, s) = (args.len() - 1, args.len());
        sql.push_str(&format!(
            " AND m.id > ?{a} AND m.internal_date >= ?{s}
              AND EXISTS (SELECT 1 FROM body b WHERE b.message_id = m.id)
              AND NOT {may_be_hostile}
              AND NOT EXISTS (SELECT 1 FROM header h JOIN sender x
                    ON x.address = h.from_addr COLLATE NOCASE
                  WHERE h.message_id = m.id AND x.kind = 'hostile')
              AND NOT EXISTS (SELECT 1 FROM message_label ml JOIN label l ON l.id = ml.label_id
                  WHERE ml.message_id = m.id AND l.provider_id IN ('SENT', 'DRAFT'))
             ORDER BY m.id LIMIT 1",
            may_be_hostile = crate::may_be_hostile!(),
        ));
        let mut st = conn.prepare(&sql)?;
        let mut rows = st.query_map(rusqlite::params_from_iter(args), |r| r.get(0))?;
        Ok(rows.next().transpose()?)
    }

    /// The SELECT of the matching messages that are not gone, with its bound
    /// values `?1` to `?n`. With `items` (a JSON array of item IDs), `form:`
    /// counts only those items.
    fn select(&self, items: Option<Value>) -> (String, Vec<Value>) {
        let mut sql = String::from("SELECT m.id FROM message m WHERE m.gone_at IS NULL");
        let mut args: Vec<Value> = Vec::new();
        let mut arg = |sql: &mut String, cond: &str, v: Value| {
            args.push(v);
            sql.push_str(&cond.replace('?', &format!("?{}", args.len())));
        };
        if let Some(fts) = self.fts() {
            arg(
                &mut sql,
                " AND m.id IN (SELECT rowid FROM message_fts WHERE message_fts MATCH ?)",
                fts.into(),
            );
        }
        for f in &self.filters {
            match f {
                Filter::From(v) => arg(
                    &mut sql,
                    " AND m.id IN (SELECT message_id FROM header
                       WHERE from_addr LIKE ? ESCAPE '\\' OR from_name LIKE ? ESCAPE '\\')",
                    like(v),
                ),
                Filter::To(v) => arg(
                    &mut sql,
                    " AND m.id IN (SELECT message_id FROM header
                       WHERE to_addrs LIKE ? ESCAPE '\\' OR cc_addrs LIKE ? ESCAPE '\\')",
                    like(v),
                ),
                Filter::After(ms) => arg(&mut sql, " AND m.internal_date >= ?", (*ms).into()),
                Filter::Before(ms) => arg(&mut sql, " AND m.internal_date < ?", (*ms).into()),
                Filter::Label(v) => arg(
                    &mut sql,
                    " AND EXISTS (SELECT 1 FROM message_label ml JOIN label l ON l.id = ml.label_id
                       WHERE ml.message_id = m.id AND (l.name = ? COLLATE NOCASE OR l.provider_id = ?))",
                    v.clone().into(),
                ),
                Filter::Form(v) => {
                    arg(
                        &mut sql,
                        " AND EXISTS (SELECT 1 FROM item_source s JOIN item i ON i.id = s.item_id
                           WHERE s.message_id = m.id AND i.form = ?",
                        v.clone().into(),
                    );
                    if let Some(items) = &items {
                        arg(
                            &mut sql,
                            " AND i.id IN (SELECT value FROM json_each(?))",
                            items.clone(),
                        );
                    }
                    sql.push(')');
                }
            }
        }
        (sql, args)
    }
}

/// A LIKE pattern for "contains `v`". LIKE ignores the case of ASCII letters.
fn like(v: &str) -> Value {
    let esc = v
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{esc}%").into()
}

/// Take one token: a string in quotes (the quotes removed), or the text up
/// to the next space. Gives the token, if it was in quotes, and the rest.
fn next_token(text: &str) -> (String, bool, &str) {
    if let Some(inner) = text.strip_prefix('"') {
        // A quote with no end runs to the end of the text.
        let end = inner.find('"').unwrap_or(inner.len());
        let tail = inner.get(end + 1..).unwrap_or("");
        return (inner[..end].to_string(), true, tail);
    }
    let end = text.find(char::is_whitespace).unwrap_or(text.len());
    (text[..end].to_string(), false, &text[end..])
}

/// The start of a day `YYYY-MM-DD`, UTC, in milliseconds since the Unix epoch.
fn day_ms(text: &str) -> Result<i64> {
    let bad = || Error::Query(format!("{text} is not a date in the form YYYY-MM-DD"));
    let parts: Vec<&str> = text.split('-').collect();
    let [y, m, d] = parts[..] else {
        return Err(bad());
    };
    if y.len() != 4 || m.len() != 2 || d.len() != 2 {
        return Err(bad());
    }
    let num = |s: &str| {
        s.parse::<i64>()
            .ok()
            .filter(|_| s.bytes().all(|b| b.is_ascii_digit()))
    };
    let (y, m, d) = (
        num(y).ok_or_else(bad)?,
        num(m).ok_or_else(bad)?,
        num(d).ok_or_else(bad)?,
    );
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days_in = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if !(1..=12).contains(&m) || d < 1 || d > days_in[(m - 1) as usize] {
        return Err(bad());
    }
    // Days from the civil date, after Howard Hinnant's `days_from_civil`.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Ok((era * 146_097 + doe - 719_468) * 86_400_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_terms_and_filters() {
        let q = Query::parse(
            r#"invoice "due friday" from:"Ana Lee" to:bo label:INBOX form:Record after:2025-01-31"#,
        )
        .unwrap();
        assert_eq!(q.terms, ["invoice", "due friday"]);
        assert_eq!(
            q.filters,
            [
                Filter::From("Ana Lee".into()),
                Filter::To("bo".into()),
                Filter::Label("INBOX".into()),
                Filter::Form("record".into()),
                Filter::After(1_738_281_600_000),
            ]
        );
    }

    #[test]
    fn escapes_fts_operators() {
        let q = Query::parse(r#"a" OR b NEAR(c) * -d ^e x"y"#).unwrap();
        assert_eq!(
            q.fts().unwrap(),
            r#""a""" "OR" "b" "NEAR(c)" "*" "-d" "^e" "x""y""#
        );
        // An unknown filter name is a plain word.
        assert_eq!(Query::parse("subject:x").unwrap().terms, ["subject:x"]);
    }

    #[test]
    fn rejects_bad_filters() {
        for bad in [
            "from:",
            "from: x",
            "to:\"\"",
            "after:2025-13-01",
            "before:2025-02-29",
            "after:25-01-01",
            "after:2025-1-01",
            "form:email",
        ] {
            assert!(Query::parse(bad).is_err(), "{bad}");
        }
        assert!(Query::parse("before:2024-02-29").is_ok());
    }

    #[test]
    fn day_ms_matches_known_days() {
        assert_eq!(day_ms("1970-01-01").unwrap(), 0);
        assert_eq!(day_ms("2000-03-01").unwrap(), 951_868_800_000);
        assert_eq!(day_ms("2026-11-15").unwrap(), 1_794_700_800_000);
    }
}
