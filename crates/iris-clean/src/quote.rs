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
    // `inline_from[i]`: from line `i`, a quote line has an own line after it.
    let mut inline_from = vec![false; lines.len() + 1];
    let mut own_after = false;
    for (i, l) in lines.iter().enumerate().rev() {
        let quote = l.trim_start().starts_with('>');
        own_after |= !quote && !l.trim().is_empty();
        inline_from[i] = inline_from[i + 1] || (quote && own_after);
    }
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
        if let Some((n, verb)) = attribution(&lines[i..]) {
            let next = lines[i + n..].iter().find(|l| !l.trim().is_empty());
            // "Here is what I wrote:" is not history. A cut needs a quote
            // after the line, or (in a reply) the start of a client line.
            // The client form alone never cuts: own lines can have a time
            // and an address too.
            // An own line after a quote line makes an inline answer: the
            // attribution and the answers stay, also in a reply.
            let inline = inline_from[i + n];
            let quoted = next.is_some_and(|l| l.trim_start().starts_with('>'));
            let client = ATTRIBUTION_START.iter().any(|p| t.starts_with(p));
            if !inline && (quoted || (is_reply && client && verb)) {
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
/// and true if it ends with a verb of [`WROTE`] (false for the client form
/// of [`dated`] alone).
fn attribution(lines: &[&str]) -> Option<(usize, bool)> {
    let verb = |s: &str| {
        let l = s.trim().to_lowercase();
        l.len() < 400 && WROTE.iter().any(|w| l.ends_with(w))
    };
    if verb(lines[0]) || dated(lines[0]) {
        return Some((1, verb(lines[0])));
    }
    let first = lines[0].trim();
    let second = lines.get(1)?.trim();
    // Clients wrap a long "On ... wrote:" line. An own sentence never joins
    // the next line.
    if second.is_empty() || first.len() + second.len() >= 400 || first.ends_with(['.', '!', '?']) {
        return None;
    }
    let joined = format!("{first} {second}");
    if ATTRIBUTION_START.iter().any(|p| first.starts_with(p)) && verb(&joined) {
        return Some((2, true));
    }
    // A wrap of the client form: the second line alone is not the form.
    (dated(&joined) && !dated(second)).then_some((2, false))
}

/// The form of the attribution line of a client in any language: a year
/// (`2025`), a time (`10:00`) and an address in `<>`, and a `:` at the end. Gmail writes
/// "Am 3. Feb. 2025 um 10:00 schrieb Ana <ana@example.org>:".
fn dated(s: &str) -> bool {
    let s = s.trim();
    if s.len() >= 400 || !(s.ends_with(':') || s.ends_with('：')) {
        return false;
    }
    // The time and the year count only outside `<>`, so an address such as
    // `<ana2025@example.org>` is not a date.
    let outside: Vec<u8> = s
        .split('<')
        .enumerate()
        .flat_map(|(i, part)| {
            let text = if i == 0 {
                part
            } else {
                part.split_once('>').map_or("", |p| p.1)
            };
            text.bytes().chain(*b" ")
        })
        .collect();
    let b = &outside[..];
    let time = b.windows(4).any(|w| {
        w[0].is_ascii_digit() && w[1] == b':' && w[2].is_ascii_digit() && w[3].is_ascii_digit()
    });
    let address = s.split('<').skip(1).any(|r| {
        r.split_once('>')
            .is_some_and(|(a, _)| a.contains('@') && !a.contains(char::is_whitespace))
    });
    // Clients write the date with its year. An own line with a time and an
    // address seldom has a year too.
    let year = b.windows(4).enumerate().any(|(i, w)| {
        (w.starts_with(b"19") || w.starts_with(b"20"))
            && w.iter().all(u8::is_ascii_digit)
            && !b.get(i + 4).is_some_and(u8::is_ascii_digit)
            && !(i > 0 && b[i - 1].is_ascii_digit())
    });
    time && address && year
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
            assert_eq!(
                strip_quotes(&format!("Ja.\n\n{l}\n> Frage?"), false),
                "Ja.",
                "{l}"
            );
        }
        // A wrap of two lines, with the address on either line.
        for l in [
            "Am Mo., 3. Feb. 2025 um 10:00 Uhr schrieb Ana Bel\n<ana@example.org>:",
            "2025年2月3日(月) Ana <ana@example.org>\n10:00 写道：",
        ] {
            assert_eq!(
                strip_quotes(&format!("Ja.\n\n{l}\n> Frage?"), true),
                "Ja.",
                "{l}"
            );
        }
        // In a reply, a listed first word with a verb is enough.
        let t = "Ja.\n\nOn Mon, 3 Feb 2025 at 10:00, Ana Bel <ana@example.org> wrote:\nFrage?";
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
            // Own lines with a time and an address, with no quote after them.
            let own = [
                "Can we meet at 10:00",
                "with Ana <ana@example.org>:",
                "Bring it.",
            ];
            let head = lines.len();
            lines.extend(own);
            let input = lines.join("\n");
            for reply in [false, true] {
                let out = strip_quotes(&input, reply);
                assert!(out.starts_with("Keep this line."), "{input:?}");
                // No `>` line comes after the own lines, so they stay when
                // no history starts before them.
                let before = strip_quotes(&lines[..head].join("\n"), reply);
                if before.split('\n').count() == head {
                    assert!(out.ends_with(&own.join("\n")), "{input:?} gave {out:?}");
                }
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
        // Own lines with the client form, without a quote after them.
        let t = "Can we meet at 10:00\nwith Ana <ana@example.org>:\nPlease bring the contract.";
        assert_eq!(strip_quotes(t, true), t);
        let t = "Call at 10:00 with Ana <ana@example.org>:\nthe plan.";
        assert_eq!(strip_quotes(t, true), t);
        // A listed first word does not make the client form history.
        let t = "On Tuesday at 10:00 meet Ana <ana@example.org>:\nPlease discuss the budget.";
        assert_eq!(strip_quotes(t, true), t);
        // Own lines after an inline quote are an answer: they stay.
        let t = "Please review our call at 10:00 with Ana <ana@example.org>:\n> Keep the price under $50\nMy answer is yes.";
        assert_eq!(strip_quotes(t, false), t);
        assert_eq!(
            strip_quotes(t, true),
            "Please review our call at 10:00 with Ana <ana@example.org>:\nMy answer is yes."
        );
        let t = "On Mon Ana wrote:\n> Q1?\nA1\n> Q2?\nA2";
        assert_eq!(strip_quotes(t, true), "On Mon Ana wrote:\nA1\nA2");
        // An own line with a time and an address but no year, before a
        // quote at the end.
        let t = "Let's meet at 10:00 with Ana <ana@example.org>:\n> The proposed time works";
        assert_eq!(
            strip_quotes(t, false),
            "Let's meet at 10:00 with Ana <ana@example.org>:"
        );
        // A year or a time inside the address does not count.
        let t = "Let's meet at 10:00 with Ana <ana2025@example.org>:\n> The proposed time works";
        assert_eq!(strip_quotes(t, false), t.lines().next().unwrap());
        // An own sentence does not join the next line.
        let t = "On Tuesday at 10:00.\nAna <ana@example.org>:\n> example";
        assert_eq!(
            strip_quotes(t, true),
            "On Tuesday at 10:00.\nAna <ana@example.org>:"
        );
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
