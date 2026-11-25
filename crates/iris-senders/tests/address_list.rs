//! Property tests of `head::each_address`, the parser of address lists. The
//! lists come from mail that the product does not control: the `To` and
//! `Cc` headers of received mail, and the `To` list that the reply form of
//! the write view reads from mail of the person (issue #500).

use iris_senders::head::addresses;

/// A small xorshift generator: the same seed gives the same cases.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len())]
    }
}

/// A random address with upper and lower case letters, digits and the
/// usual signs of a local part.
fn address(r: &mut Rng) -> String {
    const LOCAL: &[char] = &['a', 'B', 'c', 'D', 'e', '1', '7', '.', '+', '-', '_'];
    const DOMAINS: &[&str] = &[
        "example.com",
        "Example.ORG",
        "mail.example.net",
        "x.example",
    ];
    let n = 1 + r.below(10);
    let mut local: String = (0..n).map(|_| *r.pick(LOCAL)).collect();
    // A local part does not start or end with a dot.
    local = format!("a{local}z");
    // A quoted local part with the signs of list syntax (issue #980).
    if r.below(5) == 0 {
        local = format!("\"{local} <x>, (y);\"");
    }
    format!("{local}@{}", r.pick(DOMAINS))
}

/// One entry of a list in one of the forms of RFC 5322 that mail uses,
/// for the address `a`. Display names and comments hold the signs that
/// split a naive parser: commas, colons, semicolons, angle brackets and
/// parentheses.
fn entry(r: &mut Rng, a: &str) -> String {
    const NAMES: &[&str] = &[
        "Ana Lee",
        "Lee, Ana",
        "Bo: ops",
        "Sam; team",
        "A <b>",
        "Okafor (work)",
    ];
    let name = r.pick(NAMES);
    match r.below(10) {
        0 => a.to_string(),
        1 => format!("<{a}>"),
        2 => format!("\"{name}\" <{a}>"),
        3 => format!("Ana Lee <{a}>"),
        4 => format!("{a} ({name})"),
        5 => format!("(note, {name}) {a}"),
        6 => format!("\"Ana \\\"the\\\" Lee\" <{a}>"),
        // One quoted pair: a parser that ends the quote at `\"` splits
        // the name at its comma (issue #482).
        7 => format!("\"Ana \\\", x@evil.example\" <{a}>"),
        // A quoted pair in a comment: `\)` does not end the comment.
        8 => format!("{a} (x\\), <b@evil.example>, tail)"),
        // White space and a comment before the address in `<>`.
        9 if r.below(2) == 0 => format!("Ana < (note, <x>) {a}>"),
        _ => format!("=?UTF-8?Q?Ana_Lee?= <{a}>"),
    }
}

/// For random lists of all forms, with groups, empty entries and spaces,
/// the parser gives each address once, in order, in lower case, and no
/// other address.
#[test]
fn property_each_entry_gives_its_address() {
    let mut r = Rng(0x9e37_79b9_7f4a_7c15);
    for case in 0..2_000 {
        let n = r.below(6);
        let want: Vec<String> = (0..n).map(|_| address(&mut r)).collect();
        let entries: Vec<String> = want.iter().map(|a| entry(&mut r, a)).collect();
        let mut list = String::new();
        let mut i = 0;
        while i < entries.len() {
            if !list.is_empty() {
                list.push_str(r.pick(&[",", ", ", " ,\t", ",\r\n "]));
            }
            match r.below(5) {
                // A group of the next one or two entries.
                0 => {
                    let k = (1 + r.below(2)).min(entries.len() - i);
                    list.push_str("Team: ");
                    list.push_str(&entries[i..i + k].join(", "));
                    list.push(';');
                    i += k;
                }
                // An empty group, as in `undisclosed-recipients:;`.
                1 => {
                    list.push_str("undisclosed-recipients:;");
                    if !list.ends_with(',') {
                        list.push(',');
                    }
                    list.push_str(&entries[i]);
                    i += 1;
                }
                _ => {
                    list.push_str(&entries[i]);
                    i += 1;
                }
            }
        }
        if r.below(4) == 0 {
            list.push_str(", ");
        }
        let got: Vec<String> = addresses(&list).into_iter().map(|(a, _)| a).collect();
        let want: Vec<String> = want.iter().map(|a| a.to_ascii_lowercase()).collect();
        assert_eq!(got, want, "case {case}: {list:?}");
    }
}

/// For random text of the signs that the parser reads, with letters of
/// more than one byte, the parser does not panic, and each address that
/// it gives has an `@`, no space at its ends and no upper case ASCII.
/// Text that is not a list can give an entry that is not a valid address:
/// each caller checks the address (the reply form drops it with the
/// address check of R3-01).
#[test]
fn property_any_text_is_safe() {
    const SIGNS: &[&str] = &[
        "a",
        "Z",
        "@",
        ".",
        ",",
        ";",
        ":",
        "<",
        ">",
        "(",
        ")",
        "\"",
        "\\",
        " ",
        "\t",
        "é",
        "名",
        "😀",
        "example.com",
        "=?UTF-8?B?",
        "?=",
    ];
    let mut r = Rng(0x2545_f491_4f6c_dd1d);
    for _ in 0..20_000 {
        let n = r.below(24);
        let text: String = (0..n).map(|_| *r.pick(SIGNS)).collect();
        for (a, _) in addresses(&text) {
            assert!(a.contains('@'), "{text:?} gave {a:?}");
            assert_eq!(a, a.trim(), "{text:?}");
            assert_eq!(a, a.to_ascii_lowercase(), "{text:?}");
        }
    }
}

/// The comment name of a bare address keeps its quoted pairs and its
/// nested comments (issue #780).
#[test]
fn comment_name_reads_quoted_pairs_and_nested_comments() {
    for (list, want) in [
        ("a@example.org (Ana \\) Lee)", "Ana ) Lee"),
        ("a@example.org (Ana \\( Lee)", "Ana ( Lee"),
        ("a@example.org (Ana (work) Lee)", "Ana (work) Lee"),
        ("a@example.org (Ana Lee), b@example.org", "Ana Lee"),
    ] {
        let got = addresses(list);
        assert_eq!(got[0].0, "a@example.org", "{list:?}");
        assert_eq!(got[0].1.as_deref(), Some(want), "{list:?}");
    }
    // An open comment gives no name, and the address stays.
    assert_eq!(
        addresses("a@example.org (Ana"),
        vec![("a@example.org".into(), None)]
    );
}

/// A domain literal holds `:` and `,` that are not list syntax. A `[` that
/// does not follow `@`, or has no `]`, is text (issue #83).
#[test]
fn domain_literals() {
    let got = |l: &str| addresses(l).into_iter().map(|(a, _)| a).collect::<Vec<_>>();
    assert_eq!(
        got("user@[IPv6:2001:db8::1], b@example.org"),
        ["user@[ipv6:2001:db8::1]", "b@example.org"]
    );
    assert_eq!(
        got("T: <u@[IPv6:2001:db8::1]>, c@x.example;"),
        ["u@[ipv6:2001:db8::1]", "c@x.example"]
    );
    assert_eq!(
        got("Ana [work] <a@x.example>, b@y.example"),
        ["a@x.example", "b@y.example"]
    );
    assert_eq!(got("a@[x, b@y.example"), ["a@[x", "b@y.example"]);
}

/// Many open domain literals with no `]` cost one pass, not a scan for
/// each `[` (a hostile header can hold thousands).
#[test]
fn open_domain_literals_are_linear() {
    let list = "a@[x, ".repeat(200_000);
    let start = std::time::Instant::now();
    let got = addresses(&list);
    assert_eq!(got.len(), 200_000);
    assert!(
        start.elapsed() < std::time::Duration::from_secs(5),
        "{:?}",
        start.elapsed()
    );
}

/// A quoted local part in `<>` can hold `<`, `>` and `,`: they are text, not
/// list syntax (issue #980). A quote in `<>` with no closing `"@` is text,
/// so an open quote cannot hide the rest of the list.
#[test]
fn quoted_local_parts_in_angle_brackets() {
    let got = |l: &str| addresses(l).into_iter().map(|(a, _)| a).collect::<Vec<_>>();
    assert_eq!(
        got("Ana <\"a<b\"@example.org>, c@example.org"),
        ["\"a<b\"@example.org", "c@example.org"]
    );
    assert_eq!(
        got("<\"a>, b\"@example.org>, c@example.org"),
        ["\"a>, b\"@example.org", "c@example.org"]
    );
    assert_eq!(
        got("< \"x\\\"<y\"@example.org>; d@example.org"),
        ["\"x\\\"<y\"@example.org", "d@example.org"]
    );
    // An open quote: the rest of the list stays.
    assert_eq!(
        got("<\"a@example.org>, c@example.org, \"Bo\" <d@example.org>"),
        ["\"a@example.org", "c@example.org", "d@example.org"]
    );
    // The display name of a quoted local part is kept.
    assert_eq!(
        addresses("Ana Lee <\"a>b\"@example.org>")[0].1.as_deref(),
        Some("Ana Lee")
    );
}

/// Many `<"` with no closing quote cost one bounded scan each, not a scan
/// of the whole list.
#[test]
fn open_quoted_local_parts_are_linear() {
    let list = "<\"a, ".repeat(200_000);
    let start = std::time::Instant::now();
    let _ = addresses(&list);
    assert!(
        start.elapsed() < std::time::Duration::from_secs(5),
        "{:?}",
        start.elapsed()
    );
}

/// A comment and white space can come before a quoted local part in `<>`.
#[test]
fn a_comment_before_a_quoted_local_part() {
    let got = |l: &str| addresses(l).into_iter().map(|(a, _)| a).collect::<Vec<_>>();
    assert_eq!(
        got("<(note)\"a<b\"@example.org>, c@example.org"),
        ["\"a<b\"@example.org", "c@example.org"]
    );
    assert_eq!(
        got("Ana < (x) (y\\)) \"a>, b\"@example.org>"),
        ["\"a>, b\"@example.org"]
    );
}
