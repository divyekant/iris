//! `POST /api/ask` (card R5-09): a question of the person about their mail,
//! answered by `iris_understand::ask` with the messages that the answer
//! comes from.

use std::collections::HashMap;
use std::sync::Arc;

use iris_store::Store;
use iris_understand::ask;
use iris_understand::budget::ModelRow;
use iris_understand::Model;
use serde::Serialize;

use crate::read::{is_sent, message_row, sender_kind, MessageSummary};
use crate::{Error, Result};

/// Makes the model client of a `model_setting` row. The program builds an
/// `iris-model` client; tests give a fake.
pub type Connect =
    Arc<dyn Fn(&ModelRow) -> std::result::Result<Box<dyn Model>, String> + Send + Sync>;

#[derive(Debug, Serialize, PartialEq)]
pub struct AskSource {
    /// The number of the marks `[n]` of the answer that name this source.
    pub n: usize,
    /// Words of the message that support the answer. The engine checked
    /// that the message has them.
    pub quote: Option<String>,
    pub message: MessageSummary,
    /// True if the sender of the message is a stranger (specification 1,
    /// section 6.7): its text can try to mislead the answer (issue #255).
    pub stranger: bool,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct AskAnswer {
    /// The answer with one or more marks `[n]` in each sentence, or `null` when the mail
    /// does not answer the question.
    pub answer: Option<String>,
    pub sources: Vec<AskSource>,
    /// The count of messages that the model read. 0: no message matched,
    /// and no call went out.
    pub read: usize,
    /// The cost of the call, or `null` (no call, or a local model).
    pub cost_micro_usd: Option<i64>,
}

/// Answers `question`. Blocking: the model call can take up to the time-out
/// of the model row.
///
/// The mail can change during the call. Each source is checked again against
/// the rules of the search, and for a change of its content ([`ask::current`]),
/// in the same snapshot as its
/// summary. A claim with a source that fails the check goes, so the answer
/// shows only mail that Ask can read now.
pub fn answer(store: &Store, connect: &Connect, question: &str, now_ms: i64) -> Result<AskAnswer> {
    let mut a = ask::ask(store, &|row| connect(row), question, now_ms).map_err(|e| match e {
        ask::Error::Question(why) => Error::Query(why),
        ask::Error::Store(e) => Error::Store(e),
        e @ (ask::Error::NoModel | ask::Error::Budget | ask::Error::Busy) => {
            Error::Unavailable(e.to_string())
        }
        ask::Error::Model(why) => Error::Model(why),
    })?;
    let sources = store.read(|c| {
        // One snapshot for the check and the summaries.
        let tx = c.unchecked_transaction()?;
        let mut rows = HashMap::new();
        for s in &a.sources {
            if ask::current(&tx, s)? {
                if let Some((m, _)) = message_row(&tx, s.message)? {
                    // Sent mail is from the owner (spec 1 section 6.7).
                    let stranger = !is_sent(&tx, s.message)?
                        && sender_kind(&tx, m.from.address.as_deref())? == "stranger";
                    rows.insert(s.message, (m, stranger));
                }
            }
        }
        a.retain(|s| rows.contains_key(&s.message));
        Ok(a.sources
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                let (message, stranger) = rows.remove(&s.message)?;
                Some(AskSource {
                    n: i + 1,
                    quote: s.quote.clone(),
                    message,
                    stranger,
                })
            })
            .collect())
    })?;
    Ok(AskAnswer {
        answer: a.text(),
        sources,
        read: a.read,
        cost_micro_usd: a.cost_micro_usd,
    })
}
