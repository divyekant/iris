//! Evaluation of the policy (spec 2, section 6.4) and the hard limits that
//! the engine checks before it (section 6.5).

use crate::{
    Author, CommitmentKind, Effect, Form, Policy, RecordType, Rule, SenderKind, Verb, When,
};

/// One item of a message: the values that `form`, `record_type` and
/// `commitment_kind` match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Item {
    pub form: Form,
    pub record_type: Option<RecordType>,
    pub commitment_kind: Option<CommitmentKind>,
}

/// One message of an action, or the origin message of an item.
#[derive(Clone, Copy, Debug)]
pub struct Message<'a> {
    /// The address of the sender. Empty when the message has none.
    pub sender: &'a str,
    pub sender_kind: SenderKind,
    /// True when the message has one or more hostile marks (spec 1, section 6.7).
    pub hostile: bool,
    /// The names of the labels on the message.
    pub labels: &'a [String],
    /// The date of the message, in seconds since 1970-01-01 UTC.
    pub date: i64,
    pub items: &'a [Item],
}

impl Message<'_> {
    /// A message with a hostile mark matches only `hostile`.
    fn kind(&self) -> SenderKind {
        if self.hostile {
            SenderKind::Hostile
        } else {
            self.sender_kind
        }
    }
}

/// The parts of an action that the policy reads, other than its messages.
#[derive(Clone, Copy, Debug)]
pub struct Action<'a> {
    pub verb: Verb,
    pub author: Author,
    /// The job of the changeset, if it has one.
    pub job: Option<&'a str>,
    /// True when the action has the flag `bulk` (spec 2, section 5.3).
    pub bulk: bool,
    /// The time of the evaluation, in seconds since 1970-01-01 UTC.
    pub now: i64,
}

/// The result for one action, from the least strict to the most strict.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Outcome {
    /// `approved`, with `decision.by` `rule`.
    Allow,
    /// `in_review`.
    Review,
    /// `denied`.
    Deny,
}

/// The decision for one action, with the reason that the brief, the review
/// and the ledger show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    pub outcome: Outcome,
    /// The rule that gave the outcome. `None` for a hard limit and for the
    /// default review.
    pub rule: Option<u32>,
    pub reason: String,
}

/// What the brief does with one item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Attention {
    Shown,
    Hidden { rule: u32 },
    Bundled { rule: u32 },
}

/// How `form`, `record_type` and `commitment_kind` match the items of a
/// message (spec 2, section 6.2).
#[derive(Clone, Copy)]
enum Items<'a> {
    /// `allow`: the message has items, and each item has the value.
    Each,
    /// `deny` and `review`: one or more items have the value.
    Some,
    /// Attention: the item itself has the value.
    This(&'a Item),
}

impl Policy {
    /// Decides one action. The engine calls this in the mode `observe` too,
    /// to record what the policy would do; the caller keeps the action
    /// `proposed` in that mode.
    ///
    /// `messages` are the messages of the action. They can come page by page.
    pub fn decide<'m>(
        &self,
        action: &Action,
        messages: impl IntoIterator<Item = Message<'m>>,
    ) -> Decision {
        let candidates: Vec<&Rule> = self
            .rules
            .iter()
            .filter(|r| {
                r.effect.is_action()
                    && r.verbs.contains(&action.verb)
                    && r.when.author.is_none_or(|a| a == action.author)
                    && r.job.as_deref().is_none_or(|j| action.job == Some(j))
            })
            .collect();
        // For allow: true while each message matched. Else: true when one did.
        let mut matched: Vec<bool> = candidates
            .iter()
            .map(|r| r.effect == Effect::Allow)
            .collect();
        let mut count = 0usize;
        let mut hostile = false;
        for msg in messages {
            count += 1;
            hostile |= msg.kind() == SenderKind::Hostile;
            for (rule, m) in candidates.iter().zip(matched.iter_mut()) {
                if rule.effect == Effect::Allow {
                    *m = *m && matches(&rule.when, &msg, action.now, Items::Each);
                } else {
                    *m = *m || matches(&rule.when, &msg, action.now, Items::Some);
                }
            }
        }

        let verb = action.verb.name();
        let hard = if action.author != Author::Person
            && hostile
            && !matches!(action.verb, Verb::Archive | Verb::Trash | Verb::Label)
        {
            Some(Decision {
                outcome: Outcome::Deny,
                rule: None,
                reason: format!(
                    "The target has hostile mail. Only a person can {verb} it; other authors can only archive, trash or label it."
                ),
            })
        } else if action.bulk {
            Some(Decision {
                outcome: Outcome::Review,
                rule: None,
                reason: "The action has the flag bulk (more than 500 messages), so a person must review it."
                    .to_string(),
            })
        } else {
            None
        };

        // An allow rule matches each message only when there is a message.
        let hit = |effect: Effect| {
            candidates
                .iter()
                .zip(&matched)
                .filter(|(r, &m)| r.effect == effect && m && (effect != Effect::Allow || count > 0))
                .map(|(r, _)| *r)
                .min_by_key(|r| r.id)
        };
        let by_rule = if let Some(r) = hit(Effect::Deny) {
            Decision {
                outcome: Outcome::Deny,
                rule: Some(r.id),
                reason: format!("Rule {} ({}) denies {verb}.", r.id, r.name),
            }
        } else if let Some(r) = hit(Effect::Review) {
            Decision {
                outcome: Outcome::Review,
                rule: Some(r.id),
                reason: format!("Rule {} ({}) sends {verb} to review.", r.id, r.name),
            }
        } else if let Some(r) = hit(Effect::Allow) {
            Decision {
                outcome: Outcome::Allow,
                rule: Some(r.id),
                reason: format!("Rule {} ({}) allows {verb}.", r.id, r.name),
            }
        } else {
            Decision {
                outcome: Outcome::Review,
                rule: None,
                reason: format!("No rule allows {verb} for these messages."),
            }
        };

        // A rule can make a hard limit stricter, never less strict.
        match hard {
            Some(h) if h.outcome >= by_rule.outcome => h,
            _ => by_rule,
        }
    }

    /// The attention of one item: the `hide` or `bundle` rule with the lowest
    /// `id` that matches, else shown. `msg` is the origin message of the item.
    pub fn attention(&self, item: &Item, msg: &Message, now: i64) -> Attention {
        self.rules
            .iter()
            .filter(|r| matches!(r.effect, Effect::Hide | Effect::Bundle))
            .filter(|r| matches(&r.when, msg, now, Items::This(item)))
            .min_by_key(|r| r.id)
            .map_or(Attention::Shown, |r| match r.effect {
                Effect::Hide => Attention::Hidden { rule: r.id },
                _ => Attention::Bundled { rule: r.id },
            })
    }
}

impl Policy {
    /// The `notify` rule with the lowest `id` that matches the message, with
    /// the reading of a `review` rule (spec 2, section 6.4). Hostile mail
    /// (section 6.5) and mail from you give no notice.
    pub fn notice(&self, msg: &Message, now: i64) -> Option<u32> {
        if matches!(msg.kind(), SenderKind::Hostile | SenderKind::SelfOwner) {
            return None;
        }
        self.rules
            .iter()
            .filter(|r| r.effect == Effect::Notify)
            .filter(|r| matches(&r.when, msg, now, Items::Some))
            .map(|r| r.id)
            .min()
    }
}

/// True when each condition of `when` matches the message. `author` is a
/// condition of the action, and the caller checks it.
fn matches(when: &When, msg: &Message, now: i64, items: Items) -> bool {
    fn item_value<T: PartialEq + Copy>(
        want: Option<T>,
        msg: &Message,
        items: Items,
        get: fn(&Item) -> Option<T>,
    ) -> bool {
        let Some(want) = want else { return true };
        match items {
            Items::Each => !msg.items.is_empty() && msg.items.iter().all(|i| get(i) == Some(want)),
            Items::Some => msg.items.iter().any(|i| get(i) == Some(want)),
            Items::This(i) => get(i) == Some(want),
        }
    }
    item_value(when.form, msg, items, |i| Some(i.form))
        && item_value(when.record_type, msg, items, |i| i.record_type)
        && item_value(when.commitment_kind, msg, items, |i| i.commitment_kind)
        && when
            .sender
            .as_deref()
            .is_none_or(|s| sender_matches(s, msg.sender))
        && when.sender_kind.is_none_or(|k| k == msg.kind())
        && when.label.as_deref().is_none_or(|l| {
            msg.labels
                .iter()
                .any(|m| m.to_lowercase() == l.to_lowercase())
        })
        && when
            .older_than_days
            .is_none_or(|d| now.saturating_sub(msg.date) > i64::from(d) * 86_400)
}

/// `rule` is a lower-case address, or `@` and a domain.
fn sender_matches(rule: &str, sender: &str) -> bool {
    let sender = sender.trim().to_lowercase();
    match rule.strip_prefix('@') {
        Some(domain) => sender
            .rsplit_once('@')
            .is_some_and(|(local, d)| !local.is_empty() && d == domain),
        None => sender == rule,
    }
}
