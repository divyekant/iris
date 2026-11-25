//! `iris export <path>` (specification 2, section 8.2): each item as JSON
//! Lines, and each message with its raw source. A person can read the export
//! with no Iris. The export holds no secret (specification 1, section 5.4).

use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::Path;

use iris_store::{format_id, new_private_dir, Kind, Store};

use crate::read::{doc_of, message_in, MATTER};
use crate::Result;

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

fn line(out: &mut impl Write, v: &impl serde::Serialize) -> io::Result<()> {
    serde_json::to_writer(&mut *out, v)?;
    out.write_all(b"\n")
}

/// Write the export to the folder `dest`, which must not exist or must be
/// empty. The folder gets mode 0700, because it holds the mail:
///
/// - `items.jsonl`: one item document of the open model on each line.
/// - `messages.jsonl`: one message on each line, as `/api/messages/{id}`
///   gives it, with `file`: the path of its raw message in the export, or
///   `null` when the message has no content (`content_sha256` is null), and
///   `hostile_mail`: `true` if the message has a hostile mark, the label
///   SPAM or a hostile sender. The other reads of the API hide the items of
///   such a message; the export keeps them, and `items` of the message names
///   them (#819).
/// - `mail/<m-id>.eml`: the raw message (RFC 5322) of each message.
///
/// All files come from one read transaction, so each reference in them
/// agrees: a message names only items of `items.jsonl`. The rows stream
/// from SQLite, so memory does not grow with the mailbox. A message that
/// refers to a content file that is not there is an error
/// ([`iris_store::Error::MissingContent`]): the export is not complete.
///
/// The files go to the folder `.<name>.partial` next to `dest` first, and a
/// rename moves them into `dest` at the end. After an error, `dest` stays
/// empty and the partial folder is removed. After a crash, only the folder
/// with `.partial` in its name holds files (#245).
pub fn export(store: &Store, dest: &Path) -> Result<Exported> {
    // The content cleanup cannot delete a raw message during the export.
    let _lock = store.backup_lock()?;
    new_private_dir(dest)?;
    let Some(name) = dest.file_name() else {
        return Err(crate::Error::Query(
            "the export folder must have a name".into(),
        ));
    };
    let partial = dest.with_file_name(format!(".{}.partial", name.to_string_lossy()));
    // The folder of an export that stopped: Iris made it, and it is not complete.
    match fs::remove_dir_all(&partial) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => {
            return Err(iris_store::Error::from(e).into())
        }
        _ => {}
    }
    new_private_dir(&partial)?;
    let n = match write(store, &partial) {
        Ok(n) => n,
        Err(e) => {
            let _ = fs::remove_dir_all(&partial);
            return Err(e.into());
        }
    };
    // `dest` is an empty folder: a rename replaces it.
    if let Err(e) = fs::rename(&partial, dest) {
        let _ = fs::remove_dir_all(&partial);
        return Err(iris_store::Error::from(e).into());
    }
    Ok(n)
}

fn write(store: &Store, dest: &Path) -> iris_store::Result<Exported> {
    let mail = dest.join("mail");
    fs::create_dir(&mail)?;
    let mut items = BufWriter::new(File::create(dest.join("items.jsonl"))?);
    let mut messages = BufWriter::new(File::create(dest.join("messages.jsonl"))?);
    let mut n = Exported {
        items: 0,
        messages: 0,
        files: 0,
    };
    store.read(|c| {
        // One snapshot for all reads: it starts at the first read.
        let tx = c.unchecked_transaction()?;
        let mut st = tx.prepare(&format!(
            "SELECT id, form, status, doc, {MATTER} FROM item ORDER BY id"
        ))?;
        let mut rows = st.query([])?;
        while let Some(r) = rows.next()? {
            line(&mut items, &doc_of(r)?)?;
            n.items += 1;
        }
        let mut st = tx.prepare(&format!(
            "SELECT m.id, m.content_sha256, {} FROM message m ORDER BY m.id",
            iris_store::hostile_mail!()
        ))?;
        let mut rows = st.query([])?;
        while let Some(r) = rows.next()? {
            let (row, sha, hostile): (i64, Option<String>, bool) =
                (r.get(0)?, r.get(1)?, r.get(2)?);
            let Some(m) = message_in(&tx, row)? else {
                continue;
            };
            let id = format_id(Kind::Message, row);
            let file = match sha {
                Some(sha) => {
                    let src = store
                        .content_path(&sha)
                        .ok_or_else(|| iris_store::Error::MissingContent(sha.clone()))?;
                    let name = format!("mail/{id}.eml");
                    match fs::copy(&src, dest.join(&name)) {
                        Err(e) if e.kind() == io::ErrorKind::NotFound => {
                            return Err(iris_store::Error::MissingContent(sha))
                        }
                        r => r?,
                    };
                    n.files += 1;
                    Some(name)
                }
                None => None,
            };
            let mut v = serde_json::to_value(&m).map_err(io::Error::from)?;
            v["file"] = file.into();
            v["hostile_mail"] = hostile.into();
            line(&mut messages, &v)?;
            n.messages += 1;
        }
        Ok(())
    })?;
    items.flush()?;
    messages.flush()?;
    Ok(n)
}
