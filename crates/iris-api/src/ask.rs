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

use crate::read::{message_row, MessageSummary};
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
/// the rules of the search ([`ask::eligible`]) in the same snapshot as its
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
            if ask::eligible(&tx, s.message)? {
                if let Some((m, _)) = message_row(&tx, s.message)? {
                    rows.insert(s.message, m);
                }
            }
        }
        a.retain(|s| rows.contains_key(&s.message));
        Ok(a.sources
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                Some(AskSource {
                    n: i + 1,
                    quote: s.quote.clone(),
                    message: rows.remove(&s.message)?,
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
