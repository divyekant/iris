//! The read verbs (tracker item R1-23): `iris status`, `iris search`,
//! `iris show` and `iris list`. Each verb calls the same `iris_api` function as the HTTP
//! route, so the verb and the client get the same data. `--json` prints the
//! JSON of the route.

use std::fmt::Write as _;

use iris_store::Store;

/// The text that a verb prints, or the error text.
pub type Output = Result<String, String>;

pub fn status(store: &Store, as_json: bool) -> Output {
    let s = iris_api::status(store).map_err(|e| e.to_string())?;
    if as_json {
        return to_json(&s);
    }
    let mut out = String::new();
    if s.accounts.is_empty() {
        out.push_str("No account. Run `iris account add <client.json>`.\n");
    }
    for a in &s.accounts {
        let state = a.state.as_deref().unwrap_or("-");
        let mirror = match (&a.mirror_ready_at, &a.sync) {
            (_, Some(sync)) => format!(
                "{} sync, step {}",
                sync.kind,
                sync.step.map_or("-".into(), |s| s.to_string())
            ),
            (Some(at), None) => format!("mirror ready since {at}"),
            (None, None) => "first sync not started".into(),
        };
        let _ = writeln!(
            out,
            "account {} {} ({}): {state}, {mirror}",
            a.id,
            field(&a.address),
            a.provider
        );
        if a.state.as_deref() == Some(crate::engine::STATE_READ_ONLY) {
            out.push_str(
                "  read only: approved changes wait. Run `iris account add <client.json> --write`.\n",
            );
        }
        if let Some(run) = &a.last_sync {
            let _ = writeln!(
                out,
                "  last sync: {} at {}: {}",
                run.kind,
                run.started,
                field(run.result.as_deref().unwrap_or("running"))
            );
        }
    }
    let _ = writeln!(out, "Mode: {}", s.mode);
    let m = &s.messages;
    let _ = writeln!(
        out,
        "Messages: {} ({} fetched, {} understood, {} gone)",
        m.total, m.fetched, m.understood, m.gone
    );
    Ok(out)
}

pub fn search(store: &Store, text: &str, limit: u32, as_json: bool) -> Output {
    let found = iris_api::search(store, text, limit).map_err(|e| e.to_string())?;
    if as_json {
        // A list is JSON Lines (specification 2, section 8.3).
        let mut out = String::new();
        for m in &found {
            out.push_str(&serde_json::to_string(m).map_err(|e| e.to_string())?);
            out.push('\n');
        }
        return Ok(out);
    }
    let mut out = String::new();
    for m in &found {
        let _ = writeln!(out, "{}", summary_line(m));
    }
    if found.is_empty() {
        out.push_str("No messages.\n");
    }
    Ok(out)
}

pub fn show(store: &Store, id: &str, as_json: bool) -> Output {
    let not_found = || format!("no object has the ID {}", field(id));
    if id.starts_with("m-") {
        let m = iris_api::message(store, id)
            .map_err(|e| e.to_string())?
            .ok_or_else(not_found)?;
        if as_json {
            return to_json(&m);
        }
        let mut out = String::new();
        let s = &m.summary;
        let _ = writeln!(out, "ID:      {} (thread {})", s.id, s.thread);
        let _ = writeln!(out, "Date:    {}", s.date.as_deref().unwrap_or("-"));
        let _ = writeln!(out, "From:    {}", address(&s.from));
        let _ = writeln!(out, "To:      {}", field(m.to.as_deref().unwrap_or("-")));
        if let Some(cc) = &m.cc {
            let _ = writeln!(out, "Cc:      {}", field(cc));
        }
        let _ = writeln!(
            out,
            "Subject: {}",
            field(s.subject.as_deref().unwrap_or("-"))
        );
        let _ = writeln!(out, "Labels:  {}", field(&s.labels.join(", ")));
        if m.gone {
            out.push_str("Gone:    yes\n");
        }
        if !m.hostile.is_empty() {
            let _ = writeln!(out, "Hostile: {}", m.hostile.join(", "));
        }
        if !m.items.is_empty() {
            let _ = writeln!(out, "Items:   {}", m.items.join(", "));
        }
        out.push('\n');
        match &m.text {
            Some(t) => {
                out.push_str(&clean(t));
                out.push('\n');
            }
            None => out.push_str("(No clean text yet.)\n"),
        }
        return Ok(out);
    }
    let item = iris_api::item(store, id)
        .map_err(|e| e.to_string())?
        .ok_or_else(not_found)?;
    // An item is a form document: JSON is its only format.
    to_json(&item)
}

/// `iris watch add|list|remove` (card R5-07).
pub fn watch(store: &Store, command: &crate::WatchCommand) -> Output {
    let err = |e: iris_api::Error| e.to_string();
    match command {
        crate::WatchCommand::Add { text } => {
            let id = iris_api::add_watch(store, text, crate::engine::now_ms()).map_err(err)?;
            Ok(format!(
                "{id}: Iris tells you one time when a new mail matches. \
                 Check with `iris watch list`.\n"
            ))
        }
        crate::WatchCommand::List { json } => {
            let all = iris_api::watches(store).map_err(err)?;
            let mut out = String::new();
            for w in &all {
                if *json {
                    out.push_str(&serde_json::to_string(w).map_err(|e| e.to_string())?);
                    out.push('\n');
                    continue;
                }
                let state = match (&w.message, &w.arrived) {
                    (Some(m), _) => format!("arrived  {}", summary_line(m)),
                    (None, Some(at)) => format!("arrived  {at}, the mail is gone"),
                    (None, None) => format!("waits    since {}", w.created),
                };
                let _ = writeln!(out, "{}  {state}\n    watch: {}", w.id, field(&w.text));
            }
            if all.is_empty() && !json {
                out.push_str("No watches.\n");
            }
            Ok(out)
        }
        crate::WatchCommand::Remove { id } => {
            if iris_api::remove_watch(store, id).map_err(err)? {
                Ok(format!("{} removed.\n", field(id)))
            } else {
                Err(format!("no watch has the ID {}", field(id)))
            }
        }
    }
}

/// `iris list` in host mode (issue #523): the items of `iris_api::items`,
/// the same page as `GET /api/items`. A list is JSON Lines (specification
/// 2, section 8.3). The text form names the command of the next page.
pub fn list(store: &Store, filter: &iris_api::Filter, limit: u32, as_json: bool) -> Output {
    let page = iris_api::items(store, filter, limit).map_err(|e| e.to_string())?;
    let mut out = String::new();
    for i in &page.items {
        if as_json {
            out.push_str(&serde_json::to_string(i).map_err(|e| e.to_string())?);
            out.push('\n');
        } else {
            let s = |k: &str| field(i[k].as_str().unwrap_or("-"));
            let _ = writeln!(
                out,
                "{}  {}  {}  {}",
                s("id"),
                s("form"),
                s("status"),
                s("title")
            );
        }
    }
    if as_json {
        return Ok(out);
    }
    if page.items.is_empty() {
        out.push_str("No items.\n");
    }
    if let Some(next) = &page.next {
        let _ = writeln!(out, "More: add --before {}", field(next));
    }
    Ok(out)
}

fn summary_line(m: &iris_api::MessageSummary) -> String {
    format!(
        "{}  {}  {}  {}",
        m.id,
        m.date.as_deref().map_or("-", |d| d.get(..10).unwrap_or(d)),
        address(&m.from),
        field(m.subject.as_deref().unwrap_or("-"))
    )
}

fn address(a: &iris_api::Address) -> String {
    match (&a.name, &a.address) {
        (Some(n), Some(x)) => field(&format!("{n} <{x}>")),
        (None, Some(x)) => field(x),
        (Some(n), None) => field(n),
        (None, None) => "-".into(),
    }
}

fn to_json(value: &impl serde::Serialize) -> Output {
    serde_json::to_string_pretty(value)
        .map(|s| s + "\n")
        .map_err(|e| e.to_string())
}

/// Mail text is data: remove each control character except a line break and
/// a tab, so a message cannot send escape sequences to the terminal.
pub fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect()
}

/// A header or a summary field must stay on its line: change each line
/// break or other white space to a space, then remove control characters.
pub fn field(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .filter(|c| !c.is_control())
        .collect()
}

/// `iris matters` and `iris matters show <id>`: the JSON of the routes
/// `/api/matters` and `/api/matters/{id}`. They write nothing.
pub fn matters(store: &Store, command: &crate::MattersCommand) -> Output {
    let err = |e: iris_api::Error| e.to_string();
    let v = match command {
        crate::MattersCommand::Show { id } => iris_api::matter(store, id)
            .map_err(err)?
            .ok_or_else(|| format!("no matter {id}, or a proposal that is not valid now"))?,
        _ => iris_api::matters(store).map_err(err)?,
    };
    serde_json::to_string_pretty(&v).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn clean_removes_terminal_escapes() {
        assert_eq!(
            super::clean("a\x1b]0;x\x07b\u{9b}2Jc\nd\te"),
            "a]0;xb2Jc\nd\te"
        );
    }

    #[test]
    fn a_line_break_in_a_header_stays_on_the_line_of_its_message() {
        let m = iris_api::MessageSummary {
            id: "m-1".into(),
            thread: "t-1".into(),
            date: Some("2026-11-16T00:00:00Z".into()),
            from: iris_api::Address {
                name: Some("Dana\nm-2  2026-11-16".into()),
                address: Some("dana@example.test".into()),
            },
            subject: Some("Hello\r\nm-3  fake\u{2028}line\tx".into()),
            labels: vec![],
        };
        let line = super::summary_line(&m);
        assert_eq!(line.lines().count(), 1, "{line:?}");
        assert!(line.starts_with("m-1  "), "{line:?}");
        assert_eq!(
            super::field("a\nb\r\tc\u{2028}d\u{2029}e\x1b"),
            "a b  c d e"
        );
    }
}
