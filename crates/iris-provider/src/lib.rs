//! The mail provider interface of Iris (spec `docs/specs/2026-11-17-provider.md`).
//!
//! The sync, the verify, the apply and the send call a mail provider only
//! through [`Provider`]. Gmail (`iris-gmail`) is the first provider. Each
//! provider maps its own model to the label model of the mirror (spec,
//! section 4) and its own answers to [`Error`] (spec, section 5).

use std::io::Write;

use serde::Deserialize;

/// The most message IDs in one [`Provider::modify`] call. A provider with a
/// lower limit splits the call (spec, section 6).
pub const BATCH_LIMIT: usize = 1000;
/// The most bytes of a draft that [`Provider::draft_raw`] keeps. A draft
/// of Iris has a body of at most 256 KiB, so a larger message is not in the
/// form that Iris writes.
pub const DRAFT_LIMIT: usize = 4 * 1024 * 1024;
/// The most reads of messages that the sync runs at the same time.
pub const MAX_GETS: usize = 8;

#[derive(Debug)]
pub enum Error {
    /// The provider refused the secret. Get a new one one time, then call
    /// again.
    Unauthorized,
    /// The provider does not have the message, the label or the draft.
    NotFound,
    /// The provider cannot give the changes from the cursor. Do a full
    /// resync.
    HistoryExpired,
    /// Another error, with an HTTP status class (spec, section 5).
    /// `reason` is a short code of the provider.
    Api {
        status: u16,
        reason: String,
        message: String,
    },
    /// The network failed, or the answer stopped before its end.
    Transport(String),
    /// The answer is not in the shape that the provider expects.
    Decode(String),
    /// The sink of a raw message failed.
    Sink(std::io::Error),
}

/// The text of a [`Error::is_scope_refusal`] error. The engine finds it in
/// the error text of a failed action.
pub const SCOPE_REFUSAL: &str = "Gmail refused the change: Iris has read access only. \
     Run `iris account add <client.json> --write`.";

impl Error {
    /// True for the Gmail answer to a call that the granted scopes do not
    /// allow: HTTP 403 `insufficientPermissions` (or the newer
    /// `ACCESS_TOKEN_SCOPE_INSUFFICIENT`). Gmail made no change (spec
    /// `docs/specs/2026-11-18-gmail-write-access.md`).
    pub fn is_scope_refusal(&self) -> bool {
        matches!(self, Error::Api { status: 403, reason, .. }
            if reason == "insufficientPermissions" || reason == "ACCESS_TOKEN_SCOPE_INSUFFICIENT")
    }

    /// For a write call: true if the provider can have done the call
    /// (spec 2 section 7.2). A timeout, a lost connection and HTTP 5xx give
    /// an unknown result. A rate limit of a call with one request also
    /// does: the engine never reads it as a rejection (spec
    /// `docs/specs/2026-11-16-send.md`, section 5.2).
    pub fn write_result_unknown(&self) -> bool {
        match self {
            Error::Transport(_) => true,
            Error::Api { status, reason, .. } => {
                *status >= 500
                    || *status == 429
                    || (*status == 403
                        && (reason == "rateLimitExceeded" || reason == "userRateLimitExceeded"))
            }
            _ => false,
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Unauthorized => write!(f, "the mail provider refused the access token"),
            Error::NotFound => write!(f, "the mail provider does not have the item"),
            Error::HistoryExpired => write!(f, "the sync cursor is too old"),
            e @ Error::Api { .. } if e.is_scope_refusal() => f.write_str(SCOPE_REFUSAL),
            Error::Api {
                status,
                reason,
                message,
            } => write!(f, "mail provider error {status} {reason}: {message}"),
            Error::Transport(e) => write!(f, "network error: {e}"),
            Error::Decode(e) => write!(f, "bad answer from the mail provider: {e}"),
            Error::Sink(e) => write!(f, "cannot write the raw message: {e}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// The mailbox of the secret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profile {
    pub email_address: String,
    /// 0 if the provider does not count.
    pub messages_total: u64,
    pub threads_total: u64,
    /// The cursor of now (spec, section 3).
    pub cursor: String,
}

// The serde names are those of the Gmail JSON. Other providers build the
// types in code.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Label {
    pub id: String,
    pub name: String,
    /// `system` or `user`.
    #[serde(rename = "type", default)]
    pub kind: String,
    /// The counts come only from a read of one label.
    pub messages_total: Option<u64>,
    pub messages_unread: Option<u64>,
    pub threads_total: Option<u64>,
    pub threads_unread: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MessageRef {
    pub id: String,
    pub thread_id: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MessagePage {
    #[serde(default)]
    pub messages: Vec<MessageRef>,
    pub next_page_token: Option<String>,
}

/// Only the ID and the labels of a message.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Minimal {
    pub id: String,
    #[serde(default)]
    pub label_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Header {
    pub name: String,
    pub value: String,
}

/// The ID, the labels and the headers of a message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Metadata {
    pub id: String,
    pub label_ids: Vec<String>,
    pub headers: Vec<Header>,
}

/// The fields of a raw read. The data went to the sink.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawMessage {
    pub id: String,
    pub thread_id: String,
    pub label_ids: Vec<String>,
    /// Milliseconds since 1970-01-01T00:00:00Z.
    pub internal_date: i64,
    /// The count of decoded bytes that went to the sink.
    pub size: u64,
}

/// A message in a change record.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryMessage {
    pub id: String,
    pub thread_id: String,
    #[serde(default)]
    pub label_ids: Vec<String>,
}

/// One change in a change record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    Added(HistoryMessage),
    Deleted(HistoryMessage),
    LabelsAdded(HistoryMessage, Vec<String>),
    LabelsRemoved(HistoryMessage, Vec<String>),
}

/// One change record. Apply the records in the order of the page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct History {
    pub changes: Vec<Change>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryPage {
    pub history: Vec<History>,
    /// The cursor after this page. Store it after the last page.
    pub cursor: String,
    pub next_page_token: Option<String>,
}

/// The answer of a send.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Sent {
    pub id: String,
    pub thread_id: String,
    #[serde(default)]
    pub label_ids: Vec<String>,
}

/// A draft ID and its message.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Draft {
    pub id: String,
    pub message: MessageRef,
}

/// A draft with the RFC 5322 text of its message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftRaw {
    pub id: String,
    pub message: MessageRef,
    /// The message, or `None` if it is larger than the limit of the provider.
    pub raw: Option<Vec<u8>>,
}

/// One mail provider. `token` is the secret of the call: the provider does
/// not store it. A write call (`modify`, `trash`, `untrash`, `create_label`,
/// `create_draft`, `send_draft`) is made one time: see
/// [`Error::write_result_unknown`] and spec section 6.
pub trait Provider: Send + Sync {
    /// The name of the provider in a result that the person reads, for
    /// example "the draft is gone at Gmail".
    fn name(&self) -> &'static str {
        "the mail server"
    }

    /// True if the server replaces the `Message-ID` of each new draft with
    /// its own (Gmail, send spec assumption 3). Then the `draft` row takes
    /// the ID of the server. Else another ID at the server means that
    /// another client changed the draft.
    fn sets_draft_message_id(&self) -> bool {
        false
    }

    /// True if [`Provider::update_draft`] replaces the MIME bytes of a draft
    /// in place, so that the read back of the draft checks the change
    /// (Gmail). Else a change of a draft makes a new draft
    /// (`docs/specs/2026-11-25-draft-change.md`, "Not in scope").
    fn updates_drafts(&self) -> bool {
        false
    }

    /// Replaces the message of the draft `id` with `raw`, in the thread
    /// `thread` for a reply. One request: no retry after an unknown result.
    /// A provider without [`Provider::updates_drafts`] refuses the call and
    /// sends nothing.
    fn update_draft(&self, token: &str, id: &str, raw: &[u8], thread: Option<&str>) -> Result<Draft> {
        let _ = (token, id, raw, thread);
        Err(Error::Api {
            status: 400,
            reason: "unsupported".into(),
            message: "this provider cannot change a draft in place".into(),
        })
    }

    /// True if a change of `INBOX` moves the message to another folder, so
    /// that its folder label changes too (Outlook). Then the apply reads
    /// the labels back after the write. A label provider (Gmail) and a
    /// provider that refuses each move (IMAP) keep the default.
    fn moves_folders(&self) -> bool {
        false
    }

    /// True if the draft stays on the server after `send_draft` (Outlook
    /// sends the bytes with `sendMail`, and Iris never deletes a draft).
    /// Then the completed send tells the person to delete the draft there
    /// (limit #634). Gmail sends the draft itself, so the draft is gone.
    fn keeps_sent_draft(&self) -> bool {
        false
    }

    /// Iris confirmed that the sent copy `id` holds the approved content
    /// (send spec, section 6). A provider that gives `SENT` only to the
    /// copies that Iris sent (Outlook) records it here. The default does
    /// nothing: Gmail gives `SENT` itself.
    fn sent_confirmed(&self, id: &str) {
        let _ = id;
    }

    /// The mailbox of `token` and the cursor of now.
    fn profile(&self, token: &str) -> Result<Profile>;
    /// Each label of the mailbox.
    fn labels(&self, token: &str) -> Result<Vec<Label>>;
    /// One page of all mail, spam and trash included, newest first. With
    /// `label`, only the messages with that label.
    fn messages(&self, token: &str, label: Option<&str>, page: Option<&str>)
        -> Result<MessagePage>;
    /// One page of the change records after the cursor `from`.
    /// [`Error::HistoryExpired`] means: do a full resync.
    fn changes(&self, token: &str, from: &str, page: Option<&str>) -> Result<HistoryPage>;
    fn minimal(&self, token: &str, id: &str) -> Result<Minimal>;
    /// An empty `headers` gives each header.
    fn metadata(&self, token: &str, id: &str, headers: &[&str]) -> Result<Metadata>;
    /// Streams the RFC 5322 data into `sink`. If the call fails after the
    /// first byte, the sink has a part of the message: discard it.
    fn raw(&self, token: &str, id: &str, sink: &mut dyn Write) -> Result<RawMessage>;
    /// The messages whose `Message-ID` header is `message_id` (one page).
    fn messages_with_id(&self, token: &str, message_id: &str) -> Result<Vec<MessageRef>>;
    /// Adds `add` to and removes `remove` from each message of `ids`, at
    /// most [`BATCH_LIMIT`] IDs.
    fn modify(&self, token: &str, ids: &[&str], add: &[&str], remove: &[&str]) -> Result<()>;
    /// Gives the labels of the message after the call.
    fn trash(&self, token: &str, id: &str) -> Result<Minimal>;
    fn untrash(&self, token: &str, id: &str) -> Result<Minimal>;
    fn create_label(&self, token: &str, name: &str) -> Result<Label>;
    /// Each draft whose `Message-ID` header is `message_id`.
    fn drafts_with_id(&self, token: &str, message_id: &str) -> Result<Vec<Draft>>;
    /// [`Error::NotFound`]: the draft is gone, deleted or sent. Streams the
    /// draft: `raw` is `None` above [`DRAFT_LIMIT`].
    fn draft_raw(&self, token: &str, id: &str) -> Result<DraftRaw>;
    /// A draft of `raw`, in the thread `thread` for a reply.
    fn create_draft(&self, token: &str, raw: &[u8], thread: Option<&str>) -> Result<Draft>;
    /// A sent copy of the send of draft `draft`: a message with the
    /// `Message-ID` `message_id` that the send made, never the draft. The
    /// send recovery uses it (send spec, section 6). The default is the
    /// first message with that ID and the label `SENT` (Gmail). A failed
    /// read is an error, never "no copy".
    fn sent_copy(&self, token: &str, message_id: &str, draft: &str) -> Result<Option<MessageRef>> {
        let _ = draft;
        for m in self.messages_with_id(token, message_id)? {
            match self.minimal(token, &m.id) {
                Ok(l) if l.label_ids.iter().any(|l| l == "SENT") => return Ok(Some(m)),
                Ok(_) | Err(Error::NotFound) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(None)
    }
    /// Sends the draft `id` with the bytes `raw`.
    fn send_draft(&self, token: &str, id: &str, raw: &[u8], thread: Option<&str>) -> Result<Sent>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scope_refusal_is_a_rejection_with_the_fix() {
        let api = |status: u16, reason: &str| Error::Api {
            status,
            reason: reason.into(),
            message: "Request had insufficient authentication scopes.".into(),
        };
        for e in [
            api(403, "insufficientPermissions"),
            api(403, "ACCESS_TOKEN_SCOPE_INSUFFICIENT"),
        ] {
            assert!(e.is_scope_refusal());
            assert!(!e.write_result_unknown(), "Gmail made no change");
            let text = e.to_string();
            assert!(
                text.contains("iris account add <client.json> --write"),
                "{text}"
            );
            assert!(!text.contains("insufficient"), "no raw text: {text}");
        }
        for e in [
            api(403, "forbidden"),
            api(401, "insufficientPermissions"),
            api(403, ""),
        ] {
            assert!(!e.is_scope_refusal(), "{e}");
            assert!(e.to_string().starts_with("mail provider error"), "{e}");
        }
    }

    #[test]
    fn only_unknown_results_of_a_write_say_so() {
        let api = |status: u16, reason: &str| Error::Api {
            status,
            reason: reason.into(),
            message: String::new(),
        };
        assert!(Error::Transport("cut".into()).write_result_unknown());
        assert!(api(503, "").write_result_unknown());
        assert!(api(429, "").write_result_unknown());
        assert!(api(403, "rateLimitExceeded").write_result_unknown());
        assert!(!api(403, "forbidden").write_result_unknown());
        assert!(!api(400, "").write_result_unknown());
        assert!(!Error::NotFound.write_result_unknown());
        assert!(!Error::Unauthorized.write_result_unknown());
    }
}
