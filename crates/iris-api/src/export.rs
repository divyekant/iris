//! `iris export <path>` (specification 2, section 8.2): each item as JSON
//! Lines, and each message with its raw source. A person can read the export
//! with no Iris. The export holds no secret (specification 1, section 5.4).

use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::Path;

use iris_store::{format_id, new_private_dir, Kind, Store};

use crate::read::{doc_of, message, raw};
use crate::{Error, Result};

/// What [`export`] wrote.
#[derive(Debug, PartialEq, Eq)]
pub struct Exported {
    /// Lines of `items.jsonl`: all items, also the gone items.
    pub items: u64,
    /// Lines of `messages.jsonl`: all messages, also the gone messages.
    pub messages: u64,
    /// Raw messages in `mail/`. A gone message after the content cleanup
    /// has no raw message.
    pub files: u64,
}

fn io(e: io::Error) -> Error {
    Error::Store(e.into())
}

fn line(out: &mut impl Write, v: &serde_json::Value) -> Result<()> {
    serde_json::to_writer(&mut *out, v).map_err(|e| io(e.into()))?;
    out.write_all(b"\n").map_err(io)
}

/// Write the export to the folder `dest`, which must not exist or must be
/// empty. The folder gets mode 0700, because it holds the mail:
///
/// - `items.jsonl`: one item document of the open model on each line.
/// - `messages.jsonl`: one message on each line, as `/api/messages/{id}`
///   gives it, with `file`: the path of its raw message in the export, or
///   `null`.
/// - `mail/<m-id>.eml`: the raw message (RFC 5322) of each message.
pub fn export(store: &Store, dest: &Path) -> Result<Exported> {
    // The content cleanup cannot delete a raw message during the export.
    let _lock = store.backup_lock()?;
    new_private_dir(dest)?;
    let mail = dest.join("mail");
    fs::create_dir(&mail).map_err(io)?;

    let mut items = BufWriter::new(File::create(dest.join("items.jsonl")).map_err(io)?);
    let mut n_items = 0;
    store.read(|c| {
        let mut st = c.prepare("SELECT id, form, status, doc FROM item ORDER BY id")?;
        let mut rows = st.query([])?;
        while let Some(r) = rows.next()? {
            serde_json::to_writer(&mut items, &doc_of(r)?).map_err(io::Error::from)?;
            items.write_all(b"\n")?;
            n_items += 1;
        }
        Ok(())
    })?;
    items.flush().map_err(io)?;

    // shortcut: one read for each message (about 3 queries); a single query
    // is faster if an export of a large mailbox is too slow.
    let rows: Vec<i64> = store.read(|c| {
        let mut st = c.prepare("SELECT id FROM message ORDER BY id")?;
        let ids = st
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(ids)
    })?;
    let mut messages = BufWriter::new(File::create(dest.join("messages.jsonl")).map_err(io)?);
    let (mut n_messages, mut files) = (0, 0);
    for row in rows {
        let id = format_id(Kind::Message, row);
        let Some(m) = message(store, &id)? else {
            continue;
        };
        let mut v = serde_json::to_value(&m).map_err(|e| io(e.into()))?;
        let file = match raw(store, &id)? {
            Some(src) => {
                let name = format!("mail/{id}.eml");
                fs::copy(src, dest.join(&name)).map_err(io)?;
                files += 1;
                Some(name)
            }
            None => None,
        };
        v["file"] = file.into();
        line(&mut messages, &v)?;
        n_messages += 1;
    }
    messages.flush().map_err(io)?;
    Ok(Exported {
        items: n_items,
        messages: n_messages,
        files,
    })
}
