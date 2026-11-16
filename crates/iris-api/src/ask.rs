//! `POST /api/ask` (card R5-09): a question of the person about their mail,
//! answered by `iris_understand::ask` with the messages that the answer
//! comes from.

use std::sync::Arc;

use iris_store::Store;
use iris_understand::ask::{self, Answer};
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
    /// The answer with a mark `[n]` after each fact, or `null` when the mail
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
pub fn answer(store: &Store, connect: &Connect, question: &str, now_ms: i64) -> Result<AskAnswer> {
    let Answer {
        text,
        sources,
        read,
        cost_micro_usd,
    } = ask::ask(store, &|row| connect(row), question, now_ms).map_err(|e| match e {
        ask::Error::Question(why) => Error::Query(why),
        ask::Error::Store(e) => Error::Store(e),
        e @ (ask::Error::NoModel | ask::Error::Budget) => Error::Unavailable(e.to_string()),
        ask::Error::Model(why) => Error::Model(why),
    })?;
    let sources = store.read(|c| {
        let mut out = Vec::with_capacity(sources.len());
        for (i, s) in sources.into_iter().enumerate() {
            // A message row that went away after the answer: the mark stays
            // with no source, and the client shows it as text.
            if let Some((message, _)) = message_row(c, s.message)? {
                out.push(AskSource {
                    n: i + 1,
                    quote: s.quote,
                    message,
                });
            }
        }
        Ok(out)
    })?;
    Ok(AskAnswer {
        answer: text,
        sources,
        read,
        cost_micro_usd,
    })
}
