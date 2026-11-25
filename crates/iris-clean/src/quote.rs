//! Quoted history and signatures in plain text.

/// The ends of the "On ... wrote:" line in common clients and languages.
const WROTE: [&str; 17] = [
    "wrote:",
    "a écrit :",
    "a écrit:",
    "schrieb:",
    "escribió:",
    "scrisse:",
    "ha scritto:",
    "skrev:",
    "schreef:",
    "napisał:",
    "napisał(a):",
    "escreveu:",
    "napsal(a):",
    "kirjoitti:",
    "yazdı:",
    "написал(а):",
    "写道：",
];

/// The first words of an "On ... wrote:" line in common clients and languages.
const ATTRIBUTION_START: [&str; 9] = [
    "On ", "Le ", "Am ", "El ", "Il ", "Op ", "Den ", "W dniu ", "Em ",
];

/// The first words of the last line that a phone mail client adds.
const MOBILE: [&str; 4] = [
    "Sent from my ",
    "Get Outlook for ",
    "Sent from Mail for Windows",
    "Sent from Yahoo Mail",
];

/// Remove quoted history. The history starts at the first of these lines:
/// an "On ... wrote:" line, or (in a reply) an "-----Original Message-----"
/// line or an Outlook block of `From:`, `Sent:` and `Subject:` lines. In a
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
        // Outlook also puts the marker above a forwarded message, so the
        // marker is history only in a reply.
        if is_reply && (is_original_marker(t) || is_header_block(&lines[i..])) {
            end = i;
            break;
        }
        if let Some((n, dated)) = attribution(&lines[i..]) {
            let next = lines[i + n..].iter().find(|l| !l.trim().is_empty());
            // "Here is what I wrote:" is not history. A cut needs a quote
            // after the line, or (in a reply) the start or the form of a
            // client line.
            let quoted = next.is_some_and(|l| l.trim_start().starts_with('>'));
            let client = dated || ATTRIBUTION_START.iter().any(|p| t.starts_with(p));
            if quoted || (is_reply && client) {
                end = i;
                break;
            }
        }
    }
    let is_quote = |l: &str| l.trim_start().starts_with('>');
    let stop = end.min(forward);
    // Without reply headers, only a quote block at the end is history. Quote
    // lines between own lines can be a real quote, so they stay.
    let tail = lines[..stop]
        .iter()
        .rposition(|l| !l.trim().is_empty() && !is_quote(l))
        .map_or(stop, |p| p + 1);
    let mut kept: Vec<&str> = lines[..end]
        .iter()
        .enumerate()
        .filter(|(i, l)| !(*i < stop && is_quote(l) && (is_reply || *i >= tail)))
        .map(|(_, l)| *l)
        .collect();
    // A rule line or an empty line just before the history goes out too:
    // the history is a marker line or a quote block at the end. Without
    // history, the text keeps its last lines.
    let found = end < lines.len() || lines[tail..stop].iter().any(|l| is_quote(l));
    while found
        && kept
            .last()
            .is_some_and(|l| l.trim().chars().all(|c| "_-=*".contains(c)))
    {
        kept.pop();
    }
    kept.join("\n")
}

/// An "On ... wrote:" line at the start: the count of its lines (1 or 2),
/// and true if it is the form of a client in any language (see [`dated`]).
fn attribution(lines: &[&str]) -> Option<(usize, bool)> {
    let ends = |s: &str| {
        let l = s.trim().to_lowercase();
        (l.len() < 400 && WROTE.iter().any(|w| l.ends_with(w))) || dated(s)
    };
    if ends(lines[0]) {
        return Some((1, dated(lines[0])));
    }
    let first = lines[0].trim();
    let second = lines.get(1)?.trim();
    if first.len() + second.len() >= 400 {
        return None;
    }
    // Clients wrap a long "On ... wrote:" line.
    let joined = format!("{first} {second}");
    // A wrap of the client form: the first line ends with no sentence
    // mark and has no address, and the second line alone is not the form.
    let dated_wrap = dated(&joined)
        && !first.ends_with(['.', '!', '?'])
        && !first.contains('<')
        && !ends(second);
    let wrapped = ATTRIBUTION_START.iter().any(|p| first.starts_with(p)) || dated_wrap;
    (wrapped && !second.is_empty() && ends(&joined)).then_some((2, dated_wrap))
}

/// The form of the attribution line of a client in any language: a time
/// (`10:00`) and an address in `<>`, and a `:` at the end. Gmail writes
/// "Am 3. Feb. 2025 um 10:00 schrieb Ana <ana@example.org>:".
fn dated(s: &str) -> bool {
    let s = s.trim();
    if s.len() >= 400 || !(s.ends_with(':') || s.ends_with('：')) {
        return false;
    }
    let b = s.as_bytes();
    let time = b.windows(4).any(|w| {
        w[0].is_ascii_digit() && w[1] == b':' && w[2].is_ascii_digit() && w[3].is_ascii_digit()
    });
    let address = s.split('<').skip(1).any(|r| {
        r.split_once('>')
            .is_some_and(|(a, _)| a.contains('@') && !a.contains(char::is_whitespace))
    });
    time && address
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
    let starts = |l: &str, p: &[&str]| p.iter().any(|p| l.trim_start().starts_with(p));
    let after = |i: usize, n: usize| lines.iter().skip(i + 1).take(n).copied();
    starts(lines[0], &["From:"])
        && after(0, 4)
            .position(|l| starts(l, &["Sent:", "Date:"]))
            .is_some_and(|d| after(d + 1, 5).any(|l| starts(l, &["Subject:", "To:"])))
}

/// Remove the signature: the text after the last `-- ` line (RFC 3676), or
/// after a last `--` line with at most 10 lines after it. A last line from a
/// phone mail client ("Sent from my phone") goes out too.
pub fn strip_signature(text: &str) -> String {
    let mut lines: Vec<&str> = text.split('\n').collect();
    let bare = |l: &str| l.trim_end_matches([' ', '\u{a0}']) == "--";
    // The last separator that counts: a `-- ` line always, a bare `--` line
    // only with at most 10 lines after it.
    let n = lines.len();
    let counts = |(i, l): &(usize, &&str)| bare(l) && (**l != "--" || n - i <= 11);
    if let Some((i, _)) = lines.iter().enumerate().rfind(counts) {
        lines.truncate(i);
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
    fn a_long_bare_rule_after_a_signature_line() {
        let after = "x\n".repeat(11);
        let t = format!("Hi\n-- \nSignature\n--\n{after}");
        assert_eq!(strip_signature(&t), "Hi");
    }

    #[test]
    fn reply_history_goes_out() {
        let t = "Yes, Friday.\n\nOn Mon, 3 Feb 2025 at 10:00, Ana Bel <ana@example.org>\nwrote:\n> Can you?\n> Thanks";
        assert_eq!(strip_quotes(t, true), "Yes, Friday.");
        let t = "Fine.\n\n-----Original Message-----\nFrom: Ana\nold";
        assert_eq!(strip_quotes(t, true), "Fine.");
        // Outlook forwards with the same marker, and a forward stays.
        assert_eq!(strip_quotes(t, false), t);
        let t = "Fine.\n________________________________\nFrom: Ana Bel\nSent: Monday\nTo: Avel\nSubject: Plan\n\nold";
        assert_eq!(strip_quotes(t, true), "Fine.");
        // Inline answers stay, the quoted lines go.
        assert_eq!(strip_quotes("> Q1?\nA1\n> Q2?\nA2", true), "A1\nA2");
    }

    #[test]
    fn attribution_lines_of_other_languages() {
        let lines = [
            "Am Mo., 3. Feb. 2025 um 10:00 Uhr schrieb Ana Bel <ana@example.org>:",
            "Op ma 3 feb 2025 om 10:00 schreef Ana Bel <ana@example.org>:",
            "Il giorno lun 3 feb 2025 alle ore 10:00 Ana Bel <ana@example.org> ha scritto:",
            "pon., 3 lut 2025 o 10:00 Ana Bel <ana@example.org> napisał(a):",
            "Em seg., 3 de fev. de 2025 às 10:00, Ana Bel <ana@example.org> escreveu:",
            "пн, 3 февр. 2025 г. в 10:00, Ana Bel <ana@example.org>:",
            "2025年2月3日(月) 10:00 Ana Bel <ana@example.org>:",
            "Ana Bel <ana@example.org> 于2025年2月3日周一 10:00写道：",
        ];
        for l in lines {
            // With a quote after it, and in a reply without a quote.
            assert_eq!(
                strip_quotes(&format!("Ja.\n\n{l}\n> Frage?"), false),
                "Ja.",
                "{l}"
            );
            assert_eq!(
                strip_quotes(&format!("Ja.\n\n{l}\nFrage?"), true),
                "Ja.",
                "{l}"
            );
        }
        // Gmail wraps a long line.
        let t =
            "Ja.\n\nAm Mo., 3. Feb. 2025 um 10:00 Uhr schrieb Ana Bel\n<ana@example.org>:\nFrage?";
        assert_eq!(strip_quotes(t, true), "Ja.");
    }

    /// Random replies of known pieces: the quote step only takes lines out,
    /// and the own first line always stays.
    #[test]
    fn random_replies_keep_the_first_line_and_add_nothing() {
        const PIECES: [&str; 12] = [
            "Am 3. Feb. 2025 um 10:00 schrieb Ana <ana@example.org>:",
            "Ana <a@b> 10:00：",
            "<ana@example.org>:",
            "um 10:0",
            "0 <x@y>:",
            "> quoted",
            "On Mon, Ana",
            "wrote:",
            "",
            "---",
            "Ünïcödé 写道 ：",
            "From: Ana",
        ];
        let mut seed = 0x51E7_C0DE_2025_1125u64;
        let mut next = |n: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % n as u64) as usize
        };
        for _ in 0..20_000 {
            let mut lines = vec!["Keep this line."];
            for _ in 0..next(8) {
                lines.push(PIECES[next(PIECES.len())]);
            }
            let input = lines.join("\n");
            for reply in [false, true] {
                let out = strip_quotes(&input, reply);
                assert!(out.starts_with("Keep this line."), "{input:?}");
                let mut rest = lines.iter();
                for l in out.split('\n') {
                    assert!(rest.any(|x| *x == l), "{input:?} gave {out:?}");
                }
            }
        }
    }

    #[test]
    fn an_address_line_without_a_time_is_not_history() {
        let t = "Send it to Ana Bel <ana@example.org>:\nthe plan and the price.";
        assert_eq!(strip_quotes(t, true), t);
        // A time and an address, but not at the end.
        let t = "At 10:00 call Ana <ana@example.org>: then go.\nok";
        assert_eq!(strip_quotes(t, true), t);
    }

    #[test]
    fn a_line_that_ends_with_wrote_is_not_always_history() {
        let t = "Here is what I wrote:\nthe plan is good.";
        assert_eq!(strip_quotes(t, true), t);
    }

    #[test]
    fn not_a_reply_keeps_its_text() {
        let t = "As Ana said:\n\n> a real quote\n\nthe plan is good.";
        assert_eq!(strip_quotes(t, false), t);
        // A quote block at the end is history, also without reply headers.
        assert_eq!(
            strip_quotes("New answer\n\n> old question\n> more", false),
            "New answer"
        );
        assert_eq!(strip_quotes("> only a quote", false), "> only a quote");
        assert_eq!(strip_quotes("Plan\n---", true), "Plan\n---");
        // An inline quote is not history: the last rule line stays.
        assert_eq!(strip_quotes("A\n> quoted\n---", true), "A\n---");
        // A quote block at the end is history: the rule line above it goes.
        assert_eq!(strip_quotes("A\n---\n> quoted", true), "A");
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
