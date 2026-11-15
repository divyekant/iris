//! Quoted history and signatures in plain text.

/// The ends of the "On ... wrote:" line in common clients and languages.
const WROTE: [&str; 9] = [
    "wrote:",
    "a écrit :",
    "a écrit:",
    "schrieb:",
    "escribió:",
    "scrisse:",
    "skrev:",
    "schreef:",
    "napisał:",
];

/// The first words of the last line that a phone mail client adds.
const MOBILE: [&str; 4] = [
    "Sent from my ",
    "Get Outlook for ",
    "Sent from Mail for Windows",
    "Sent from Yahoo Mail",
];

/// Remove quoted history. The history starts at the first of these lines:
/// an "On ... wrote:" line, an "-----Original Message-----" line, or (in a
/// reply) an Outlook block of `From:`, `Sent:` and `Subject:` lines. In a
/// reply, each line that starts with `>` goes out too. A forwarded message
/// stays: the search stops at a "Forwarded message" line.
pub fn strip_quotes(text: &str, is_reply: bool) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut end = lines.len();
    // The `>` lines of a forwarded message belong to that message, so they stay.
    let mut forward = lines.len();
    for i in 0..lines.len() {
        let t = lines[i].trim();
        if is_forward_marker(t) {
            forward = i;
            break;
        }
        if is_original_marker(t) || (is_reply && is_header_block(&lines[i..])) {
            end = i;
            break;
        }
        if let Some(n) = attribution(&lines[i..]) {
            let next = lines[i + n..].iter().find(|l| !l.trim().is_empty());
            if is_reply || next.is_some_and(|l| l.trim_start().starts_with('>')) {
                end = i;
                break;
            }
        }
    }
    let mut kept: Vec<&str> = lines[..end]
        .iter()
        .enumerate()
        .filter(|(i, l)| !(is_reply && *i < forward && l.trim_start().starts_with('>')))
        .map(|(_, l)| *l)
        .collect();
    // A rule line or an empty line before the history goes out too.
    while kept
        .last()
        .is_some_and(|l| l.trim().chars().all(|c| "_-=*".contains(c)))
    {
        kept.pop();
    }
    kept.join("\n")
}

/// The count of lines (1 or 2) of an "On ... wrote:" line at the start.
fn attribution(lines: &[&str]) -> Option<usize> {
    let ends = |s: &str| {
        let s = s.trim().to_lowercase();
        s.len() < 400 && WROTE.iter().any(|w| s.ends_with(w))
    };
    if ends(lines[0]) {
        return Some(1);
    }
    let first = lines[0].trim();
    let second = lines.get(1)?.trim();
    // Clients wrap a long "On ... wrote:" line.
    (first.starts_with("On ") && !second.is_empty() && ends(&format!("{first} {second}")))
        .then_some(2)
}

fn is_original_marker(t: &str) -> bool {
    let l = t.to_lowercase();
    t.starts_with("-----")
        && t.ends_with("-----")
        && (l.contains("original message") || l.contains("ursprüngliche nachricht"))
}

fn is_forward_marker(t: &str) -> bool {
    let l = t.to_lowercase();
    (t.starts_with("---") && l.contains("forwarded message")) || l == "begin forwarded message:"
}

/// An Outlook reply header: `From:`, then `Sent:` or `Date:`, then `Subject:` or `To:`.
fn is_header_block(lines: &[&str]) -> bool {
    let starts = |l: &&str, p: &[&str]| p.iter().any(|p| l.trim_start().starts_with(p));
    starts(&lines[0], &["From:"])
        && lines
            .iter()
            .skip(1)
            .take(4)
            .any(|l| starts(l, &["Sent:", "Date:"]))
        && lines
            .iter()
            .skip(1)
            .take(5)
            .any(|l| starts(l, &["Subject:", "To:"]))
}

/// Remove the signature: the text after the last `-- ` line (RFC 3676), or
/// after a last `--` line with at most 10 lines after it. A last line from a
/// phone mail client ("Sent from my phone") goes out too.
pub fn strip_signature(text: &str) -> String {
    let mut lines: Vec<&str> = text.split('\n').collect();
    let bare = |l: &str| l.trim_end_matches([' ', '\u{a0}']) == "--";
    if let Some(i) = lines.iter().rposition(|l| bare(l)) {
        if lines[i] != "--" || lines.len() - i <= 11 {
            lines.truncate(i);
        }
    }
    while lines.last().is_some_and(|l| {
        let t = l.trim();
        t.is_empty() || MOBILE.iter().any(|m| t.starts_with(m))
    }) {
        lines.pop();
    }
    lines.join("\n")
}

/// Remove the end block that `text` shares with an earlier message, for
/// example a signature without a `-- ` line. Each item of `earlier` is the
/// clean text of an earlier message from the same sender in the thread. The
/// block must have 2 or more lines that are not empty, and text must stay.
pub fn remove_repeated_end(text: &str, earlier: &[&str]) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut cut = 0;
    for e in earlier {
        let el: Vec<&str> = e.lines().filter(|l| !l.trim().is_empty()).collect();
        let (mut i, mut j, mut matched, mut start) = (lines.len(), el.len(), 0, lines.len());
        while i > 0 && j > 0 {
            let l = lines[i - 1].trim();
            if l.is_empty() {
                i -= 1;
                continue;
            }
            if l != el[j - 1].trim() {
                break;
            }
            matched += 1;
            i -= 1;
            j -= 1;
            start = i;
        }
        if matched >= 2 && lines[..start].iter().any(|l| !l.trim().is_empty()) {
            cut = cut.max(lines.len() - start);
        }
    }
    lines[..lines.len() - cut].join("\n").trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reply_history_goes_out() {
        let t = "Yes, Friday.\n\nOn Mon, 3 Feb 2025 at 10:00, Ana Bel <ana@example.org>\nwrote:\n> Can you?\n> Thanks";
        assert_eq!(strip_quotes(t, true), "Yes, Friday.");
        let t = "Fine.\n\n-----Original Message-----\nFrom: Ana\nold";
        assert_eq!(strip_quotes(t, false), "Fine.");
        let t = "Fine.\n________________________________\nFrom: Ana Bel\nSent: Monday\nTo: Avel\nSubject: Plan\n\nold";
        assert_eq!(strip_quotes(t, true), "Fine.");
        // Inline answers stay, the quoted lines go.
        assert_eq!(strip_quotes("> Q1?\nA1\n> Q2?\nA2", true), "A1\nA2");
    }

    #[test]
    fn not_a_reply_keeps_its_text() {
        let t = "Ana wrote:\nthe plan is good.\n> a real quote";
        assert_eq!(strip_quotes(t, false), t);
        let t = "FYI\n\n---------- Forwarded message ---------\nFrom: Ana\nDate: Mon\nSubject: Plan\nTo: Avel\n\nOn Sun Bo wrote:\n> x";
        assert_eq!(strip_quotes(t, true), t);
    }

    #[test]
    fn signatures_go_out() {
        assert_eq!(
            strip_signature("Hi\n-- \nAna\nCEO\n\nSent from my phone"),
            "Hi"
        );
        assert_eq!(strip_signature("Hi\n\nSent from my iPhone\n"), "Hi");
        let long = format!("a\n--\n{}", "x\n".repeat(20));
        assert_eq!(strip_signature(&long), long.trim_end());
    }

    #[test]
    fn repeated_end_blocks_go_out() {
        let earlier = "Old text\n\nAna Bel\nSales lead\n+1 555 0100";
        assert_eq!(
            remove_repeated_end("New text\n\nAna Bel\nSales lead\n+1 555 0100\n", &[earlier]),
            "New text"
        );
        // One line is not enough, and the whole text never goes.
        assert_eq!(remove_repeated_end("Ok\n\nAna", &["x\nAna"]), "Ok\n\nAna");
        assert_eq!(remove_repeated_end("a\nb", &["a\nb"]), "a\nb");
    }
}
